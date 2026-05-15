mod catchup;
mod crontab;
mod runner;
mod schedule;
mod setup;
mod state;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "cronx", version, about)]
struct Cli {
    /// Wrap a command, run it, and update job state.
    #[arg(long, value_name = "COMMAND", conflicts_with_all = ["catch_up", "setup", "takedown"])]
    run: Option<String>,

    /// Slug identifying the job; required with --run.
    #[arg(long, value_name = "SLUG", conflicts_with_all = ["catch_up", "setup", "takedown"])]
    slug: Option<String>,

    /// Scan crontab and re-run any managed jobs that missed a scheduled fire.
    #[arg(long, conflicts_with_all = ["setup", "takedown"])]
    catch_up: bool,

    /// With --catch-up: report what would run without executing or baselining.
    #[arg(long, requires = "catch_up")]
    dry_run: bool,

    /// Install the catch-up runner into the user's crontab.
    #[arg(long, conflicts_with = "takedown")]
    setup: bool,

    /// Remove the catch-up runner from the user's crontab (undoes --setup).
    #[arg(long)]
    takedown: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.setup {
        return setup::run_setup();
    }

    if cli.takedown {
        return setup::run_takedown();
    }

    if cli.catch_up {
        return catchup::run_catch_up(cli.dry_run);
    }

    match (cli.run, cli.slug) {
        (Some(command), Some(slug)) => {
            let code = runner::run_wrapped(&command, &slug)?;
            std::process::exit(code);
        }
        (Some(_), None) => anyhow::bail!("--run requires --slug"),
        (None, Some(_)) => anyhow::bail!("--slug requires --run"),
        (None, None) => {
            anyhow::bail!(
                "nothing to do — pass --run \"<command>\" --slug \"<slug>\", --catch-up, --setup, or --takedown"
            )
        }
    }
}
