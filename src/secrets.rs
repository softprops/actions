//! Interfaces for accessing and updating GitHub secrets
use crate::{github::Requests, StringErr};
use futures::stream::StreamExt;
use reqwest::Client;
use sodiumoxide::crypto::box_::{self, PublicKey};
use std::{env, error::Error, pin::Pin};
use structopt::StructOpt;

/// 🤫 Interact with workflow secrets
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
    /// Create a secret
    Create {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Secret name
        #[structopt(short, long)]
        name: String,
        /// Secret value
        #[structopt(short, long)]
        value: String,
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
            println!("{}", requests.public_key(repository).await?.key);
        }
        Secrets::Delete { repository, name } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")?;
            let requests = Requests { client, token };
            requests.delete_secret(repository, name.clone()).await?;
            println!("Secret {} is deleted", name);
        }
        Secrets::Create {
            repository,
            name,
            value,
        } => {
            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")?;
            let requests = Requests { client, token };
            let crate::github::Key { key_id, key } = requests.public_key(&repository).await?;
            let theirs = PublicKey::from_slice(&base64::decode(key)?).unwrap();
            let (_, ours) = box_::gen_keypair();
            let nonce = box_::gen_nonce();
            let encrypted = box_::seal(&value.as_bytes(), &nonce, &theirs, &ours);
            let encrypted_value = base64::encode(encrypted);
            requests
                .upsert_secret(repository, name, encrypted_value, key_id)
                .await?;
        }
    }

    Ok(())
}
