//! Git provider adapters (ADR 0001).
//!
//! Everything in here is pure: providers differ only in how a webhook is
//! verified, how its payload maps to neutral events, and how an outbound
//! API request is shaped. The shared core (linking, transitions, the job
//! runner) never sees a provider payload.
//!
//! Outbound seam: `status_request`. Inbound seam: `verify_signature` +
//! `parse_event` (provider payload → neutral `GitEvent`). GitHub and Gitea
//! share a payload shape (Gitea mirrors GitHub); GitLab differs in event
//! names, field names, and auth (a shared token rather than an HMAC).

use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use subtle::ConstantTimeEq;

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

    /// Header carrying the request's authenticity proof (HMAC signature, or
    /// GitLab's shared token).
    pub fn signature_header(self) -> &'static str {
        match self {
            Self::Github => "x-hub-signature-256",
            Self::Gitlab => "x-gitlab-token",
            Self::Gitea => "x-gitea-signature",
        }
    }

    /// Header naming the event kind.
    pub fn event_header(self) -> &'static str {
        match self {
            Self::Github => "x-github-event",
            Self::Gitlab => "x-gitlab-event",
            Self::Gitea => "x-gitea-event",
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

// ─── inbound: webhook verification + payload → neutral events ────────────────

/// PR/MR lifecycle in the neutral model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

impl PrState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Merged => "merged",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub url: Option<String>,
}

/// What a webhook tells us, stripped of provider specifics. The shared
/// handler turns these into task links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitEvent {
    /// A branch push. `branch` is `None` for tag/other refs (skipped).
    Push {
        branch: Option<String>,
        commits: Vec<Commit>,
    },
    PullRequest {
        number: u64,
        title: String,
        body: String,
        url: Option<String>,
        state: PrState,
        head_sha: Option<String>,
    },
}

fn hmac_hex(secret: &str, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// Verify the authenticity of a webhook. GitHub/Gitea HMAC-SHA256 the body
/// (GitHub prefixes the hex with `sha256=`; Gitea sends bare hex); GitLab
/// compares the shared token. Constant-time throughout.
pub fn verify_signature(provider: Provider, secret: &str, presented: &str, body: &[u8]) -> bool {
    match provider {
        Provider::Github => {
            let Some(hex) = presented.strip_prefix("sha256=") else {
                return false;
            };
            ct_eq(&hmac_hex(secret, body), hex)
        }
        Provider::Gitea => ct_eq(&hmac_hex(secret, body), presented),
        Provider::Gitlab => ct_eq(secret, presented),
    }
}

/// Map a provider webhook (already identified by its event-type header) to
/// neutral events. Unrecognised event types yield an empty vec — we ack and
/// ignore. Returns `BadRequest` only if the body isn't the JSON we expect.
pub fn parse_event(provider: Provider, event_type: &str, body: &[u8]) -> AppResult<Vec<GitEvent>> {
    let json: Value = serde_json::from_slice(body)
        .map_err(|_| AppError::BadRequest("invalid JSON body".into()))?;
    Ok(match provider {
        // Gitea mirrors GitHub's push/pull_request payloads.
        Provider::Github | Provider::Gitea => match event_type {
            "push" => vec![parse_github_push(&json)],
            "pull_request" => parse_github_pr(&json).into_iter().collect(),
            _ => vec![],
        },
        Provider::Gitlab => match event_type {
            "Push Hook" => vec![parse_gitlab_push(&json)],
            "Merge Request Hook" => parse_gitlab_mr(&json).into_iter().collect(),
            _ => vec![],
        },
    })
}

fn branch_from_ref(r: &str) -> Option<String> {
    r.strip_prefix("refs/heads/")
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn commits_from(json: &Value) -> Vec<Commit> {
    json.get("commits")
        .and_then(|c| c.as_array())
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let sha = c.get("id").and_then(Value::as_str)?.to_string();
            Some(Commit {
                sha,
                message: c
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                url: c.get("url").and_then(Value::as_str).map(str::to_string),
            })
        })
        .collect()
}

fn parse_github_push(json: &Value) -> GitEvent {
    let branch = json
        .get("ref")
        .and_then(Value::as_str)
        .and_then(branch_from_ref);
    GitEvent::Push {
        branch,
        commits: commits_from(json),
    }
}

fn parse_github_pr(json: &Value) -> Option<GitEvent> {
    let pr = json.get("pull_request")?;
    let action = json.get("action").and_then(Value::as_str).unwrap_or("");
    let merged = pr.get("merged").and_then(Value::as_bool).unwrap_or(false);
    let state = if merged {
        PrState::Merged
    } else if action == "closed" {
        PrState::Closed
    } else {
        PrState::Open
    };
    Some(GitEvent::PullRequest {
        number: pr.get("number").and_then(Value::as_u64).unwrap_or(0),
        title: pr
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        body: pr
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        url: pr
            .get("html_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        state,
        head_sha: pr
            .get("head")
            .and_then(|h| h.get("sha"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn parse_gitlab_push(json: &Value) -> GitEvent {
    let branch = json
        .get("ref")
        .and_then(Value::as_str)
        .and_then(branch_from_ref);
    GitEvent::Push {
        branch,
        commits: commits_from(json),
    }
}

fn parse_gitlab_mr(json: &Value) -> Option<GitEvent> {
    let attrs = json.get("object_attributes")?;
    // GitLab MR state: opened | closed | locked | merged.
    let state = match attrs.get("state").and_then(Value::as_str).unwrap_or("") {
        "merged" => PrState::Merged,
        "closed" => PrState::Closed,
        _ => PrState::Open,
    };
    Some(GitEvent::PullRequest {
        number: attrs.get("iid").and_then(Value::as_u64).unwrap_or(0),
        title: attrs
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        body: attrs
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        url: attrs.get("url").and_then(Value::as_str).map(str::to_string),
        state,
        head_sha: attrs
            .get("last_commit")
            .and_then(|c| c.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
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

    // ─── inbound: verification ──────────────────────────────────────────────

    #[test]
    fn github_signature_verifies_with_prefix() {
        let body = br#"{"hello":"world"}"#;
        let sig = format!("sha256={}", hmac_hex("s3cr3t", body));
        assert!(verify_signature(Provider::Github, "s3cr3t", &sig, body));
        assert!(!verify_signature(Provider::Github, "wrong", &sig, body));
        assert!(!verify_signature(
            Provider::Github,
            "s3cr3t",
            &sig,
            b"tampered"
        ));
        // Gitea sends bare hex, so the prefixed form must fail for it.
        assert!(!verify_signature(
            Provider::Github,
            "s3cr3t",
            &hmac_hex("s3cr3t", body),
            body
        ));
    }

    #[test]
    fn gitea_signature_is_bare_hex() {
        let body = br#"{"x":1}"#;
        let sig = hmac_hex("k", body);
        assert!(verify_signature(Provider::Gitea, "k", &sig, body));
        assert!(!verify_signature(
            Provider::Gitea,
            "k",
            &format!("sha256={sig}"),
            body
        ));
    }

    #[test]
    fn gitlab_token_is_constant_time_equality() {
        let body = br#"{"x":1}"#;
        assert!(verify_signature(Provider::Gitlab, "tok", "tok", body));
        assert!(!verify_signature(Provider::Gitlab, "tok", "nope", body));
        assert!(!verify_signature(Provider::Gitlab, "tok", "to", body));
    }

    // ─── inbound: payload parsing ───────────────────────────────────────────

    #[test]
    fn github_push_yields_branch_and_commits() {
        let body = br#"{
            "ref": "refs/heads/DEMO-1-add-thing",
            "commits": [
                {"id": "abcdef1234", "message": "DEMO-2 fix it", "url": "http://c/1"}
            ]
        }"#;
        let events = parse_event(Provider::Github, "push", body).unwrap();
        assert_eq!(
            events,
            vec![GitEvent::Push {
                branch: Some("DEMO-1-add-thing".into()),
                commits: vec![Commit {
                    sha: "abcdef1234".into(),
                    message: "DEMO-2 fix it".into(),
                    url: Some("http://c/1".into()),
                }],
            }]
        );
    }

    #[test]
    fn tag_push_has_no_branch() {
        let body = br#"{"ref": "refs/tags/v1.0", "commits": []}"#;
        let events = parse_event(Provider::Gitea, "push", body).unwrap();
        assert_eq!(
            events,
            vec![GitEvent::Push {
                branch: None,
                commits: vec![]
            }]
        );
    }

    #[test]
    fn github_merged_pr_maps_state_and_head_sha() {
        let body = br#"{
            "action": "closed",
            "pull_request": {
                "number": 7, "title": "DEMO-1 thing", "body": "closes DEMO-2",
                "html_url": "http://pr/7", "merged": true, "head": {"sha": "deadbeef"}
            }
        }"#;
        let events = parse_event(Provider::Github, "pull_request", body).unwrap();
        assert_eq!(
            events,
            vec![GitEvent::PullRequest {
                number: 7,
                title: "DEMO-1 thing".into(),
                body: "closes DEMO-2".into(),
                url: Some("http://pr/7".into()),
                state: PrState::Merged,
                head_sha: Some("deadbeef".into()),
            }]
        );
    }

    #[test]
    fn gitlab_mr_uses_object_attributes() {
        let body = br#"{
            "object_attributes": {
                "iid": 12, "title": "DEMO-3 gl", "description": "body",
                "url": "http://gl/mr/12", "state": "merged",
                "last_commit": {"id": "cafe1234"}
            }
        }"#;
        let events = parse_event(Provider::Gitlab, "Merge Request Hook", body).unwrap();
        assert_eq!(
            events,
            vec![GitEvent::PullRequest {
                number: 12,
                title: "DEMO-3 gl".into(),
                body: "body".into(),
                url: Some("http://gl/mr/12".into()),
                state: PrState::Merged,
                head_sha: Some("cafe1234".into()),
            }]
        );
    }

    #[test]
    fn gitlab_push_hook_parses_like_github() {
        let body =
            br#"{"ref":"refs/heads/DEMO-9","commits":[{"id":"aa","message":"m","url":null}]}"#;
        let events = parse_event(Provider::Gitlab, "Push Hook", body).unwrap();
        assert_eq!(
            events,
            vec![GitEvent::Push {
                branch: Some("DEMO-9".into()),
                commits: vec![Commit {
                    sha: "aa".into(),
                    message: "m".into(),
                    url: None
                }],
            }]
        );
    }

    #[test]
    fn unknown_event_types_are_ignored() {
        assert!(parse_event(Provider::Github, "ping", b"{}")
            .unwrap()
            .is_empty());
        assert!(parse_event(Provider::Gitlab, "Tag Push Hook", b"{}")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn bad_json_is_a_bad_request() {
        assert!(matches!(
            parse_event(Provider::Github, "push", b"not json"),
            Err(AppError::BadRequest(_))
        ));
    }
}
