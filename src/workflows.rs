use crate::{
    github::{Requests, Workflow},
    StringErr,
};
use colored::Colorize;
use futures::{stream::Stream, StreamExt};
use reqwest::Client;
use std::{
    env,
    error::Error,
    io::{stdout, Write},
    pin::Pin,
};
use structopt::StructOpt;
use tabwriter::TabWriter;

/// 🤹 Discover repository workflows
#[derive(StructOpt, Debug)]
pub enum Workflows {
    List {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Workflow name
        #[structopt(short, long, env = "ACTIONS_WORKFLOW")]
        workflow: Option<String>,
    },
}

fn filtered_workflows(
    workflow: Option<String>,
    workflows: impl Stream<Item = Workflow>,
) -> impl Stream<Item = Workflow> {
    workflows.filter(move |flow| {
        let matched = workflow.as_ref().map_or(true, |name| {
            flow.name.to_lowercase().contains(&name.to_lowercase())
        });
        async move { matched }
    })
}

pub async fn workflows(args: Workflows) -> Result<(), Box<dyn Error>> {
    match args {
        Workflows::List {
            repository,
            workflow,
        } => {
            let mut writer = TabWriter::new(stdout());

            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")
                .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
            let requests = Requests { client, token };

            writeln!(writer, "Workflow\tPath")?;
            let mut workflows =
                filtered_workflows(workflow, requests.clone().workflows(repository.clone()))
                    .boxed();
            while let Some(workflow) = Pin::new(&mut workflows).next().await {
                writeln!(
                    writer,
                    "{}\t{}",
                    workflow.name.bold(),
                    workflow.path.dimmed()
                )?;
            }
            writer.flush()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use futures_await_test::async_test;

    #[async_test]
    async fn filtered_workflows_filters_workflows_by_name() {
        assert_eq!(
            filtered_workflows(
                Some("CI".into()),
                stream::iter(vec![
                    Workflow {
                        id: 1,
                        name: "ci test".into(),
                        state: "completed".into(),
                        path: ".github/workflows".into()
                    },
                    Workflow {
                        id: 2,
                        name: "test".into(),
                        state: "completed".into(),
                        path: ".github/workflows".into()
                    }
                ])
            )
            .collect::<Vec<_>>()
            .await,
            vec![Workflow {
                id: 1,
                name: "ci test".into(),
                state: "completed".into(),
                path: ".github/workflows".into()
            }]
        );
    }
}
