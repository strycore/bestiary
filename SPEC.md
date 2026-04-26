# bestiary — design spec

> Status: design draft + foundation implemented. Catalog schema and library API are stable; CLI verbs `ls` / `show` / `lookup` / `dump` work. JSON API generator and downstream-tool integrations come next.

## What it is

bestiary is the canonical catalog of Linux applications and where they keep their data on disk. Each entry — a *creature* — describes one app's locations across install flavors: native (XDG dirs), flatpak (`~/.var/app/...`), snap (`~/snap/...`), and legacy (`~/.appname` style).

The data is the primary artifact. A Rust binary ships embedded with the curated catalog; downstream tools (fili, grimoire, others) consume bestiary as a Rust library or read the JSON dump.

## Tooling layout

| Project | Role |
|---|---|
| **moi** | Claude Agent frontend |
| **fili** | observe + classify what's on disk |
| **grimoire** | declarative install + state-reaching |
| **bestiary** *(this)* | canonical catalog of apps × data locations |

In time, **fili** will use bestiary's lookup API to classify `~/.config/X` entries (replacing duplicated path rules in fili's `rules.json`) and to drive `fili backup`. **grimoire** uses it for `grimoire restore` and `grimoire migrate native↔flatpak`.

bestiary depends on nothing of the above. It's the leaf of the dependency graph.

## Vocabulary

| Term | Meaning |
|---|---|
| **creature** | One app's entry in the catalog |
| **dwelling** | A creature's locations under one install flavor |
| **flavor** | `native` \| `flatpak` \| `snap` \| `legacy` |
| **kind** | `config` \| `data` \| `cache` \| `state` (XDG-aligned) |
| **catalog** | The full set of creatures (embedded + personal overrides) |

## Creature schema

```yaml
# REQUIRED
name: discord                                   # ^[a-z][a-z0-9-]*$, unique

# OPTIONAL but recommended
display_name: Discord
category: communication                          # free-form
homepage: https://discord.com
grimoire_spell: discord                          # cross-reference
tags: [chat, voip]

dwellings:                                       # at least one flavor required
  native:
    config: ~/.config/discord                    # at least one of config/data/cache/state
    data:   ~/.config/discord
    cache:  ~/.config/discord/Cache
    state:  ~/.config/discord/logs
  flatpak:
    flatpak_id: com.discordapp.Discord           # required on flatpak dwellings
    config: ~/.var/app/com.discordapp.Discord/config/discord
    cache:  ~/.var/app/com.discordapp.Discord/cache/discord
  snap:
    snap_name: discord                           # required on snap dwellings
    data: ~/snap/discord/current
  legacy:
    data: ~/.discord                             # pre-XDG location, kept for migration

backup_exclude:                                  # globs relative to dwelling paths
  - "Cache/*"
  - "GPUCache/*"
```

Validation rules:

- `name` matches `^[a-z][a-z0-9-]*$`.
- `dwellings` has at least one entry.
- Each declared dwelling has at least one of `config` / `data` / `cache` / `state`.
- `flatpak` dwellings require `flatpak_id`.
- `snap` dwellings require `snap_name`.
- Unknown top-level fields are rejected (`#[serde(deny_unknown_fields)]` + JSON Schema `additionalProperties: false`).

The canonical machine-readable definition lives at [`schema/creature.schema.json`](./schema/creature.schema.json). CI and an integration test validate every shipped creature against it.

## Library API surface

```rust
pub fn load() -> Result<Catalog>;                                // embedded + personal
pub fn embedded_only() -> Result<Catalog>;                       // tests / offline

impl Catalog {
    pub fn get(&self, name: &str) -> Option<&CatalogEntry>;
    pub fn iter(&self) -> impl Iterator<Item = (&String, &CatalogEntry)>;
    pub fn lookup_path(&self, p: &Path) -> Option<&CatalogEntry>;     // ← path → creature
    pub fn map_flavor(&self, p: &Path, target: Flavor) -> Option<PathBuf>;  // ← native ↔ flatpak ↔ snap path translation
}
```

`lookup_path` finds the longest-matching dwelling-root prefix (so `~/.config/discord/themes/dark.json` resolves to Discord). `map_flavor` translates a path under one flavor's dwelling to the equivalent path under another (used for native→flatpak migration).

## CLI

```
bestiary ls                                   List creatures.
  --category <name>                            Filter by category.
  --personal                                   Only personal entries.
  --embedded                                   Only embedded entries.

bestiary show <creature>                       Render the creature's full schema.

bestiary lookup <path>                         Identify which creature owns a path.

bestiary dump [--out <file>]                   Emit the catalog as JSON (stdout or file).
```

## Layout

### Repo

```
github.com/strycore/bestiary/
├── creatures/                  # PR target — one YAML per app
│   ├── discord.yaml
│   ├── thunderbird.yaml
│   └── ...
├── schema/
│   └── creature.schema.json
├── src/                        # Rust source
├── tests/
│   └── schema.rs
├── install.sh                  # one-liner installer (post-first-release)
├── README.md
├── SPEC.md
└── Cargo.toml
```

`creatures/*.yaml` are embedded into the binary at build time via `include_dir!`.

### Runtime

```
~/.config/bestiary/
└── creatures/
    └── my-private-app.yaml      # personal creatures override embedded by name match
```

## Status

| Area | State |
|---|---|
| Creature schema (YAML + JSON Schema) | ✅ |
| Library API (`get`, `iter`, `lookup_path`, `map_flavor`) | ✅ |
| Embedded + personal merge with override | ✅ |
| `bestiary ls` / `show` / `lookup` / `dump` | ✅ |
| Pre-commit hook (fmt + clippy + tests) | ✅ |
| 10 reference creatures | ✅ |
| Static JSON API + dump publishing | not started |
| `bestiary scan` (which creatures inhabit this machine) | not started |
| fili integration (replace fili's `<home>/.config/X` rules) | deferred |
| grimoire `restore` / `migrate` integration | deferred |

## Open follow-ups

- Static JSON API hosted at `https://strycore.github.io/bestiary/api/<name>.json` (or Cloudflare Pages) — generated by CI from `creatures/*.yaml` on push to main.
- Versioned dump tarball published as a GitHub release artifact.
- Backup manifest schema (a separate JSON Schema in this repo) so fili's `backup` and grimoire's `restore` agree on the archive format.
- Schema-validation hook: `bash -n` every implicit shell expansion in path strings (none today, but if creatures grow shell-style helpers, lint them).
- Snap layout audit — snap paths vary more than flatpak's; currently the catalog has minimal snap coverage.
