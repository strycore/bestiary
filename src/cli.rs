use crate::catalog::{Catalog, Source};
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

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

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Ls(a) => ls(a),
        Command::Show(a) => show(a),
        Command::Lookup(a) => lookup(a),
        Command::Dump(a) => dump(a),
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
