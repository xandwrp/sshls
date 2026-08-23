use std::collections::HashSet;
use std::path::Path;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub version: u32,
    #[serde(default)]
    pub hosts: Vec<Host>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            hosts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
    pub name: String,
    pub hostname: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl Inventory {
    pub fn validate(&self) -> Result<()> {
        if self.version != SCHEMA_VERSION {
            bail!(
                "unsupported inventory version {} (this binary supports version {SCHEMA_VERSION})",
                self.version
            );
        }

        let mut names = HashSet::new();
        for host in &self.hosts {
            host.validate()?;
            if !names.insert(host.name.to_ascii_lowercase()) {
                bail!("duplicate destination name {:?}", host.name);
            }
        }
        Ok(())
    }

    pub fn normalize(&mut self) {
        for host in &mut self.hosts {
            host.tags.sort_by_key(|tag| tag.to_ascii_lowercase());
            host.tags
                .dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        }
        self.hosts.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
                .then_with(|| left.name.cmp(&right.name))
        });
    }

    pub fn get(&self, name: &str) -> Result<&Host> {
        self.hosts
            .iter()
            .find(|host| host.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("unknown destination {name:?}"))
    }

    pub fn get_mut(&mut self, name: &str) -> Result<&mut Host> {
        self.hosts
            .iter_mut()
            .find(|host| host.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("unknown destination {name:?}"))
    }

    pub fn add(&mut self, host: Host) -> Result<()> {
        validate_name(&host.name)?;
        if self
            .hosts
            .iter()
            .any(|existing| existing.name.eq_ignore_ascii_case(&host.name))
        {
            bail!("destination {:?} already exists", host.name);
        }
        host.validate()?;
        self.hosts.push(host);
        Ok(())
    }

    pub fn clone_host(&mut self, source: &str, name: String) -> Result<()> {
        validate_name(&name)?;
        if self
            .hosts
            .iter()
            .any(|host| host.name.eq_ignore_ascii_case(&name))
        {
            bail!("destination {name:?} already exists");
        }
        let mut clone = self.get(source)?.clone();
        clone.name = name;
        self.hosts.push(clone);
        Ok(())
    }

    pub fn rename(&mut self, old: &str, new: String) -> Result<()> {
        validate_name(&new)?;
        let is_same = self
            .get(old)
            .is_ok_and(|host| host.name.eq_ignore_ascii_case(&new));
        if !is_same
            && self
                .hosts
                .iter()
                .any(|host| host.name.eq_ignore_ascii_case(&new))
        {
            bail!("destination {new:?} already exists");
        }
        self.get_mut(old)?.name = new;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        let index = self
            .hosts
            .iter()
            .position(|host| host.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("unknown destination {name:?}"))?;
        self.hosts.remove(index);
        Ok(())
    }

    pub fn add_tags(&mut self, name: &str, tags: Vec<String>) -> Result<()> {
        for tag in &tags {
            validate_tag(tag)?;
        }
        let host = self.get_mut(name)?;
        for tag in tags {
            if !host
                .tags
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&tag))
            {
                host.tags.push(tag);
            }
        }
        Ok(())
    }

    pub fn remove_tags(&mut self, name: &str, tags: &[String]) -> Result<()> {
        for tag in tags {
            validate_tag(tag)?;
        }
        let host = self.get_mut(name)?;
        host.tags.retain(|existing| {
            !tags
                .iter()
                .any(|removed| existing.eq_ignore_ascii_case(removed))
        });
        Ok(())
    }

    pub fn find(&self, text: &str) -> Vec<&Host> {
        let query = text.to_lowercase();
        self.hosts
            .iter()
            .filter(|host| {
                host.name.to_lowercase().contains(&query)
                    || host.hostname.to_lowercase().contains(&query)
                    || host
                        .user
                        .as_deref()
                        .is_some_and(|user| user.to_lowercase().contains(&query))
                    || host
                        .tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&query))
            })
            .collect()
    }
}

impl Host {
    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        validate_hostname(&self.hostname)?;
        if let Some(user) = &self.user {
            validate_user(user)?;
        }
        if self.port == Some(0) {
            bail!("destination {:?} has invalid port 0", self.name);
        }
        if let Some(identity) = &self.identity_file {
            if identity.is_empty() || identity.chars().any(|character| character.is_control()) {
                bail!("destination {:?} has an invalid identity path", self.name);
            }
            if identity.contains('"') {
                bail!(
                    "destination {:?} has an identity path containing a double quote",
                    self.name
                );
            }
            if identity.contains("${") {
                bail!(
                    "destination {:?} has an identity path containing OpenSSH environment expansion syntax",
                    self.name
                );
            }
            if cfg!(unix) && identity.contains('\\') {
                bail!(
                    "destination {:?} has an identity path containing a backslash, which OpenSSH may reinterpret",
                    self.name
                );
            }
            if !(Path::new(identity).is_absolute()
                || identity.starts_with("~/")
                || cfg!(windows) && identity.starts_with("~\\"))
            {
                bail!(
                    "destination {:?} has a relative identity path; use an absolute path or one beginning with ~/",
                    self.name
                );
            }
        }

        let mut tags = HashSet::new();
        for tag in &self.tags {
            validate_tag(tag)?;
            if !tags.insert(tag.to_ascii_lowercase()) {
                bail!("destination {:?} contains duplicate tag {tag:?}", self.name);
            }
        }
        Ok(())
    }

    pub fn destination(&self) -> String {
        let mut destination = String::new();
        if let Some(user) = &self.user {
            destination.push_str(user);
            destination.push('@');
        }
        if self.port.is_some() && self.hostname.contains(':') {
            destination.push('[');
            destination.push_str(&self.hostname);
            destination.push(']');
        } else {
            destination.push_str(&self.hostname);
        }
        if let Some(port) = self.port {
            destination.push(':');
            destination.push_str(&port.to_string());
        }
        destination
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 255 {
        bail!("destination name must contain between 1 and 255 characters");
    }
    if !name
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphanumeric())
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        bail!(
            "invalid destination name {name:?}; use ASCII letters, numbers, dots, underscores, or hyphens, starting with a letter or number"
        );
    }
    Ok(())
}

fn validate_tag(tag: &str) -> Result<()> {
    if tag.is_empty()
        || tag.len() > 64
        || !tag
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        bail!(
            "invalid tag {tag:?}; use 1-64 ASCII letters, numbers, dots, underscores, or hyphens"
        );
    }
    Ok(())
}

fn validate_hostname(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || ".-_:%".contains(character))
    {
        bail!(
            "hostname must use ASCII letters, numbers, dots, colons, underscores, percent signs, or hyphens"
        );
    }
    Ok(())
}

fn validate_user(value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        bail!("user must use ASCII letters, numbers, dots, underscores, or hyphens");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str) -> Host {
        Host {
            name: name.into(),
            hostname: "example.com".into(),
            user: None,
            port: None,
            identity_file: None,
            tags: Vec::new(),
        }
    }

    #[test]
    fn names_are_case_insensitively_unique() {
        let mut inventory = Inventory::default();
        inventory.add(host("Prod")).unwrap();
        assert!(inventory.add(host("prod")).is_err());
    }

    #[test]
    fn normalize_sorts_hosts_and_tags() {
        let mut first = host("zebra");
        first.tags = vec!["Web".into(), "prod".into(), "web".into()];
        let mut inventory = Inventory {
            version: 1,
            hosts: vec![first, host("Alpha")],
        };
        inventory.normalize();
        assert_eq!(inventory.hosts[0].name, "Alpha");
        assert_eq!(inventory.hosts[1].tags, ["prod", "Web"]);
    }

    #[test]
    fn rejects_config_injection() {
        let mut invalid = host("safe");
        invalid.hostname = "example.com\nProxyCommand bad".into();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn rejects_relative_identity_paths() {
        let mut invalid = host("safe");
        invalid.identity_file = Some(".ssh/id_ed25519".into());
        assert!(invalid.validate().is_err());

        invalid.identity_file = Some("~/.ssh/id_ed25519".into());
        assert!(invalid.validate().is_ok());
    }

    #[test]
    fn rejects_identity_environment_expansion() {
        let mut invalid = host("safe");
        invalid.identity_file = Some("~/.ssh/${KEY_FILE}".into());
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn find_searches_all_useful_fields() {
        let mut candidate = host("warehouse");
        candidate.hostname = "db.example.com".into();
        candidate.user = Some("deploy".into());
        candidate.tags = vec!["production".into()];
        let inventory = Inventory {
            version: 1,
            hosts: vec![candidate],
        };
        for query in ["WARE", "db.example", "PLOY", "duct"] {
            assert_eq!(inventory.find(query).len(), 1);
        }
    }
}
