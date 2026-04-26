# bestiary

A catalog of Linux applications and the places on disk where they keep their config, data, cache, and state — across native, flatpak, snap, and legacy install flavors.

> Status: alpha. ~650 catalog entries, library API, and `ls` / `show` / `lookup` / `dump` / `scan` CLI all work.

## Why

Linux apps scatter their files across XDG dirs (`~/.config/X`, `~/.local/share/X`), per-flavor sandboxes (`~/.var/app/X/...` for flatpak, `~/snap/X/...` for snap), and historical legacy locations (`~/.X`). The same Discord install lives in three different places depending on how it was installed.

bestiary catalogs those locations: one entry per app, with a per-flavor breakdown. Any tool that needs to know "where does Discord keep its config?" or "what's the flatpak equivalent of this native config path?" can ask bestiary. The data is the primary artifact; the Rust binary is a thin viewer; downstream tools either depend on bestiary as a Rust library or fetch the JSON dump.

## Vocabulary

| Term | Meaning |
|---|---|
| **entry** | One application's record — its identity and where its data lives |
| **flavor** | Install variant: `native`, `flatpak`, `snap`, `legacy` |
| **location** | Per-flavor path bundle: which paths the app uses under one flavor |
| **kind** | Within a location: `config`, `data`, `cache`, `state` (XDG-aligned) |
| **catalog** | The collection of entries (embedded set + personal `~/.config/bestiary/apps/`) |

## Install

Same one-liner pattern as grimoire:

```sh
curl -fsSL https://raw.githubusercontent.com/strycore/bestiary/main/install.sh | bash
```

(Will work after the first GitHub release is cut.)

## Build from source

```sh
cargo build --release
./target/release/bestiary --help
```

## What works today

```sh
bestiary ls                                    # all apps in the catalog
bestiary ls --category development             # filter by category
bestiary show discord                          # render an entry
bestiary lookup ~/.config/heroic               # which app owns this path?
bestiary scan                                  # walk XDG dirs + dotfiles, list unmatched
bestiary scan -k                               # list matched paths with their owning app
bestiary dump > catalog.json                   # full catalog as JSON (for offline / API use)
```

The library face, for downstream tools:

```rust
use bestiary::Catalog;

let cat = bestiary::Catalog::load()?;
cat.lookup_path(Path::new("~/.config/discord/settings.json")); // → Discord entry
cat.map_flavor(Path::new("~/.config/discord"), bestiary::Flavor::Flatpak);
//   → ~/.var/app/com.discordapp.Discord/config/discord
```

## Schema

An entry is a YAML file under `apps/<name>.yaml`:

```yaml
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
    data:   ~/.var/app/com.discordapp.Discord/data
    cache:  ~/.var/app/com.discordapp.Discord/cache/discord

backup_exclude:
  - "Cache/*"
  - "GPUCache/*"

tags: [chat, voip]
```

Required: `name`, at least one flavor under `locations`. Each location needs at least one of `config` / `data` / `cache` / `state`. Flatpak locations must declare `flatpak_id`; snap locations must declare `snap_name`. Each path field accepts either a string or a list of strings. A single `*` in a path matches any run of non-`/` characters (useful for rotated logs and host-suffixed state files). The full schema lives in [`schema/app.schema.json`](./schema/app.schema.json).

Personal additions go in `~/.config/bestiary/apps/*.yaml` and override embedded entries by `name` match.

## Contributing

The repo's primary content is data: YAML entries under [`apps/`](./apps). Adding or improving an entry is the main contribution path — you don't need a Rust toolchain to do it.

**Adding an app:**

1. Find a path your machine has that nothing in the catalog claims. Easiest way is to grab the binary from a release, then run `bestiary scan` — anything in the unknown list is fair game.
2. Create `apps/<name>.yaml`. The filename stem must match the `name:` field (lowercase letters, digits, dashes — `[a-z][a-z0-9-]*`).
3. Use the schema above. One entry per app: fold sibling files (e.g. `~/.config/foorc` + `~/.local/share/foo` + `~/.cache/foo`) into a single entry with `config:` / `data:` / `cache:` / `state:`. If a path field needs multiple values, use a YAML list. For pre-XDG conventions (`~/.foo`), use the `legacy:` flavor.
4. Open a PR. CI runs the JSON-schema validation, name-pattern check, and "every embedded yaml parses" test. If it goes green and the entry is honest about where the app actually stores its data, that's all that's needed.

**Editing an existing entry:** same flow — change the file, push the PR. If an app moved (XDG migration, flatpak ID change), update both flavors rather than adding a parallel entry.

**What not to add:**

- Backup or rotation artifacts (`*.bak`, `*.backup*`, `*.old`, `*.prev`, editor swap files). Those belong to a cleanup tool, not a catalog of apps.
- Auto-generated state with random suffixes (e.g. `recently-used.xbel.A1B2C3`). Cover the canonical name; let the random one fall through.
- Per-machine variants. If a path embeds a hostname or version, use a `*` wildcard so the entry is portable.

## Goals out of scope (intentional)

These shape what bestiary will and won't grow into. They're decisions, not pending features.

- **Curated, not auto-discovered.** The catalog is hand-written. We don't infer entries by scanning binaries or watching syscalls — too many false positives, and the value is in human-vetted facts.
- **Apps only, not the OS.** Kernel state, systemd services, distro package metadata, and driver configs aren't in scope. If a user can't reasonably uninstall it, it's not an app.
- **Current state only.** No version history per app — if `~/.foo/v2/config` replaces `~/.foo/config`, we update the entry in place. Anyone needing point-in-time data can pin a git rev.
- **Transient artifacts are out.** Logs rotate, swaps churn, backups accumulate. The catalog records *where the app keeps its things*, not every file the app has ever generated. (A separate tool, [fili](https://github.com/strycore/fili), classifies and cleans those.)
- **No behavior, just locations.** bestiary doesn't back up, restore, sync, migrate flavors, or version-control configs. Tools like grimoire/chezmoi/rclone consume the catalog and do those things.
- **One entry per app.** When a single application leaves files in five different places, those are five paths in one entry — not five entries that share a tag.

## License

GPL-3.0-or-later for the code. Catalog data (`apps/*.yaml`) is intended to be permissively reusable; treat it as facts about where applications store their config, not code.
