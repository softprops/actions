//! Interfaces for accessing and updating GitHub secrets
use crate::{github::Requests, StringErr};
use futures::stream::StreamExt;
use reqwest::Client;
use std::{env, error::Error, pin::Pin};
use structopt::StructOpt;

/// 🤫 Get information availble workflow secrets
#[derive(StructOpt, Debug)]
pub enum Secrets {
    /// List repository secrets
    List {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
    },
    /// Get a public key used for creating secrets
    PublicKey {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
    },
    Delete {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Name of secret to delete
        // #[structopt(short, long)]
        name: String,
    },
}

pub async fn secrets(args: Secrets) -> Result<(), Box<dyn Error>> {
    match args {
        Secrets::List { repository } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")
                .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
            let requests = Requests { client, token };
            let mut secrets = requests.clone().secrets(repository).boxed();
            while let Some(secret) = Pin::new(&mut secrets).next().await {
                println!("{}", secret.name);
            }
        }
        Secrets::PublicKey { repository } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")?;
            let requests = Requests { client, token };
            println!("{}", requests.public_key(repository).await?);
        }
        Secrets::Delete { repository, name } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")?;
            let requests = Requests { client, token };
            requests.delete_secret(repository, name.clone()).await?;
            println!("Secret {} is deleted", name);
        }
    }

    Ok(())
}
