use chrono::{DateTime, Utc};
use futures::{
    stream,
    stream::{Stream, StreamExt},
};
use hyperx::header::{Header, Link, RelationType};
use reqwest::{header::LINK, RequestBuilder, Response};
use serde::{de::DeserializeOwned, Deserialize};
use std::{collections::BTreeMap, error::Error, time::Duration};
use url::form_urlencoded::byte_serialize as urlencode;

#[derive(Debug, Deserialize, Clone)]
struct CodeSearch {
    incomplete_results: bool,
    items: Vec<CodeSearchItem>,
}

#[derive(Debug, Deserialize, Clone)]
struct CodeSearchItem {
    pub name: String,
    pub path: String,
    pub repository: Repository,
}

#[derive(Debug, Deserialize, Clone)]
struct Repository {
    pub full_name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Repo {
    pub full_name: String,
    pub workflows: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Artifacts {
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Artifact {
    pub id: usize,
    pub name: String,
    pub size_in_bytes: usize,
    pub archive_download_url: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Key {
    pub key: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Secrets {
    pub secrets: Vec<Secret>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Secret {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Workflows {
    pub workflows: Vec<Workflow>,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct Workflow {
    pub id: usize,
    pub name: String,
    pub state: String,
    pub path: String,
}

impl Workflow {
    pub fn filename(&self) -> String {
        self.path.replace(".github/workflows/", "")
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Runs {
    pub workflow_runs: Vec<Run>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Run {
    pub id: usize,
    pub head_branch: String,
    pub conclusion: Option<String>,
    pub event: String,
    pub status: String,
    pub jobs_url: String,
    pub logs_url: String,
    pub artifacts_url: String,
    pub cancel_url: String,
    pub rerun_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub html_url: String,
}

impl Run {
    pub fn duration(&self) -> Duration {
        (self.updated_at - self.created_at).to_std().unwrap()
    }
}

/// A GitHub actions client for executing requests
#[derive(Clone)]
pub struct Requests {
    pub client: reqwest::Client,
    pub token: String,
}

enum PageState {
    Fetch(Box<RequestBuilder>),
    End,
}

impl Requests {
    fn builder(
        &self,
        builder: RequestBuilder,
    ) -> RequestBuilder {
        builder.header("User-Agent", env!("CARGO_PKG_NAME")).header(
            "Authorization",
            format!("bearer {token}", token = self.token),
        )
    }

    fn get(
        &self,
        url: &str,
    ) -> RequestBuilder {
        self.builder(self.client.get(url))
    }

    fn delete(
        &self,
        url: &str,
    ) -> RequestBuilder {
        self.builder(self.client.delete(url))
    }

    /// Drives a paginated pull-oriented stream of api results to completion
    fn paginate<F, C, P: DeserializeOwned, I: DeserializeOwned>(
        self,
        state: PageState,
        mut into: F,
        mut cont: C,
    ) -> impl Stream<Item = I>
    where
        F: FnMut(P) -> Vec<I> + Copy,
        C: FnMut(&Vec<I>) -> bool + Copy,
    {
        stream::unfold(state, move |state| {
            let this = self.clone();
            async move {
                match state {
                    PageState::Fetch(builder) => {
                        let response = builder.send().await.ok()?;
                        let next = next_link(&response);
                        let items = into(response.json::<P>().await.ok()?);
                        let next_state = match next {
                            Some(link) if cont(&items) => {
                                PageState::Fetch(Box::new(this.get(&link)))
                            }
                            _ => PageState::End,
                        };
                        Some((stream::iter(items), next_state))
                    }
                    PageState::End => return None,
                }
            }
        })
        .flatten()
    }

    pub async fn repos(
        self,
        org: String,
    ) -> Vec<Repo> {
        let builder = self.get("https://api.github.com/search/code").query(&[
            ("per_page", "100"),
            (
                "q",
                format!("org:{org} path:.github/workflows", org = org).as_str(),
            ),
        ]);
        self.paginate(
            PageState::Fetch(Box::new(builder)),
            |s: CodeSearch| s.items,
            |_| true,
        )
        .fold(
            BTreeMap::default(),
            move |mut state: BTreeMap<String, Vec<String>>, item| async {
                state
                    .entry(item.repository.full_name)
                    .or_insert_with(Vec::new)
                    .push(item.path);
                state
            },
        )
        .await
        .into_iter()
        .map(|(full_name, workflows)| Repo {
            full_name,
            workflows,
        })
        .collect()
    }

    /// Gets your public key, which you must store. You need your public key to use other secrets endpoints.
    /// Use the returned key to encrypt your secrets. Anyone with read access to the repository can use this endpoint.
    /// GitHub Apps must have the secrets permission to use this endpoint.
    ///
    /// See the [developer docs](https://developer.github.com/v3/actions/secrets/#get-your-public-key) for more information
    pub async fn public_key(
        self,
        repository: String,
    ) -> Result<String, Box<dyn Error>> {
        Ok(self
            .get(&format!(
                "https://api.github.com/repos/{repo}/actions/secrets/public-key",
                repo = repository
            ))
            .send()
            .await?
            .json::<Key>()
            .await?
            .key)
    }

    pub async fn delete_secret(
        self,
        repository: String,
        name: String,
    ) -> Result<(), Box<dyn Error>> {
        self.delete(&format!(
            "https://api.github.com/repos/{repo}/actions/secrets/{name}",
            repo = repository,
            name = name
        ))
        .send()
        .await?;
        Ok(())
    }

    /// Lists all secrets available in a repository without revealing their encrypted values.
    /// Anyone with write access to the repository can use this endpoint.
    /// GitHub Apps must have the secrets permission to use this endpoint.
    ///
    /// See the [developer docs](https://developer.github.com/v3/actions/secrets/#list-secrets-for-a-repository) for more information
    pub fn secrets(
        self,
        repository: String,
    ) -> impl Stream<Item = Secret> {
        let builder = self
            .get(&format!(
                "https://api.github.com/repos/{repo}/actions/secrets",
                repo = repository
            ))
            .query(&[("per_page", "100")]);
        self.paginate(
            PageState::Fetch(Box::new(builder)),
            |w: Secrets| w.secrets,
            |_| true,
        )
    }

    /// Lists artifacts for a workflow run. Anyone with read access to the repository can use this endpoint. GitHub Apps must have the actions permission to use this endpoint.
    ///
    /// See the [developer docs](https://developer.github.com/v3/actions/artifacts/#list-workflow-run-artifacts) for more information
    pub fn artifacts(
        self,
        repository: String,
        run_id: usize,
    ) -> impl Stream<Item = Artifact> {
        let builder = self
            .get(&format!(
                "https://api.github.com/repos/{repo}/actions/runs/{run_id}/artifacts",
                repo = repository,
                run_id = run_id
            ))
            .query(&[("per_page", "100")]);
        self.paginate(
            PageState::Fetch(Box::new(builder)),
            |w: Artifacts| w.artifacts,
            |_| true,
        )
    }

    /// Deletes an artifact for a workflow run. Anyone with write access to the repository can use this endpoint. GitHub Apps must have the actions permission to use this endpoint.
    ///
    /// See the [developer docs](https://developer.github.com/v3/actions/artifacts/#delete-an-artifact) for more information
    pub async fn delete_artifact(
        self,
        repository: String,
        artifact_id: usize,
    ) -> Result<(), Box<dyn Error>> {
        self.delete(&format!(
            "https://api.github.com/repos/{repo}/actions/artifacts/{artifact_id}",
            repo = repository,
            artifact_id = artifact_id
        ))
        .send()
        .await?;
        Ok(())
    }

    /// Lists the workflows in a repository. Anyone with read access to the repository can use this endpoint.
    /// GitHub Apps must have the actions permission to use this endpoint.
    ///
    /// See the [developer docs](https://developer.github.com/v3/actions/workflows/#list-repository-workflows) for more information
    pub fn workflows(
        self,
        repository: String,
    ) -> impl Stream<Item = Workflow> {
        let builder = self
            .get(&format!(
                "https://api.github.com/repos/{repo}/actions/workflows",
                repo = repository
            ))
            .query(&[("per_page", "100")]);
        self.paginate(
            PageState::Fetch(Box::new(builder)),
            |w: Workflows| w.workflows,
            |_| true,
        )
    }

    /// List all workflow runs for a workflow.
    ///
    /// https://developer.github.com/v3/actions/workflow_runs/#list-workflow-runs
    pub fn runs(
        self,
        repository: String,
        workflow: String,
        since: DateTime<Utc>,
    ) -> impl Stream<Item = Run> {
        let builder = self
            .get(&format!(
                "https://api.github.com/repos/{repo}/actions/workflows/{workflow}/runs",
                repo = repository,
                workflow = urlencode(workflow.as_bytes()).collect::<String>()
            ))
            .query(&[("per_page", "100"), ("status", "completed")]);
        self.paginate(
            PageState::Fetch(Box::new(builder)),
            |w: Runs| w.workflow_runs,
            move |runs: &Vec<Run>| runs.iter().any(|run| run.created_at >= since),
        )
    }
}

fn next_link(response: &Response) -> Option<String> {
    Link::parse_header(&response.headers().get(LINK)?)
        .ok()?
        .values()
        .iter()
        .find_map(|value| {
            value.rel().and_then(|rels| {
                if rels.iter().any(|rel| rel == &RelationType::Next) {
                    Some(value.link().into())
                } else {
                    None
                }
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_next_link_returns_none_when_link_is_absent() {
        assert_eq!(
            next_link(&Response::from(http::Response::new(vec![]))),
            None
        )
    }

    #[test]
    fn parse_next_link_returns_none_when_link_is_present() {
        assert_eq!(
            next_link(&Response::from(
                http::Response::builder()
                    .header(
                        "Link",
                        r#"<https://api.github.com/test&page=2>; rel="next""#
                    )
                    .body(vec![])
                    .unwrap()
            )),
            Some("https://api.github.com/test&page=2".into())
        )
    }
}
