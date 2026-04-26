//! The bestiary catalog: embedded entries from `apps/*.yaml` plus per-user
//! overrides at `~/.config/bestiary/apps/*.yaml`. Personal entries with the
//! same `name` as an embedded entry win.

use crate::creature::{Creature, Flavor};
use anyhow::{Context, Result};
use include_dir::{Dir, include_dir};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

static EMBEDDED_CREATURES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/apps");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Embedded,
    Personal,
}

#[derive(Debug, Clone)]
pub struct CatalogEntry {
    pub creature: Creature,
    pub source: Source,
}

pub struct Catalog {
    entries: BTreeMap<String, CatalogEntry>,
}

impl Catalog {
    /// Load embedded + personal entries.
    pub fn load() -> Result<Self> {
        let mut entries = load_embedded()?;
        if let Some(dir) = personal_apps_dir() {
            merge_personal(&mut entries, &dir)?;
        }
        Ok(Self { entries })
    }

    /// Embedded entries only — for tests and offline tooling.
    pub fn embedded_only() -> Result<Self> {
        Ok(Self {
            entries: load_embedded()?,
        })
    }

    #[cfg(test)]
    pub fn from_creatures(creatures: Vec<Creature>) -> Self {
        let entries = creatures
            .into_iter()
            .map(|c| {
                (
                    c.name.clone(),
                    CatalogEntry {
                        creature: c,
                        source: Source::Embedded,
                    },
                )
            })
            .collect();
        Self { entries }
    }

    pub fn get(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.get(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &CatalogEntry)> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Look up the creature whose dwelling-paths cover `query`. The longest
    /// matching path wins, so an entry for `~/.config/discord` matches a
    /// query of `~/.config/discord/settings.json`.
    pub fn lookup_path(&self, query: &Path) -> Option<&CatalogEntry> {
        let query = expand_tilde(query);
        let query_str = query.to_string_lossy();
        let mut best: Option<(usize, &CatalogEntry)> = None;
        for entry in self.entries.values() {
            for dwelling in entry.creature.dwellings.values() {
                for (_kind, raw) in dwelling.paths() {
                    let resolved = expand_tilde(Path::new(raw));
                    let resolved_str = resolved.to_string_lossy();
                    if query_str == resolved_str
                        || query_str.starts_with(&format!("{resolved_str}/"))
                    {
                        let len = resolved_str.len();
                        if best.map(|(b, _)| b < len).unwrap_or(true) {
                            best = Some((len, entry));
                        }
                    }
                }
            }
        }
        best.map(|(_, e)| e)
    }

    /// Translate a path under one flavor's dwelling to the equivalent path
    /// under `target` for the same creature. Returns `None` if no creature
    /// owns the input path or the target flavor isn't known for that creature.
    pub fn map_flavor(&self, source: &Path, target: Flavor) -> Option<PathBuf> {
        let entry = self.lookup_path(source)?;
        let source_expanded = expand_tilde(source);
        let source_str = source_expanded.to_string_lossy().into_owned();

        // Find which (flavor, kind, base) the source falls under.
        for dwelling in entry.creature.dwellings.values() {
            for (kind, raw) in dwelling.paths() {
                let base = expand_tilde(Path::new(raw));
                let base_str = base.to_string_lossy();
                if source_str == base_str {
                    // Exact dwelling root → translate to the target's
                    // matching kind.
                    let target_dwelling = entry.creature.dwellings.get(&target)?;
                    let target_path = match kind {
                        crate::creature::Kind::Config => target_dwelling.config.as_deref(),
                        crate::creature::Kind::Data => target_dwelling.data.as_deref(),
                        crate::creature::Kind::Cache => target_dwelling.cache.as_deref(),
                        crate::creature::Kind::State => target_dwelling.state.as_deref(),
                    }?;
                    return Some(expand_tilde(Path::new(target_path)));
                }
                if source_str.starts_with(&format!("{base_str}/")) {
                    // Sub-path of a dwelling root → translate the prefix.
                    let suffix = &source_str[base_str.len()..];
                    let target_dwelling = entry.creature.dwellings.get(&target)?;
                    let target_base = match kind {
                        crate::creature::Kind::Config => target_dwelling.config.as_deref(),
                        crate::creature::Kind::Data => target_dwelling.data.as_deref(),
                        crate::creature::Kind::Cache => target_dwelling.cache.as_deref(),
                        crate::creature::Kind::State => target_dwelling.state.as_deref(),
                    }?;
                    let mut out = expand_tilde(Path::new(target_base));
                    if let Some(suf) = suffix.strip_prefix('/') {
                        out.push(suf);
                    }
                    return Some(out);
                }
            }
        }
        None
    }
}

fn load_embedded() -> Result<BTreeMap<String, CatalogEntry>> {
    let mut out = BTreeMap::new();
    for f in EMBEDDED_CREATURES.files() {
        if f.path().extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let yaml = f
            .contents_utf8()
            .with_context(|| format!("embedded creature {} not UTF-8", f.path().display()))?;
        let creature = Creature::from_yaml(yaml)
            .with_context(|| format!("embedded creature {}", f.path().display()))?;
        let stem = f
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem != creature.name {
            anyhow::bail!(
                "embedded creature {} declares name {:?} but filename stem is {:?}",
                f.path().display(),
                creature.name,
                stem,
            );
        }
        out.insert(
            creature.name.clone(),
            CatalogEntry {
                creature,
                source: Source::Embedded,
            },
        );
    }
    Ok(out)
}

fn merge_personal(into: &mut BTreeMap<String, CatalogEntry>, dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading personal apps dir {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("yaml") {
            continue;
        }
        let creature = Creature::from_file(&path)?;
        into.insert(
            creature.name.clone(),
            CatalogEntry {
                creature,
                source: Source::Personal,
            },
        );
    }
    Ok(())
}

fn personal_apps_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bestiary").join("apps"))
}

fn expand_tilde(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if s == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Dwelling, Flavor};

    fn fake(name: &str, native_config: &str, flatpak_config: Option<(&str, &str)>) -> Creature {
        let mut dwellings = BTreeMap::new();
        dwellings.insert(
            Flavor::Native,
            Dwelling {
                config: Some(native_config.to_string()),
                ..Default::default()
            },
        );
        if let Some((id, path)) = flatpak_config {
            dwellings.insert(
                Flavor::Flatpak,
                Dwelling {
                    flatpak_id: Some(id.to_string()),
                    config: Some(path.to_string()),
                    ..Default::default()
                },
            );
        }
        Creature {
            name: name.into(),
            display_name: None,
            category: None,
            homepage: None,
            dwellings,
            backup_exclude: vec![],
            tags: vec![],
        }
    }

    #[test]
    fn lookup_path_matches_dwelling_root() {
        let cat = Catalog::from_creatures(vec![fake("foo", "/home/me/.config/foo", None)]);
        let hit = cat.lookup_path(Path::new("/home/me/.config/foo")).unwrap();
        assert_eq!(hit.creature.name, "foo");
    }

    #[test]
    fn lookup_path_matches_subpath() {
        let cat = Catalog::from_creatures(vec![fake("foo", "/home/me/.config/foo", None)]);
        let hit = cat
            .lookup_path(Path::new("/home/me/.config/foo/themes/dark.json"))
            .unwrap();
        assert_eq!(hit.creature.name, "foo");
    }

    #[test]
    fn lookup_path_misses_unrelated() {
        let cat = Catalog::from_creatures(vec![fake("foo", "/home/me/.config/foo", None)]);
        assert!(cat.lookup_path(Path::new("/home/me/.config/bar")).is_none());
    }

    #[test]
    fn lookup_path_picks_longest_match() {
        // foo lives at /home/me, foo-extras lives at /home/me/extras.
        // /home/me/extras/data should match foo-extras, not foo.
        let cat = Catalog::from_creatures(vec![
            fake("foo", "/home/me", None),
            fake("foo-extras", "/home/me/extras", None),
        ]);
        let hit = cat.lookup_path(Path::new("/home/me/extras/data")).unwrap();
        assert_eq!(hit.creature.name, "foo-extras");
    }

    #[test]
    fn map_flavor_translates_native_to_flatpak() {
        let cat = Catalog::from_creatures(vec![fake(
            "foo",
            "/home/me/.config/foo",
            Some(("com.foo.Foo", "/home/me/.var/app/com.foo.Foo/config/foo")),
        )]);
        let mapped = cat
            .map_flavor(Path::new("/home/me/.config/foo"), Flavor::Flatpak)
            .unwrap();
        assert_eq!(
            mapped,
            PathBuf::from("/home/me/.var/app/com.foo.Foo/config/foo")
        );
    }

    #[test]
    fn map_flavor_preserves_subpath() {
        let cat = Catalog::from_creatures(vec![fake(
            "foo",
            "/home/me/.config/foo",
            Some(("com.foo.Foo", "/home/me/.var/app/com.foo.Foo/config/foo")),
        )]);
        let mapped = cat
            .map_flavor(
                Path::new("/home/me/.config/foo/themes/dark.json"),
                Flavor::Flatpak,
            )
            .unwrap();
        assert_eq!(
            mapped,
            PathBuf::from("/home/me/.var/app/com.foo.Foo/config/foo/themes/dark.json")
        );
    }
}
