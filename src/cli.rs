use crate::catalog::{Catalog, Source};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "bestiary",
    version,
    about = "A catalog of Linux apps and where they keep their data."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// List apps in the catalog.
    Ls(LsArgs),
    /// Render an app's full entry.
    Show(ShowArgs),
    /// Look up which app owns a given path.
    Lookup(LookupArgs),
    /// Dump the catalog as JSON (defaults to stdout).
    Dump(DumpArgs),
    /// Walk standard XDG roots + home dotfiles and report which paths
    /// the catalog claims, plus what's still unmapped.
    Scan(ScanArgs),
}

#[derive(clap::Args)]
pub struct LsArgs {
    /// Filter by category.
    #[arg(long)]
    pub category: Option<String>,
    /// Show only personal entries (from ~/.config/bestiary/apps/).
    #[arg(long)]
    pub personal: bool,
    /// Show only embedded (shareable) entries.
    #[arg(long, conflicts_with = "personal")]
    pub embedded: bool,
}

#[derive(clap::Args)]
pub struct ShowArgs {
    pub app: String,
}

#[derive(clap::Args)]
pub struct LookupArgs {
    /// File or directory to identify.
    pub path: PathBuf,
}

#[derive(clap::Args)]
pub struct DumpArgs {
    /// Write to a file instead of stdout.
    #[arg(long)]
    pub out: Option<PathBuf>,
}

#[derive(clap::Args)]
pub struct ScanArgs {
    /// Home directory to scan. Defaults to $HOME. Pointing at a mounted
    /// backup (e.g. `/mnt/oldbox/home/alice`) lets you scan someone
    /// else's dotfiles without having to be them.
    pub home: Option<PathBuf>,
    /// Directories to scan instead of the default XDG roots. Their
    /// immediate children are looked up. Repeatable.
    #[arg(long = "root")]
    pub roots: Vec<PathBuf>,
    /// Print only paths the catalog doesn't claim.
    #[arg(short = 'u', long)]
    pub unknown_only: bool,
    /// Print only the matched paths (with their owning app).
    #[arg(short = 'k', long, conflicts_with = "unknown_only")]
    pub known_only: bool,
    /// Emit JSON instead of human-readable output.
    #[arg(long)]
    pub json: bool,
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Ls(a) => ls(a),
        Command::Show(a) => show(a),
        Command::Lookup(a) => lookup(a),
        Command::Dump(a) => dump(a),
        Command::Scan(a) => scan(a),
    }
}

fn ls(args: LsArgs) -> Result<()> {
    let cat = Catalog::load()?;
    let mut rows: Vec<(String, Source, String, Option<String>)> = cat
        .iter()
        .filter(|(_, e)| match (args.personal, args.embedded) {
            (true, _) => e.source == Source::Personal,
            (_, true) => e.source == Source::Embedded,
            _ => true,
        })
        .filter(|(_, e)| match &args.category {
            Some(c) => e.creature.category.as_deref() == Some(c.as_str()),
            None => true,
        })
        .map(|(name, e)| {
            (
                name.clone(),
                e.source,
                e.creature.pretty_name(),
                e.creature.category.clone(),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    if rows.is_empty() {
        println!("(no entries)");
        return Ok(());
    }

    let any_personal = rows.iter().any(|(_, src, _, _)| *src == Source::Personal);
    let name_w = rows.iter().map(|(n, _, _, _)| n.len()).max().unwrap_or(0);
    let pretty_w = rows.iter().map(|(_, _, p, _)| p.len()).max().unwrap_or(0);

    for (name, src, pretty, category) in rows {
        let prefix = match (any_personal, src) {
            (true, Source::Personal) => "~",
            (true, Source::Embedded) => " ",
            _ => "",
        };
        let category = category.unwrap_or_default();
        if any_personal {
            println!(
                "  {} {:<name_w$}  {:<pretty_w$}  {}",
                prefix, name, pretty, category
            );
        } else {
            println!("  {:<name_w$}  {:<pretty_w$}  {}", name, pretty, category);
        }
    }
    Ok(())
}

fn show(args: ShowArgs) -> Result<()> {
    let cat = Catalog::load()?;
    let entry = cat
        .get(&args.app)
        .with_context(|| format!("app {:?} not found", args.app))?;
    let yaml = serde_yml::to_string(&entry.creature).context("re-serializing entry")?;
    println!("# source: {:?}", entry.source);
    print!("{yaml}");
    Ok(())
}

fn lookup(args: LookupArgs) -> Result<()> {
    let cat = Catalog::load()?;
    let path = if args.path.is_absolute() {
        args.path
    } else {
        std::env::current_dir()
            .context("getting cwd")?
            .join(args.path)
    };
    match cat.lookup_path(&path) {
        Some(entry) => {
            println!(
                "{}  ({})",
                entry.creature.name,
                entry.creature.pretty_name()
            );
            if let Some(c) = &entry.creature.category {
                println!("category: {c}");
            }
            Ok(())
        }
        None => {
            anyhow::bail!("no app claims {}", path.display());
        }
    }
}

fn scan(args: ScanArgs) -> Result<()> {
    let cat = Catalog::load()?;

    // Resolve the home dir we're scanning. If the user passed one
    // explicitly we use it; otherwise fall back to $HOME.
    let scan_home = match args.home {
        Some(h) => Some(canonicalize_or_keep(&h)),
        None => std::env::var_os("HOME").map(|h| PathBuf::from(h.clone())),
    };
    // The "real" home that catalog paths expand `~` against.
    let real_home = std::env::var_os("HOME").map(PathBuf::from);

    let roots = if args.roots.is_empty() {
        default_scan_roots(scan_home.as_deref())
    } else {
        args.roots
    };

    let mut entries = collect_scan_entries(&roots, scan_home.as_deref());
    entries.sort();
    entries.dedup();

    let mut known: Vec<(PathBuf, String)> = Vec::new();
    let mut unknown: Vec<PathBuf> = Vec::new();
    for p in &entries {
        // The catalog declares paths like `~/.config/discord` which expand
        // to $HOME/... at lookup time. When scanning a foreign home, we
        // need to translate the scan path so its home-prefix matches the
        // running user's $HOME — otherwise nothing resolves.
        let lookup_target = match (&scan_home, &real_home) {
            (Some(sh), Some(rh)) if sh != rh => match p.strip_prefix(sh) {
                Ok(rel) => rh.join(rel),
                Err(_) => p.clone(),
            },
            _ => p.clone(),
        };
        match cat.lookup_path(&lookup_target) {
            Some(e) => known.push((p.clone(), e.creature.name.clone())),
            None => unknown.push(p.clone()),
        }
    }

    if args.json {
        let known_json: Vec<_> = known
            .iter()
            .map(|(p, n)| serde_json::json!({"path": p, "app": n}))
            .collect();
        let unknown_json: Vec<_> = unknown.iter().map(|p| serde_json::json!(p)).collect();
        let out = serde_json::json!({
            "total": entries.len(),
            "known": known.len(),
            "unknown": unknown.len(),
            "matches": known_json,
            "unmatched": unknown_json,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    // Default mode: list unknowns (most actionable). `--known-only`
    // flips that to list matches with their owning app. `--unknown-only`
    // is the same as the default but explicit (and suppresses the summary
    // when piped, since callers in scripts likely don't want it).
    if args.known_only {
        for (p, name) in &known {
            println!("{}\t{}", p.display(), name);
        }
    } else {
        for p in &unknown {
            println!("{}", p.display());
        }
    }

    let pct = if entries.is_empty() {
        0.0
    } else {
        100.0 * known.len() as f64 / entries.len() as f64
    };
    eprintln!(
        "scanned {} paths: {} known, {} unknown ({:.1}% covered)",
        entries.len(),
        known.len(),
        unknown.len(),
        pct
    );
    Ok(())
}

/// Default roots for `bestiary scan`: XDG dirs + flatpak sandbox root +
/// top-level home dotfiles. Each yields its immediate children as scan
/// targets (we don't recurse — `lookup_path` already covers descendants
/// of any matched dir).
fn default_scan_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else { return vec![] };
    vec![
        home.join(".config"),
        home.join(".local/share"),
        home.join(".local/state"),
        home.join(".var/app"),
        home.to_path_buf(),
    ]
}

/// Canonicalize a path if possible, otherwise just return it. We use
/// this for the user-supplied scan home so `..` and trailing slashes
/// don't trip up the home-prefix substitution.
fn canonicalize_or_keep(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Collect immediate children of each root. The home dir itself only
/// emits dot-prefixed entries (regular files & dirs at the home root
/// aren't bestiary's concern).
fn collect_scan_entries(roots: &[PathBuf], home: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let dotfiles_only = home.is_some_and(|h| h == root);
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if dotfiles_only {
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if !name.starts_with('.') || name == "." || name == ".." {
                    continue;
                }
            }
            out.push(p);
        }
    }
    out
}

fn dump(args: DumpArgs) -> Result<()> {
    let cat = Catalog::load()?;
    let mut out = serde_json::Map::new();
    for (name, entry) in cat.iter() {
        let value = serde_json::to_value(&entry.creature)?;
        out.insert(name.clone(), value);
    }
    let value = serde_json::Value::Object(out);
    let s = serde_json::to_string_pretty(&value)?;
    match args.out {
        Some(p) => std::fs::write(&p, s).with_context(|| format!("writing {}", p.display()))?,
        None => println!("{s}"),
    }
    Ok(())
}
