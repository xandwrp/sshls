use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

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
        let override_dir = absolute_path_override("SSHLS_DIR")?;
        let custom_dir = override_dir.is_some();
        let ssh_dir = override_dir.unwrap_or_else(|| home.join(".ssh"));
        let ssh_binary = env::var_os("SSHLS_SSH_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ssh"));
        if ssh_binary.as_os_str().is_empty() {
            bail!("SSHLS_SSH_BIN must not be empty");
        }
        let mut paths = Self::from_parts(home.clone(), ssh_dir, ssh_binary, custom_dir);
        paths
            .include_target()
            .context("validating SSHLS_DIR for use in OpenSSH configuration")?;
        paths.ssh_config_override = absolute_path_override("SSHLS_USER_CONFIG")?;
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

    pub fn include_target(&self) -> Result<String> {
        if self.custom_dir {
            quote_path(&self.generated_config)
        } else {
            Ok("~/.ssh/sshls.conf".into())
        }
    }

    pub fn include_directive(&self) -> Result<String> {
        Ok(format!("Include {}", self.include_target()?))
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
        let expected = self.include_target()?;
        for line in contents.lines() {
            let line = strip_comment(line).trim();
            let Some((keyword, value)) = split_keyword(line) else {
                continue;
            };
            if keyword.eq_ignore_ascii_case("host") || keyword.eq_ignore_ascii_case("match") {
                return Ok(false);
            }
            if keyword.eq_ignore_ascii_case("include") && path_text_eq(value, &expected) {
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

fn absolute_path_override(name: &str) -> Result<Option<PathBuf>> {
    let Some(path) = env::var_os(name).map(PathBuf::from) else {
        return Ok(None);
    };
    if path.as_os_str().is_empty() || !path.is_absolute() {
        bail!("{name} must be an absolute path");
    }
    Ok(Some(path))
}

fn quote_path(path: &Path) -> Result<String> {
    let value = path
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
    let value = if cfg!(windows) {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    };
    if value.chars().any(char::is_control)
        || value.contains('"')
        || value.contains("${")
        || value.contains(['*', '?', '[', ']'])
        || (cfg!(unix) && value.contains('\\'))
    {
        bail!(
            "{} contains characters that are unsafe in an OpenSSH Include directive",
            path.display()
        );
    }
    let value = value.replace('%', "%%");
    if value.chars().any(char::is_whitespace) || value.contains('#') {
        Ok(format!("\"{value}\""))
    } else {
        Ok(value)
    }
}

fn split_keyword(line: &str) -> Option<(&str, &str)> {
    let index = line.find(|character: char| character.is_whitespace() || character == '=')?;
    let keyword = &line[..index];
    let remainder = line[index..].trim_start();
    let value = remainder
        .strip_prefix('=')
        .unwrap_or(remainder)
        .trim_start();
    (!keyword.is_empty() && !value.is_empty()).then_some((keyword, value))
}

fn path_text_eq(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
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
        assert_eq!(
            paths.include_directive().unwrap(),
            "Include ~/.ssh/sshls.conf"
        );
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
            paths.include_directive().unwrap(),
            "Include \"C:/Some Place/ssh/sshls.conf\""
        );
    }

    #[test]
    fn custom_location_quotes_comments_and_escapes_tokens() {
        let paths = AppPaths::from_parts(
            PathBuf::from("C:/Users/Me"),
            PathBuf::from("C:/Hash#100%/ssh"),
            PathBuf::from("ssh"),
            true,
        );
        assert_eq!(
            paths.include_directive().unwrap(),
            "Include \"C:/Hash#100%%/ssh/sshls.conf\""
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
            format!(
                "Host unrelated\n    {}\n",
                paths.include_directive().unwrap()
            ),
        )
        .unwrap();
        assert!(!paths.user_config_has_include().unwrap());

        std::fs::write(
            &paths.user_config,
            format!(
                "{} # managed\nHost unrelated\n",
                paths.include_directive().unwrap()
            ),
        )
        .unwrap();
        assert!(paths.user_config_has_include().unwrap());
    }

    #[test]
    fn include_accepts_openssh_equals_syntax() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_parts(
            temporary.path().to_owned(),
            temporary.path().to_owned(),
            PathBuf::from("ssh"),
            true,
        );
        let target = paths.include_target().unwrap();
        std::fs::write(&paths.user_config, format!("Include = {target}\n")).unwrap();
        assert!(paths.user_config_has_include().unwrap());

        std::fs::write(&paths.user_config, format!("Include={target}\n")).unwrap();
        assert!(paths.user_config_has_include().unwrap());
    }
}
