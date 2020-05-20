use crate::{
    github::{Requests, Workflow},
    StringErr,
};
use chrono::{offset::TimeZone, DateTime, Datelike, Utc};
use colored::Colorize;
use futures::{stream::Stream, StreamExt};
use humantime::format_duration;
use reqwest::Client;
use std::{
    env,
    str::FromStr,
    error::Error,
    io::{stdout, Write},
    pin::Pin,

};
use structopt::StructOpt;
use tabwriter::TabWriter;

#[derive(Debug)]
pub enum Format {
    Tab,
    Csv,
}

impl Default for Format {
    fn default() -> Self {
        Format::Tab
    }
}

impl FromStr for Format {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "csv" => Ok(Format::Csv),
            "tab" => Ok(Format::Tab),
            other => Err(format!(
                "{} is not a supported format. try 'csv' or 'tab' instead",
                other
            )),
        }
    }
}

/// 🏃 Get workflow run information
#[derive(StructOpt, Debug)]
pub enum Runs {
    /// List runs for a given workflow
    List {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Workflow name
        #[structopt(short, long, env = "ACTIONS_WORKFLOW")]
        workflow: String,
        /// List all runs since date in yyyy-mm-dd format
        #[structopt(short, long, env = "ACTIONS_SINCE")]
        since: Option<String>,
        /// Format of output 'tab' (default) or 'csv'
        #[structopt(default_value = "tab", short, long, env = "ACTIONS_FORMAT")]
        format: Format,
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

fn date_or_first_of_the_month(timestamp: Option<impl AsRef<str>>) -> DateTime<Utc> {
    timestamp
        .and_then(|ts| {
            chrono::NaiveDate::parse_from_str(ts.as_ref(), "%Y-%m-%d")
                .ok()
                .map(|fixed| {
                    let then = fixed.and_hms(0, 0, 0);
                    Utc.ymd(then.year(), then.month(), then.day())
                        .and_hms(0, 0, 0)
                })
        })
        .unwrap_or_else(|| {
            let now = Utc::now().naive_utc();
            Utc.ymd(now.year(), now.month(), 1).and_hms(0, 0, 0)
        })
}


pub async fn runs(args: Runs) -> Result<(), Box<dyn Error>> {
    match args {
        Runs::List {
            repository,
            workflow,
            since,
            ..
        } => {
            let since = date_or_first_of_the_month(since);
            let mut writer = TabWriter::new(stdout());

            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")
                .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
            let requests = Requests { client, token };
            let mut workflows = filtered_workflows(
                Some(workflow),
                requests.clone().workflows(repository.clone()),
            )
            .boxed();
            while let Some(workflow) = Pin::new(&mut workflows).next().await {
                let mut runs = requests
                    .clone()
                    .runs(repository.clone(), workflow.id.to_string(), since)
                    .boxed();
                Pin::new(&mut runs)
                    .for_each_concurrent(Some(20),  |run| {
                    let workflow = workflow.clone();
                    async move {
                        println!(
                            "{} {} {} {} {}",
                            workflow.name,
                            run.id,
                            match &run.conclusion.clone().unwrap_or_default()[..] {
                                "failure" => "failure".red(),
                                "success" => "success".green(),
                                other => other.dimmed(),
                            },
                            format_duration(run.duration()),
                            run.html_url.dimmed()
                        )
                    }})
                    .await;
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

    #[test]
    fn date_or_first_of_the_month_parses_dates() {
        let since = date_or_first_of_the_month(Some("2020-03-12"));
        assert_eq!(since, Utc.ymd(2020, 3, 12).and_hms(0, 0, 0))
    }
}
