//! A bestiary entry — one application and where it stores its data.
//!
//! Mirrors `creature.schema.json`. Personal entries from
//! `~/.config/bestiary/creatures/*.yaml` use the same schema and override
//! embedded entries by name match.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// One application's bestiary entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Creature {
    /// Unique kebab-case identifier (e.g. `discord`, `vscode`, `android-studio`).
    pub name: String,

    /// Human-readable name (e.g. "Discord", "Visual Studio Code").
    /// Defaults to a Title-Cased rendering of `name` if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Free-form grouping — `communication`, `development`, `media`, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,

    /// Project homepage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,

    /// Per-flavor data locations. Keys are install variants
    /// (`native`, `flatpak`, `snap`, `legacy`); values are the paths
    /// the app uses under that variant. Stored in YAML as `locations:`.
    #[serde(default, rename = "locations")]
    pub dwellings: BTreeMap<Flavor, Dwelling>,

    /// Glob patterns to skip when this creature is bundled into a backup.
    /// Relative to each dwelling's paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backup_exclude: Vec<String>,

    /// Free-form tags for search and filtering.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Flavor {
    /// Standard package install or upstream tarball — XDG dirs.
    Native,
    /// `~/.var/app/<flatpak-id>/...`
    Flatpak,
    /// `~/snap/<snap-name>/...`
    Snap,
    /// Pre-XDG `~/.appname` style — kept for migration assistance.
    Legacy,
}

impl Flavor {
    pub fn as_str(self) -> &'static str {
        match self {
            Flavor::Native => "native",
            Flavor::Flatpak => "flatpak",
            Flavor::Snap => "snap",
            Flavor::Legacy => "legacy",
        }
    }
}

/// Where one flavor of a creature stores its data.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Dwelling {
    /// Flatpak app id (e.g. `com.discordapp.Discord`). Required when this
    /// dwelling's parent flavor is `flatpak`; otherwise omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatpak_id: Option<String>,

    /// Snap name (e.g. `slack`). Required when parent flavor is `snap`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap_name: Option<String>,

    /// User config (XDG_CONFIG_HOME / `~/.config/<app>` typically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,

    /// User data (XDG_DATA_HOME / `~/.local/share/<app>` typically).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,

    /// Cache (regenerable, safe to drop).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,

    /// Per-machine state (logs, cookies, sockets — XDG_STATE_HOME).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// Kind of data stored at a path. Used by callers to decide what to back up
/// vs. what to ignore (cache is always skippable).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Config,
    Data,
    Cache,
    State,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Config => "config",
            Kind::Data => "data",
            Kind::Cache => "cache",
            Kind::State => "state",
        }
    }
}

impl Dwelling {
    /// Iterate over the (kind, path) pairs that are populated.
    pub fn paths(&self) -> Vec<(Kind, &str)> {
        let mut out = Vec::new();
        if let Some(p) = &self.config {
            out.push((Kind::Config, p.as_str()));
        }
        if let Some(p) = &self.data {
            out.push((Kind::Data, p.as_str()));
        }
        if let Some(p) = &self.cache {
            out.push((Kind::Cache, p.as_str()));
        }
        if let Some(p) = &self.state {
            out.push((Kind::State, p.as_str()));
        }
        out
    }
}

impl Creature {
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        let creature: Creature = serde_yml::from_str(yaml).context("parsing creature YAML")?;
        creature.validate()?;
        Ok(creature)
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let yaml = std::fs::read_to_string(path)
            .with_context(|| format!("reading creature {}", path.display()))?;
        let creature = Self::from_yaml(&yaml)
            .with_context(|| format!("loading creature {}", path.display()))?;
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && stem != creature.name
        {
            bail!(
                "creature file {} declares name {:?} but filename stem is {:?}",
                path.display(),
                creature.name,
                stem,
            );
        }
        Ok(creature)
    }

    fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;

        if let Some(d) = self.dwellings.get(&Flavor::Flatpak)
            && d.flatpak_id.is_none()
        {
            bail!(
                "creature {:?}: flatpak dwelling has no flatpak_id",
                self.name
            );
        }
        if let Some(d) = self.dwellings.get(&Flavor::Snap)
            && d.snap_name.is_none()
        {
            bail!("creature {:?}: snap dwelling has no snap_name", self.name);
        }

        // Every dwelling should have at least one populated path.
        for (flavor, d) in &self.dwellings {
            if d.paths().is_empty() {
                bail!(
                    "creature {:?}: {} dwelling has no paths set",
                    self.name,
                    flavor.as_str()
                );
            }
        }
        Ok(())
    }

    /// Display name, falling back to a Title-Cased rendering of `name`.
    pub fn pretty_name(&self) -> String {
        if let Some(d) = &self.display_name {
            d.clone()
        } else {
            self.name
                .split('-')
                .map(|w| {
                    let mut chars = w.chars();
                    match chars.next() {
                        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    }
}

fn validate_name(name: &str) -> Result<()> {
    if !name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        bail!("creature name {name:?} must start with a lowercase letter");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("creature name {name:?} must match [a-z][a-z0-9-]*");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn discord() -> &'static str {
        r#"
name: discord
display_name: Discord
category: communication
homepage: https://discord.com
locations:
  native:
    config: ~/.config/discord
    cache: ~/.config/discord/Cache
  flatpak:
    flatpak_id: com.discordapp.Discord
    config: ~/.var/app/com.discordapp.Discord/config/discord
    cache: ~/.var/app/com.discordapp.Discord/cache/discord
backup_exclude: ["Cache/*", "GPUCache/*"]
"#
    }

    #[test]
    fn parses_a_typical_creature() {
        let c = Creature::from_yaml(discord()).unwrap();
        assert_eq!(c.name, "discord");
        assert_eq!(c.pretty_name(), "Discord");
        assert_eq!(c.dwellings.len(), 2);
        assert_eq!(
            c.dwellings[&Flavor::Flatpak].flatpak_id.as_deref(),
            Some("com.discordapp.Discord")
        );
    }

    #[test]
    fn pretty_name_falls_back_to_titlecase() {
        let yaml = r#"
name: android-studio
locations:
  native:
    data: ~/.local/share/android-studio
"#;
        let c = Creature::from_yaml(yaml).unwrap();
        assert_eq!(c.pretty_name(), "Android Studio");
    }

    #[test]
    fn flatpak_dwelling_must_declare_id() {
        let yaml = r#"
name: x
locations:
  flatpak:
    config: ~/.var/app/x/config
"#;
        assert!(Creature::from_yaml(yaml).is_err());
    }

    #[test]
    fn dwelling_must_have_at_least_one_path() {
        let yaml = r#"
name: x
locations:
  native:
    flatpak_id: foo
"#;
        let err = Creature::from_yaml(yaml).unwrap_err();
        assert!(err.to_string().contains("no paths"));
    }

    #[test]
    fn rejects_bad_name() {
        let yaml = r#"
name: Discord
locations:
  native:
    config: ~/.config/discord
"#;
        assert!(Creature::from_yaml(yaml).is_err());
    }

    #[test]
    fn rejects_unknown_field() {
        let yaml = r#"
name: x
mystery: 42
locations:
  native:
    config: ~/.config/x
"#;
        assert!(Creature::from_yaml(yaml).is_err());
    }
}
