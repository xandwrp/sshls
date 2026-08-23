use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use fs4::{FileExt, TryLockError};
use tempfile::NamedTempFile;

use crate::inventory::Inventory;
use crate::openssh::generate_config;
use crate::paths::AppPaths;

#[derive(Debug)]
pub struct Store {
    paths: AppPaths,
}

impl Store {
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    pub fn load(&self) -> Result<Inventory> {
        let contents = match fs::read_to_string(&self.paths.inventory) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Inventory::default());
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reading {}", self.paths.inventory.display()));
            }
        };
        let mut inventory: Inventory = toml::from_str(&contents)
            .with_context(|| format!("parsing {}", self.paths.inventory.display()))?;
        inventory.validate()?;
        inventory.normalize();
        Ok(inventory)
    }

    pub fn lock(&self) -> Result<File> {
        ensure_directory(&self.paths)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&self.paths.lock)
            .with_context(|| format!("opening {}", self.paths.lock.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .with_context(|| format!("securing {}", self.paths.lock.display()))?;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match FileExt::try_lock(&file) {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(TryLockError::WouldBlock) => {
                    anyhow::bail!(
                        "timed out waiting for another sshls process ({})",
                        self.paths.lock.display()
                    );
                }
                Err(TryLockError::Error(error)) => {
                    return Err(error)
                        .with_context(|| format!("locking {}", self.paths.lock.display()));
                }
            }
        }
        Ok(file)
    }

    pub fn save_and_sync(&self, inventory: &Inventory) -> Result<()> {
        inventory.validate()?;
        ensure_directory(&self.paths)?;
        let serialized = toml::to_string_pretty(inventory).context("serializing inventory")?;
        atomic_write(&self.paths.inventory, serialized.as_bytes())?;
        self.sync(inventory).context(
            "the inventory was saved, but the generated config could not be updated; run `sshls config sync`",
        )
    }

    pub fn regenerate(&self) -> Result<()> {
        let _lock = self.lock()?;
        let inventory = self.load()?;
        self.sync(&inventory)
    }

    pub fn sync(&self, inventory: &Inventory) -> Result<()> {
        inventory.validate()?;
        ensure_directory(&self.paths)?;
        let generated = generate_config(inventory);
        atomic_write(&self.paths.generated_config, generated.as_bytes())
    }
}

fn ensure_directory(paths: &AppPaths) -> Result<()> {
    if paths.ssh_dir.exists() {
        return Ok(());
    }
    fs::create_dir_all(&paths.ssh_dir)
        .with_context(|| format!("creating {}", paths.ssh_dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&paths.ssh_dir, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("securing {}", paths.ssh_dir.display()))?;
    }
    Ok(())
}

fn atomic_write(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating a temporary file in {}", parent.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))
            .context("securing temporary file")?;
    }

    temporary
        .write_all(contents)
        .with_context(|| format!("writing temporary file for {}", path.display()))?;
    temporary.flush().context("flushing temporary file")?;
    temporary
        .as_file()
        .sync_all()
        .context("syncing temporary file")?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("replacing {}", path.display()))?;

    #[cfg(unix)]
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .with_context(|| format!("syncing {}", parent.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::inventory::Host;

    fn test_store(directory: &std::path::Path) -> Store {
        Store::new(AppPaths::from_parts(
            directory.to_owned(),
            directory.join(".ssh"),
            PathBuf::from("ssh"),
            true,
        ))
    }

    #[test]
    fn repeated_atomic_writes_replace_existing_files() {
        let temporary = tempfile::tempdir().unwrap();
        let store = test_store(temporary.path());
        let mut inventory = Inventory::default();
        store.save_and_sync(&inventory).unwrap();
        inventory
            .add(Host {
                name: "prod".into(),
                hostname: "example.com".into(),
                user: None,
                port: None,
                identity_file: None,
                tags: vec![],
            })
            .unwrap();
        store.save_and_sync(&inventory).unwrap();
        assert_eq!(store.load().unwrap().hosts.len(), 1);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let temporary = tempfile::tempdir().unwrap();
        let store = test_store(temporary.path());
        fs::create_dir_all(&store.paths.ssh_dir).unwrap();
        fs::write(&store.paths.inventory, "version = 1\nunexpected = true\n").unwrap();
        assert!(store.load().is_err());
    }
}
