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
use std::time::Duration;
use structopt::StructOpt;
use tabwriter::TabWriter;
use humantime::format_duration;

/// 🤹 Get workflow information
#[derive(StructOpt, Debug)]
pub enum Workflows {
    /// List declared workflows
    List {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Workflow name
        #[structopt(short, long, env = "ACTIONS_WORKFLOW")]
        workflow: Option<String>,
    },
    /// List billable minutes declared workflows
    Usage {
       /// GitHub repository in the form owner/repo
       #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
       repository: String,
       /// Workflow name
       #[structopt(short, long, env = "ACTIONS_WORKFLOW")]
       workflow: Option<String>,
    }
    // todo: Show
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
        Workflows::Usage {
            repository,
            workflow,
        } => {
            let mut writer = TabWriter::new(stdout());

            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")
                .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
            let requests = Requests { client, token };

            writeln!(writer, "Workflow\tLinux\tMacOs\tWindows")?;
            let mut workflows =
                filtered_workflows(workflow, requests.clone().workflows(repository.clone()))
                    .boxed();
            let sum = std::rc::Rc::new(std::cell::RefCell::new(Duration::default()));
            while let Some(workflow) = Pin::new(&mut workflows).next().await {
                let usage = requests.workflow_usage(repository.clone(), workflow.id).await?;
                let ubuntu = usage.ubuntu();
                let macos = usage.macos();
                let windows = usage.windows();
                *sum.borrow_mut() += ubuntu + macos + windows;
                writeln!(
                    writer,
                    "{}\t{}\t{}\t{}",
                    workflow.name.bold(),
                    format_duration(ubuntu),
                    format_duration(macos),
                    format_duration(windows),
                )?;
            }
            writer.flush()?;
            println!(
                "\nTotal minutes spent {}",
                (sum.borrow().as_secs() / 60).to_string().bold()
            );
        }
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
