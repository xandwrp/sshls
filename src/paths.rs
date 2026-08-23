use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct AppPaths {
    pub home: PathBuf,
    pub ssh_dir: PathBuf,
    pub inventory: PathBuf,
    pub generated_config: PathBuf,
    pub user_config: PathBuf,
    pub lock: PathBuf,
    pub ssh_binary: PathBuf,
    pub ssh_config_override: Option<PathBuf>,
    custom_dir: bool,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let home =
            dirs::home_dir().context("could not determine the current user's home directory")?;
        let override_dir = env::var_os("SSHLS_DIR").map(PathBuf::from);
        let custom_dir = override_dir.is_some();
        let ssh_dir = override_dir.unwrap_or_else(|| home.join(".ssh"));
        let ssh_binary = env::var_os("SSHLS_SSH_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ssh"));
        let mut paths = Self::from_parts(home.clone(), ssh_dir, ssh_binary, custom_dir);
        paths.ssh_config_override = env::var_os("SSHLS_USER_CONFIG").map(PathBuf::from);
        paths.user_config = paths
            .ssh_config_override
            .clone()
            .unwrap_or_else(|| home.join(".ssh").join("config"));
        Ok(paths)
    }

    pub fn from_parts(
        home: PathBuf,
        ssh_dir: PathBuf,
        ssh_binary: PathBuf,
        custom_dir: bool,
    ) -> Self {
        Self {
            home,
            inventory: ssh_dir.join("sshls.toml"),
            generated_config: ssh_dir.join("sshls.conf"),
            user_config: ssh_dir.join("config"),
            lock: ssh_dir.join(".sshls.lock"),
            ssh_dir,
            ssh_binary,
            ssh_config_override: None,
            custom_dir,
        }
    }

    pub fn include_target(&self) -> String {
        if self.custom_dir {
            quote_path(&self.generated_config)
        } else {
            "~/.ssh/sshls.conf".into()
        }
    }

    pub fn include_directive(&self) -> String {
        format!("Include {}", self.include_target())
    }

    pub fn user_config_has_include(&self) -> Result<bool> {
        let contents = match std::fs::read_to_string(&self.user_config) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reading {}", self.user_config.display()));
            }
        };
        let expected = self.include_target();
        for line in contents.lines() {
            let line = strip_comment(line).trim();
            let Some((keyword, value)) = split_keyword(line) else {
                continue;
            };
            if keyword.eq_ignore_ascii_case("host") || keyword.eq_ignore_ascii_case("match") {
                return Ok(false);
            }
            if keyword.eq_ignore_ascii_case("include") && value.eq_ignore_ascii_case(&expected) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub fn expand_identity(&self, identity: &str) -> PathBuf {
        if identity == "~" {
            self.home.clone()
        } else if let Some(rest) = identity
            .strip_prefix("~/")
            .or_else(|| identity.strip_prefix("~\\"))
        {
            self.home.join(rest)
        } else {
            let path = Path::new(identity);
            if path.is_absolute() {
                path.to_owned()
            } else {
                self.home.join(path)
            }
        }
    }
}

fn quote_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if value.chars().any(char::is_whitespace) {
        format!("\"{value}\"")
    } else {
        value
    }
}

fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let index = line.find(char::is_whitespace)?;
    Some((&line[..index], line[index..].trim()))
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else if character == '#' && !quoted {
            return &line[..index];
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_location_uses_portable_tilde_include() {
        let paths = AppPaths::from_parts(
            PathBuf::from("/home/me"),
            PathBuf::from("/home/me/.ssh"),
            PathBuf::from("ssh"),
            false,
        );
        assert_eq!(paths.include_directive(), "Include ~/.ssh/sshls.conf");
    }

    #[test]
    fn custom_location_quotes_spaces() {
        let paths = AppPaths::from_parts(
            PathBuf::from("C:/Users/Me"),
            PathBuf::from("C:/Some Place/ssh"),
            PathBuf::from("ssh"),
            true,
        );
        assert_eq!(
            paths.include_directive(),
            "Include \"C:/Some Place/ssh/sshls.conf\""
        );
    }

    #[test]
    fn comments_inside_quoted_paths_are_not_stripped() {
        assert_eq!(
            strip_comment("Include \"C:/A # Place/file\" # comment"),
            "Include \"C:/A # Place/file\" "
        );
    }

    #[test]
    fn include_must_be_global() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_parts(
            temporary.path().to_owned(),
            temporary.path().to_owned(),
            PathBuf::from("ssh"),
            true,
        );
        std::fs::write(
            &paths.user_config,
            format!("Host unrelated\n    {}\n", paths.include_directive()),
        )
        .unwrap();
        assert!(!paths.user_config_has_include().unwrap());

        std::fs::write(
            &paths.user_config,
            format!("{} # managed\nHost unrelated\n", paths.include_directive()),
        )
        .unwrap();
        assert!(paths.user_config_has_include().unwrap());
    }
}
