mod catchup;
mod crontab;
mod runner;
mod schedule;
mod state;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "cronx", version, about)]
struct Cli {
    /// Wrap a command, run it, and update job state.
    #[arg(long, value_name = "COMMAND", conflicts_with = "catch_up")]
    run: Option<String>,

    /// Slug identifying the job; required with --run.
    #[arg(long, value_name = "SLUG", conflicts_with = "catch_up")]
    slug: Option<String>,

    /// Scan crontab and re-run any managed jobs that missed a scheduled fire.
    #[arg(long)]
    catch_up: bool,

    /// With --catch-up: report what would run without executing or baselining.
    #[arg(long, requires = "catch_up")]
    dry_run: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

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
                "nothing to do — pass --run \"<command>\" --slug \"<slug>\", or --catch-up"
            )
        }
    }
}
