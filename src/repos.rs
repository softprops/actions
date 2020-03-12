use crate::{github::Requests, StringErr};
use reqwest::Client;
use std::{
    env,
    error::Error,
    io::{stdout, Write},
};
use structopt::StructOpt;
use tabwriter::TabWriter;

/// 🌌 repos using GitHub Actions
#[derive(StructOpt, Debug)]
pub struct Repos {
    /// GitHub repository in the form `owner/repo`
    #[structopt(short, long, env = "ACTIONS_ORG")]
    org: String,
}

pub async fn repos(args: Repos) -> Result<(), Box<dyn Error>> {
    let Repos { org } = args;
    let client = Client::new();
    let token = env::var("GITHUB_TOKEN")
        .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
    let requests = Requests { client, token };
    let repos = requests.clone().repos(org).await;
    let mut writer = TabWriter::new(stdout());
    writeln!(writer, "Repo\tWorkflow Count")?;
    for repo in repos {
        writeln!(writer, "{}\t{}", repo.full_name, repo.workflows.len())?;
    }
    writer.flush()?;

    Ok(())
}
