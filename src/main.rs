use bestiary::cli::{self, Cli};
use clap::Parser;

fn main() {
    let cli = Cli::parse();
    if let Err(err) = cli::run(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
