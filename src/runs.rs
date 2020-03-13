use crate::{
    github::{Requests, Run, Workflow},
    StringErr,
};
use chrono::{offset::TimeZone, DateTime, Datelike, Utc};
use colored::Colorize;
use futures::{stream::Stream, StreamExt};
use reqwest::Client;
use spinner::SpinnerBuilder;
use humantime::format_duration;
use std::{
    cell::RefCell,
    cmp, env,
    error::Error,
    io::{stdout, Write},
    pin::Pin,
    rc::Rc,
    str::FromStr,
    time::Duration,
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
    /// Extract usage statistics for the current month
    Stats {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Workflow name
        #[structopt(short, long, env = "ACTIONS_WORKFLOW")]
        workflow: Option<String>,
        /// Since date in yyyy-mm-dd format
        #[structopt(short, long, env = "ACTIONS_SINCE")]
        since: Option<String>,
        /// Format of output 'tab' (default) or 'csv'
        #[structopt(default_value = "tab", short, long, env = "ACTIONS_FORMAT")]
        format: Format,
    },
    /// List runs for a given workflow
    List {
        /// GitHub repository in the form owner/repo
        #[structopt(short, long, env = "ACTIONS_REPOSITORY")]
        repository: String,
        /// Workflow name
        #[structopt(short, long, env = "ACTIONS_WORKFLOW")]
        workflow: String,
        /// Since date in yyyy-mm-dd format
        #[structopt(short, long, env = "ACTIONS_SINCE")]
        since: Option<String>,
        /// Format of output 'tab' (default) or 'csv'
        #[structopt(default_value = "tab", short, long, env = "ACTIONS_FORMAT")]
        format: Format,
    },
}

#[derive(Default, PartialEq, Debug)]
struct RunStats {
    count: usize,
    total: Duration,
    min: Option<Duration>,
    max: Option<Duration>,
}

async fn run_stats(
    since: DateTime<Utc>,
    runs: impl Stream<Item = Run>,
) -> RunStats {
    runs.fold(RunStats::default(), |stats, run| {
        async move {
            if run.created_at < since {
                return stats;
            }
            let RunStats {
                count,
                total,
                min,
                max,
            } = stats;
            let duration = run.duration();
            RunStats {
                count: count + 1,
                total: total + duration,
                min: Some(min.map_or(duration, |m| cmp::min(duration, m))),
                max: Some(max.map_or(duration, |m| cmp::max(duration, m))),
            }
        }
    })
    .await
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

fn spinner() -> SpinnerBuilder {
    SpinnerBuilder::new("Fetching run data...".into())
        .spinner(vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
        .step(std::time::Duration::from_millis(100))
}

pub async fn runs(args: Runs) -> Result<(), Box<dyn Error>> {
    match args {
        Runs::Stats {
            repository,
            workflow,
            since,
            ..
        } => {
            let since = date_or_first_of_the_month(since);
            let writer = Rc::new(RefCell::new(TabWriter::new(stdout())));

            let client = Client::new();
            let token = env::var("GITHUB_TOKEN")
                .map_err(|_| StringErr("Please provide a GITHUB_TOKEN env variable".into()))?;
            let requests = Requests { client, token };
            let spinner = spinner().start();
            writeln!(
                writer.clone().borrow_mut(),
                "Workflow\tRuns\tTotal Duration\tMin Duration\tMax Duration"
            )?;
            let mut workflows =
                filtered_workflows(workflow, requests.clone().workflows(repository.clone()))
                    .boxed();
            let sum = Rc::new(RefCell::new(Duration::default()));
            let clone_writer = writer.clone();
            let clone_sum = sum.clone();
            Pin::new(&mut workflows)
                .map(move |workflow| {
                    (
                        workflow,
                        requests.clone(),
                        repository.clone(),
                        writer.clone(),
                        sum.clone(),
                    )
                })
                .for_each_concurrent(Some(20), |(workflow, requests, repository, writer, sum)| {
                    async move {
                        let RunStats {
                            count,
                            total,
                            min,
                            max,
                        } = run_stats(since, requests.runs(repository, workflow.filename(), since))
                            .await;
                        *sum.borrow_mut() += total;
                        writeln!(
                            writer.clone().borrow_mut(),
                            "{}\t{}\t{}\t{}\t{}",
                            workflow.name.bold(),
                            count,
                            format_duration(total),
                            min.map(|min| format_duration(min).to_string())
                                .unwrap_or_else(|| "-".into()),
                            max.map(|max| format_duration(max).to_string())
                                .unwrap_or_else(|| "-".into())
                        )
                        .unwrap();
                    }
                })
                .await;
            spinner.close();
            if let Some(mut t) = term::stdout() {
                t.carriage_return()?;
                t.delete_line()?;
            }

            clone_writer.borrow_mut().flush()?;
            println!(
                "\nTotal minutes spent {}",
                (clone_sum.borrow().as_secs() / 60).to_string().bold()
            );
        }
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
                    .runs(repository.clone(), workflow.filename(), since)
                    .boxed();
                Pin::new(&mut runs)
                    .for_each_concurrent(Some(20), |run| {
                        async move {
                            println!(
                                "{} {} {} {}",
                                run.id,
                                match &run.conclusion.clone().unwrap_or_default()[..] {
                                    "failure" => "failure".red(),
                                    "success" => "success".green(),
                                    other => other.dimmed(),
                                },
                                format_duration(run.duration()),
                                run.html_url.dimmed()
                            )
                        }
                    })
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

    #[async_test]
    async fn run_stats_yields_default_for_empty_runs() {
        let since = date_or_first_of_the_month(Some("-"));
        assert_eq!(run_stats(since, stream::empty()).await, RunStats::default());
    }
}
