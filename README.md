# bestiary

A catalog of Linux applications and the places on disk where they keep their config, data, cache, and state — across native, flatpak, snap, and legacy install flavors.

> Status: alpha. Catalog schema, ten reference apps, library API, and `ls` / `show` / `lookup` / `dump` CLI all work.

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

Required: `name`, at least one flavor under `locations`. Each location needs at least one of `config` / `data` / `cache` / `state`. Flatpak locations must declare `flatpak_id`; snap locations must declare `snap_name`. The full schema lives in [`schema/app.schema.json`](./schema/app.schema.json).

Personal additions go in `~/.config/bestiary/apps/*.yaml` and override embedded entries by `name` match.

## Contributing

After cloning, point git at the in-repo hook once:

```sh
git config core.hooksPath .githooks
```

`pre-commit` runs `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, and the schema validation tests.

The bar for a new entry: **don't ship one you can't validate works**. Test on at least one machine — verify the paths actually exist for that flavor of the app on a current distro. Drop test commands and screenshots into the PR description.

## Goals out of scope (intentional)

- **Versioning** of app config (chezmoi / yadm territory; lean on git for that)
- **Live sync between machines** (rclone, syncthing, Nextcloud)
- **Backup logic itself** — bestiary catalogs *where* the data is. *What to do with it* (archive, restore, migrate flavors) belongs to whatever tool consumes the catalog.

## License

GPL-3.0-or-later for the code. Catalog data (`apps/*.yaml`) is intended to be permissively reusable; treat it as facts about where applications store their config, not code.
