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

    /// Look up the creature whose dwelling-paths cover `query`.
    ///
    /// Resolution strategy, in order:
    /// 1. **Longest declared-path prefix.** Exact match to a path on some
    ///    flavor's dwelling, or any descendant of it. Longest path wins.
    /// 2. **Flatpak-id sandbox match.** Query under `~/.var/app/<id>/...`
    ///    falls back to looking up `<id>` against any dwelling's
    ///    `flatpak_id`. Catches the bare sandbox dir without each entry
    ///    having to declare it explicitly.
    /// 3. **Snap-name sandbox match.** Same for `~/snap/<name>/...`.
    ///
    /// Declared paths may use a single `*` to match within one filename
    /// segment (e.g. `~/.zcompdump*` covers `~/.zcompdump-<host>-5.9`).
    /// Wildcards never cross `/`.
    pub fn lookup_path(&self, query: &Path) -> Option<&CatalogEntry> {
        let query = expand_tilde(query);
        let query_str = query.to_string_lossy();

        // 1. Longest declared-path prefix (with wildcard support).
        let mut best: Option<(usize, &CatalogEntry)> = None;
        for entry in self.entries.values() {
            for dwelling in entry.creature.dwellings.values() {
                for (_kind, raw) in dwelling.paths() {
                    let resolved = expand_tilde(Path::new(raw));
                    let resolved_str = resolved.to_string_lossy();
                    if let Some(score) = match_score(&resolved_str, &query_str)
                        && best.map(|(b, _)| b < score).unwrap_or(true)
                    {
                        best = Some((score, entry));
                    }
                }
            }
        }
        if let Some((_, e)) = best {
            return Some(e);
        }

        // 2. Flatpak sandbox id (~/.var/app/<id>).
        if let Some(id) = sandbox_segment(&query_str, "/.var/app/")
            && let Some(e) = self.find_by_flatpak_id(id)
        {
            return Some(e);
        }

        // 3. Snap sandbox name (~/snap/<name>).
        if let Some(name) = sandbox_segment(&query_str, "/snap/")
            && let Some(e) = self.find_by_snap_name(name)
        {
            return Some(e);
        }

        None
    }

    fn find_by_flatpak_id(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.values().find(|e| {
            e.creature
                .dwellings
                .values()
                .any(|d| d.flatpak_id.as_deref() == Some(id))
        })
    }

    fn find_by_snap_name(&self, name: &str) -> Option<&CatalogEntry> {
        self.entries.values().find(|e| {
            e.creature
                .dwellings
                .values()
                .any(|d| d.snap_name.as_deref() == Some(name))
        })
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
                    // matching kind. Use the first path on the target side.
                    let target_dwelling = entry.creature.dwellings.get(&target)?;
                    let target_path = target_kind_first(target_dwelling, kind)?;
                    return Some(expand_tilde(Path::new(target_path)));
                }
                if source_str.starts_with(&format!("{base_str}/")) {
                    // Sub-path of a dwelling root → translate the prefix.
                    let suffix = &source_str[base_str.len()..];
                    let target_dwelling = entry.creature.dwellings.get(&target)?;
                    let target_base = target_kind_first(target_dwelling, kind)?;
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

/// Match score for a query path against a declared (possibly-wildcarded)
/// path. Returns `Some(score)` if the query is covered (exact match,
/// descendant, or wildcard match) and `None` otherwise. Higher score is a
/// more specific (longer literal-prefix) match — used to break ties when
/// multiple entries cover the same query.
///
/// Wildcards: a single `*` in the declared path matches any run of
/// non-`/` characters. Useful for host-suffixed state files like
/// `~/.zcompdump*` which expand to `~/.zcompdump-<host>-<version>`.
fn match_score(declared: &str, query: &str) -> Option<usize> {
    if !declared.contains('*') {
        // Fast path: literal prefix match.
        if query == declared {
            return Some(declared.len());
        }
        if query.starts_with(declared) && query.as_bytes().get(declared.len()) == Some(&b'/') {
            return Some(declared.len());
        }
        return None;
    }
    // Wildcard path: `*` matches `[^/]*`. We allow the query to be the
    // matched literal or a descendant of it.
    let parts: Vec<&str> = declared.split('*').collect();
    let mut cur = 0usize;
    let mut literal_len = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            if !query[cur..].starts_with(part) {
                return None;
            }
            cur += part.len();
            literal_len += part.len();
            continue;
        }
        // Find `part` in the rest of the query without crossing `/`.
        let segment_end = query[cur..]
            .find('/')
            .map(|i| cur + i)
            .unwrap_or(query.len());
        let hay = &query[cur..segment_end];
        match hay.find(part) {
            Some(off) => {
                cur += off + part.len();
                literal_len += part.len();
            }
            None => return None,
        }
    }
    // After consuming all literal parts, the trailing `*` (if any) eats
    // the remainder of the current segment, but we still allow descendant
    // paths after that segment.
    Some(literal_len)
}

/// First declared path for a given kind on a dwelling, or `None` if that
/// kind is unset. Used when translating between flavors — we pick a
/// canonical destination rather than emitting every alternate path.
fn target_kind_first(
    dwelling: &crate::creature::Dwelling,
    kind: crate::creature::Kind,
) -> Option<&str> {
    let paths = match kind {
        crate::creature::Kind::Config => dwelling.config.as_ref(),
        crate::creature::Kind::Data => dwelling.data.as_ref(),
        crate::creature::Kind::Cache => dwelling.cache.as_ref(),
        crate::creature::Kind::State => dwelling.state.as_ref(),
    }?;
    paths.first().map(|s| s.as_str())
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

/// Pull the immediate child segment under `marker` out of `path`. e.g.
/// `sandbox_segment("/home/me/.var/app/com.foo.Bar/config/foo", "/.var/app/")`
/// returns `Some("com.foo.Bar")`. Trailing-slash queries (the bare sandbox
/// dir itself) also match.
fn sandbox_segment<'a>(path: &'a str, marker: &str) -> Option<&'a str> {
    let idx = path.find(marker)?;
    let after = &path[idx + marker.len()..];
    let segment = after.split('/').next()?;
    if segment.is_empty() {
        None
    } else {
        Some(segment)
    }
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
                config: Some(crate::creature::Paths(vec![native_config.to_string()])),
                ..Default::default()
            },
        );
        if let Some((id, path)) = flatpak_config {
            dwellings.insert(
                Flavor::Flatpak,
                Dwelling {
                    flatpak_id: Some(id.to_string()),
                    config: Some(crate::creature::Paths(vec![path.to_string()])),
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
    fn lookup_path_resolves_bare_flatpak_sandbox_dir() {
        // The catalog declares paths INSIDE the flatpak sandbox dir; a query
        // for the bare sandbox dir itself should still resolve via flatpak_id.
        let cat = Catalog::from_creatures(vec![fake(
            "foo",
            "/home/me/.config/foo",
            Some(("com.foo.Foo", "/home/me/.var/app/com.foo.Foo/config/foo")),
        )]);
        let hit = cat
            .lookup_path(Path::new("/home/me/.var/app/com.foo.Foo"))
            .expect("bare sandbox dir should resolve");
        assert_eq!(hit.creature.name, "foo");
    }

    #[test]
    fn lookup_path_resolves_subpath_under_unmapped_flatpak_dir() {
        // Catalog only declares a config subpath; a query for the cache subdir
        // (which isn't catalogued) should still resolve via flatpak_id.
        let cat = Catalog::from_creatures(vec![fake(
            "foo",
            "/home/me/.config/foo",
            Some(("com.foo.Foo", "/home/me/.var/app/com.foo.Foo/config/foo")),
        )]);
        let hit = cat
            .lookup_path(Path::new(
                "/home/me/.var/app/com.foo.Foo/cache/foo/sessions",
            ))
            .expect("uncatalogued subpath under known flatpak id should resolve");
        assert_eq!(hit.creature.name, "foo");
    }

    #[test]
    fn lookup_path_misses_unknown_flatpak_id() {
        let cat = Catalog::from_creatures(vec![fake(
            "foo",
            "/home/me/.config/foo",
            Some(("com.foo.Foo", "/home/me/.var/app/com.foo.Foo/config/foo")),
        )]);
        assert!(
            cat.lookup_path(Path::new("/home/me/.var/app/com.somebody.Else"))
                .is_none()
        );
    }

    #[test]
    fn lookup_path_matches_wildcard() {
        // ~/.zcompdump* should match the bare file and host-suffixed variants
        // but not unrelated siblings.
        let cat = Catalog::from_creatures(vec![fake("zsh", "/home/me/.zcompdump*", None)]);
        for p in [
            "/home/me/.zcompdump",
            "/home/me/.zcompdump-127-5.9",
            "/home/me/.zcompdump-host-5.9.zwc",
        ] {
            assert!(
                cat.lookup_path(Path::new(p)).is_some(),
                "expected match for {p}"
            );
        }
        // `*` must not cross `/`.
        assert!(
            cat.lookup_path(Path::new("/home/me/.zcompdump/sub/file"))
                .is_some(),
            "descendant of wildcard match should still resolve"
        );
        assert!(
            cat.lookup_path(Path::new("/home/me/.bashrc")).is_none(),
            "unrelated sibling must not match wildcard"
        );
    }

    #[test]
    fn lookup_path_matches_one_of_many_paths() {
        // A single dwelling can declare multiple paths per kind; any of
        // them should resolve.
        let mut dwellings = BTreeMap::new();
        dwellings.insert(
            Flavor::Native,
            Dwelling {
                config: Some(crate::creature::Paths(vec![
                    "/home/me/.zshrc".into(),
                    "/home/me/.zshenv".into(),
                ])),
                ..Default::default()
            },
        );
        let cat = Catalog::from_creatures(vec![Creature {
            name: "zsh".into(),
            display_name: None,
            category: None,
            homepage: None,
            dwellings,
            backup_exclude: vec![],
            tags: vec![],
        }]);
        assert_eq!(
            cat.lookup_path(Path::new("/home/me/.zshenv"))
                .unwrap()
                .creature
                .name,
            "zsh"
        );
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
