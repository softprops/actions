use crate::{github::Requests, StringErr};
use futures::stream::StreamExt;
use reqwest::Client;
use std::{env, error::Error, pin::Pin};
use structopt::StructOpt;

/// 📦 Get workflow artifacts
#[derive(StructOpt, Debug)]
pub enum Artifacts {
    /// List repository secrets
    List {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Id of run
        #[structopt(long)]
        run_id: usize,
    },
    /// Delete a workflow run artifact
    Delete {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Id of artifact to delete
        #[structopt(short, long)]
        artifact_id: usize,
    },
}

pub async fn artifacts(args: Artifacts) -> Result<(), Box<dyn Error>> {
    match args {
        Artifacts::List { repository, run_id } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")
                .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
            let requests = Requests { client, token };
            let mut artifacts = requests.clone().artifacts(repository, run_id).boxed();
            while let Some(artifact) = Pin::new(&mut artifacts).next().await {
                println!("{}", artifact.name);
            }
        }
        Artifacts::Delete {
            repository,
            artifact_id,
        } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")?;
            let requests = Requests { client, token };
            requests.delete_artifact(repository, artifact_id).await?;
            println!("Artifact {} is deleted", artifact_id);
        }
    }

    Ok(())
}
