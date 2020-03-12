mod artifacts;
mod repos;
mod runs;
mod secrets;
mod workflows;
use artifacts::{artifacts, Artifacts};
use repos::{repos, Repos};
use runs::{runs, Runs};
use secrets::{secrets, Secrets};
use std::error::Error;
use structopt::StructOpt;
use workflows::{workflows, Workflows};
mod github;
use colored::Colorize;
use std::{fmt, process::exit};

#[derive(Debug)]
struct StringErr(String);

impl Error for StringErr {}

impl fmt::Display for StringErr {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// 🎬 GitHub actions cli
///
/// A `GITHUB_TOKEN` env variable is required
/// to authenticate with the GitHub's actions API
#[derive(Debug, StructOpt)]
enum Options {
    Artifacts(Artifacts),
    Repos(Repos),
    Runs(Runs),
    Secrets(Secrets),
    Workflows(Workflows),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    if let Err(msg) = match Options::from_args() {
        Options::Artifacts(args) => artifacts(args).await,
        Options::Repos(args) => repos(args).await,
        Options::Runs(args) => runs(args).await,
        Options::Secrets(args) => secrets(args).await,
        Options::Workflows(args) => workflows(args).await,
    } {
        eprintln!("{}: {}", "error".bold().red(), msg);
        exit(1);
    }
    Ok(())
}
