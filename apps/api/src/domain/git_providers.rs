//! Git provider adapters (ADR 0001).
//!
//! Everything in here is pure: providers differ only in how a webhook is
//! verified, how its payload maps to neutral events, and how an outbound
//! API request is shaped. The shared core (linking, transitions, the job
//! runner) never sees a provider payload.
//!
//! Slice 1 ships the outbound seam (commit status); the inbound seams for
//! GitLab/Gitea land with the multi-provider slice.

use crate::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Github,
    Gitlab,
    Gitea,
}

impl Provider {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "github" => Some(Self::Github),
            "gitlab" => Some(Self::Gitlab),
            "gitea" => Some(Self::Gitea),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
            Self::Gitea => "gitea",
        }
    }
    /// API root for the cloud instance; overridden by `base_url` for
    /// self-hosted installs.
    fn default_api_base(self) -> &'static str {
        match self {
            Self::Github => "https://api.github.com",
            Self::Gitlab => "https://gitlab.com",
            Self::Gitea => "", // Gitea is self-hosted; base_url is required.
        }
    }
}

/// Commit-status states in the neutral model. Providers map them to their
/// own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusState {
    Pending,
    Success,
}

/// Map a Sprintly task status to the commit status we publish. The contract
/// is "is the task this commit/PR belongs to finished?" — anything short of
/// done is pending.
pub fn task_status_to_state(task_status: &str) -> StatusState {
    if task_status == "done" {
        StatusState::Success
    } else {
        StatusState::Pending
    }
}

/// An outbound HTTP request as plain data; the job runner executes it.
#[derive(Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    /// (name, value) pairs; values may contain secrets — never log them.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Build the provider API call that sets a commit status. `repo` is
/// "owner/name" (GitLab also accepts its numeric project id). `target_url`
/// should deep-link to the Sprintly task.
#[allow(clippy::too_many_arguments)] // a pure builder; args mirror the API call
pub fn status_request(
    provider: Provider,
    base_url: Option<&str>,
    repo: &str,
    token: &str,
    sha: &str,
    state: StatusState,
    context: &str,
    description: &str,
    target_url: &str,
) -> AppResult<HttpRequest> {
    let base = base_url
        .map(|b| b.trim_end_matches('/').to_string())
        .unwrap_or_else(|| provider.default_api_base().to_string());
    if base.is_empty() {
        return Err(AppError::BadRequest(
            "this provider needs a base_url (self-hosted)".into(),
        ));
    }

    Ok(match provider {
        // GitHub and Gitea share the status API shape; Gitea serves it
        // under /api/v1.
        Provider::Github | Provider::Gitea => {
            let prefix = match provider {
                Provider::Github => String::new(),
                _ => "/api/v1".to_string(),
            };
            let gh_state = match state {
                StatusState::Pending => "pending",
                StatusState::Success => "success",
            };
            HttpRequest {
                method: "POST",
                url: format!("{base}{prefix}/repos/{repo}/statuses/{sha}"),
                headers: vec![
                    ("Authorization".into(), format!("token {token}")),
                    ("Content-Type".into(), "application/json".into()),
                    ("Accept".into(), "application/json".into()),
                    // GitHub requires a User-Agent on API calls.
                    ("User-Agent".into(), "sprintly".into()),
                ],
                body: serde_json::json!({
                    "state": gh_state,
                    "context": context,
                    "description": description,
                    "target_url": target_url,
                })
                .to_string(),
            }
        }
        Provider::Gitlab => {
            let gl_state = match state {
                StatusState::Pending => "running",
                StatusState::Success => "success",
            };
            // GitLab addresses projects by URL-encoded path or numeric id.
            let project = urlencode(repo);
            HttpRequest {
                method: "POST",
                url: format!("{base}/api/v4/projects/{project}/statuses/{sha}"),
                headers: vec![
                    ("PRIVATE-TOKEN".into(), token.to_string()),
                    ("Content-Type".into(), "application/json".into()),
                ],
                body: serde_json::json!({
                    "state": gl_state,
                    "name": context,
                    "description": description,
                    "target_url": target_url,
                })
                .to_string(),
            }
        }
    })
}

/// Percent-encode everything outside RFC 3986 unreserved characters —
/// enough for a GitLab project path ("group/repo" → "group%2Frepo").
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_round_trip() {
        for p in [Provider::Github, Provider::Gitlab, Provider::Gitea] {
            assert_eq!(Provider::parse(p.as_str()), Some(p));
        }
        assert_eq!(Provider::parse("bitbucket"), None);
    }

    #[test]
    fn task_status_mapping() {
        assert_eq!(task_status_to_state("done"), StatusState::Success);
        for s in ["todo", "in_progress", "review"] {
            assert_eq!(task_status_to_state(s), StatusState::Pending);
        }
    }

    #[test]
    fn github_status_request_shape() {
        let r = status_request(
            Provider::Github,
            None,
            "acme/app",
            "tok123",
            "abc123",
            StatusState::Success,
            "sprintly/DEMO-1",
            "DEMO-1 is done",
            "https://pm.example/tasks/DEMO-1",
        )
        .unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(
            r.url,
            "https://api.github.com/repos/acme/app/statuses/abc123"
        );
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "token tok123"));
        assert!(r.body.contains(r#""state":"success""#));
        assert!(r.body.contains(r#""context":"sprintly/DEMO-1""#));
    }

    #[test]
    fn gitlab_encodes_project_path_and_maps_pending() {
        let r = status_request(
            Provider::Gitlab,
            None,
            "group/app",
            "glpat",
            "abc123",
            StatusState::Pending,
            "sprintly/DEMO-1",
            "DEMO-1 is in review",
            "https://pm.example/tasks/DEMO-1",
        )
        .unwrap();
        assert_eq!(
            r.url,
            "https://gitlab.com/api/v4/projects/group%2Fapp/statuses/abc123"
        );
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "PRIVATE-TOKEN" && v == "glpat"));
        assert!(r.body.contains(r#""state":"running""#));
    }

    #[test]
    fn gitea_needs_base_url_and_gets_v1_prefix() {
        assert!(status_request(
            Provider::Gitea,
            None,
            "acme/app",
            "t",
            "s",
            StatusState::Pending,
            "c",
            "d",
            "u",
        )
        .is_err());
        let r = status_request(
            Provider::Gitea,
            Some("https://git.acme.dev/"),
            "acme/app",
            "t",
            "abc",
            StatusState::Success,
            "c",
            "d",
            "u",
        )
        .unwrap();
        assert_eq!(
            r.url,
            "https://git.acme.dev/api/v1/repos/acme/app/statuses/abc"
        );
    }
}
