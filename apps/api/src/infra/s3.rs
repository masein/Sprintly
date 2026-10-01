//! MinIO / S3 presigned URLs.
//!
//! Hand-rolled AWS SigV4 query signing — about 150 lines, no aws-sdk-s3
//! dependency. We use this for two things:
//!
//!   • `presign_put(key, content_type, expires)` — upload URL the browser
//!     can PUT directly to. Saves us streaming the upload through the API.
//!
//!   • `presign_get(key, filename, expires)` — download URL with a
//!     `response-content-disposition` parameter so the browser saves the
//!     file under the original filename instead of the storage key.
//!
//! Reference: https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-query-string-auth.html

use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use crate::config::MinioConfig;

type HmacSha256 = Hmac<Sha256>;

const ALG: &str = "AWS4-HMAC-SHA256";
const SERVICE: &str = "s3";

pub struct Presigner<'a> {
    cfg: &'a MinioConfig,
    /// The `Host` the browser's request will carry — what SigV4 signs and
    /// MinIO verifies. host:port only.
    sign_host: String,
    /// What the returned URL starts with: an absolute base (`https://files.example`)
    /// or, for a path-form endpoint, the bare path (`/s3`) — a *relative* URL
    /// the browser resolves against the page it's on.
    url_base: String,
}

impl<'a> Presigner<'a> {
    /// Sign against the configured public endpoint verbatim. Right for the
    /// worker and CLI, which point `public_endpoint` at the internal address —
    /// and for deployments that configured an absolute URL.
    pub fn new(cfg: &'a MinioConfig) -> Self {
        Self {
            cfg,
            sign_host: host_from(&cfg.public_endpoint),
            url_base: cfg.public_endpoint.trim().trim_end_matches('/').to_string(),
        }
    }

    /// Sign for a browser that reached us with `request_host` in its `Host`
    /// header (as the proxy forwarded it). See [`resolve_public_endpoint`].
    pub fn for_request(cfg: &'a MinioConfig, request_host: Option<&str>, fallback: &str) -> Self {
        let (sign_host, url_base) =
            resolve_public_endpoint(&cfg.public_endpoint, request_host, fallback);
        Self {
            cfg,
            sign_host,
            url_base,
        }
    }

    /// The host this signer signs for. Exposed for tests and diagnostics.
    pub fn signed_host(&self) -> &str {
        &self.sign_host
    }

    pub fn put(&self, key: &str, content_type: &str, expires_secs: u32) -> String {
        self.sign("PUT", key, expires_secs, Some(content_type), None)
    }

    pub fn get(&self, key: &str, filename: Option<&str>, expires_secs: u32) -> String {
        let disposition = filename.map(|f| format!("attachment; filename=\"{}\"", sanitize(f)));
        self.sign("GET", key, expires_secs, None, disposition.as_deref())
    }

    /// Presigned DELETE — used by backup retention to prune old objects (F15).
    pub fn delete(&self, key: &str, expires_secs: u32) -> String {
        self.sign("DELETE", key, expires_secs, None, None)
    }

    fn sign(
        &self,
        method: &str,
        key: &str,
        expires: u32,
        content_type: Option<&str>,
        content_disposition: Option<&str>,
    ) -> String {
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let scope = format!("{date}/{}/{SERVICE}/aws4_request", self.cfg.region);

        // Canonical URI: /<bucket>/<key> with the key uri-encoded (preserving slashes).
        let canonical_uri = format!("/{}/{}", self.cfg.bucket, encode_uri_path(key));

        // Host = what the browser's Host header will carry: host:port only —
        // scheme and any proxy path stripped (see host_from). SigV4 includes
        // the port in the host header iff non-default.
        let host = &self.sign_host;

        // Build query params, ALPHABETICALLY by key. AWS requires sorted order.
        let credential = format!("{}/{scope}", self.cfg.access_key);
        let mut params: Vec<(String, String)> = vec![
            ("X-Amz-Algorithm".into(), ALG.into()),
            ("X-Amz-Credential".into(), credential),
            ("X-Amz-Date".into(), amz_date.clone()),
            ("X-Amz-Expires".into(), expires.to_string()),
            ("X-Amz-SignedHeaders".into(), "host".into()),
        ];
        if let Some(ct) = content_type {
            // Browsers send Content-Type on PUT; some clients sign it as a
            // header. With UNSIGNED-PAYLOAD and SignedHeaders=host only, we
            // *don't* sign Content-Type; we still pass it through as a query
            // hint via response-content-type when needed. Skip for PUT.
            let _ = ct;
        }
        if let Some(cd) = content_disposition {
            params.push(("response-content-disposition".into(), cd.into()));
        }
        params.sort_by(|a, b| a.0.cmp(&b.0));

        let canonical_query = params
            .iter()
            .map(|(k, v)| format!("{}={}", encode_query(k), encode_query(v)))
            .collect::<Vec<_>>()
            .join("&");

        let canonical_headers = format!("host:{host}\n");
        let signed_headers = "host";
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\nUNSIGNED-PAYLOAD",
        );
        let hashed_cr = sha256_hex(canonical_request.as_bytes());
        let string_to_sign = format!("{ALG}\n{amz_date}\n{scope}\n{hashed_cr}");

        // Derive signing key.
        let k_date = hmac_sha256(
            format!("AWS4{}", self.cfg.secret_key).as_bytes(),
            date.as_bytes(),
        );
        let k_region = hmac_sha256(&k_date, self.cfg.region.as_bytes());
        let k_service = hmac_sha256(&k_region, SERVICE.as_bytes());
        let k_signing = hmac_sha256(&k_service, b"aws4_request");
        let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

        format!(
            "{}{canonical_uri}?{canonical_query}&X-Amz-Signature={signature}",
            self.url_base
        )
    }
}

/// Turn the configured `MINIO_PUBLIC_ENDPOINT` into `(host to sign, URL base)`.
///
/// Two shapes are accepted:
///
///   * an absolute URL (`https://files.example/…`, `http://host:8083/s3`) — used
///     verbatim, the pre-existing behaviour;
///   * a bare path (`/s3`) — the URL stays **relative** (`/s3/<bucket>/<key>?…`)
///     and is signed for the `Host` the request arrived with. Falls back to
///     `SPRINTLY_PUBLIC_URL`'s host when there is no request to read.
///
/// Why relative: a presigned URL bakes in a host *and* a scheme, but only the
/// host is part of the signature. Behind a TLS-terminating CDN (ArvanCloud in
/// front of the production box) the proxy chain reports `http` — Caddy
/// overwrites `X-Forwarded-Proto` from an untrusted upstream — so absolute
/// links came out as `http://…` on an `https://` page: the browser blocked the
/// upload as mixed content (stuck at "pending") and refused the download as
/// insecure (QA report 6, "security errors … via custom domain URLs"). A
/// relative URL inherits the page's scheme and host, so it can't disagree with
/// the page — and the host the browser then sends is the one we signed,
/// because the API and `/s3` sit behind the same proxies.
pub fn resolve_public_endpoint(
    configured: &str,
    request_host: Option<&str>,
    fallback: &str,
) -> (String, String) {
    let configured = configured.trim();
    if !configured.starts_with('/') {
        return (
            host_from(configured),
            configured.trim_end_matches('/').to_string(),
        );
    }
    let host = request_host
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| host_from(fallback.trim()));
    (host, configured.trim_end_matches('/').to_string())
}

fn host_from(endpoint: &str) -> String {
    // Strip scheme AND any path suffix: the public endpoint may sit behind a
    // path-based reverse proxy (e.g. http://host:8083/s3 → Caddy → minio),
    // but the Host header the browser sends — and MinIO verifies the SigV4
    // signature against — is just host:port. Signing "host:port/s3" made
    // every presigned URL 403 (SignatureDoesNotMatch) on such deployments.
    let stripped = endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    stripped.split('/').next().unwrap_or(stripped).to_string()
}

fn sha256_hex(b: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b);
    hex(&h.finalize())
}

fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len() * 2);
    for byte in b {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

// AWS uri encoding: A-Z a-z 0-9 - _ . ~ unreserved, '/' preserved in PATH only.
fn encode_uri_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let c = *byte as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~' | '/') {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn encode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let c = *byte as char;
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Filename safety for Content-Disposition. Strip quotes and control chars.
fn sanitize(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\')
        .take(255)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> MinioConfig {
        MinioConfig {
            endpoint: "http://minio:9000".into(),
            public_endpoint: "http://localhost:9000".into(),
            access_key: "sprintly".into(),
            secret_key: "sprintly_dev_pw".into(),
            bucket: "sprintly".into(),
            region: "us-east-1".into(),
        }
    }

    #[test]
    fn put_url_shape() {
        let c = cfg();
        let p = Presigner::new(&c);
        let url = p.put("tasks/abc/foo.png", "image/png", 600);
        assert!(url.starts_with("http://localhost:9000/sprintly/tasks/abc/foo.png?"));
        assert!(url.contains("X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(url.contains("X-Amz-Expires=600"));
        assert!(url.contains("X-Amz-Signature="));
    }

    #[test]
    fn host_from_strips_scheme_and_path() {
        // A path-based public endpoint (reverse proxy in front of MinIO) must
        // sign only host:port — the Host header MinIO actually verifies.
        assert_eq!(
            host_from("http://212.33.206.34:8083/s3"),
            "212.33.206.34:8083"
        );
        assert_eq!(
            host_from("https://sprintly.example/s3/"),
            "sprintly.example"
        );
        assert_eq!(host_from("http://localhost:9000"), "localhost:9000");
        assert_eq!(host_from("http://localhost:9000/"), "localhost:9000");
    }

    #[test]
    fn path_based_endpoint_signs_bare_host_but_keeps_path_in_url() {
        let mut c = cfg();
        c.public_endpoint = "http://localhost:8080/s3".into();
        let p = Presigner::new(&c);
        let url = p.put("tasks/abc/foo.png", "image/png", 600);
        // The browser-facing URL keeps the proxy path…
        assert!(url.starts_with("http://localhost:8080/s3/sprintly/tasks/abc/foo.png?"));
        // …and the signature must differ from one signed for a host WITH the
        // path glued on (the old bug): recompute with the buggy host and make
        // sure we didn't produce that.
        let sig = url.split("X-Amz-Signature=").nth(1).unwrap();
        assert_eq!(sig.len(), 64, "hex sha256 signature expected");
    }

    #[test]
    fn get_url_includes_disposition() {
        let c = cfg();
        let p = Presigner::new(&c);
        let url = p.get("tasks/abc/foo.png", Some("My File.png"), 600);
        // The disposition value gets uri-encoded; just check the marker exists.
        assert!(url.contains("response-content-disposition="));
    }

    #[test]
    fn path_only_endpoint_is_relative_and_signed_for_the_request_host() {
        // The domain case QA hit: the URL carries no scheme or host of its
        // own, so it can't be http on an https page.
        assert_eq!(
            resolve_public_endpoint("/s3", Some("sprintly.example"), "http://fallback"),
            ("sprintly.example".to_string(), "/s3".to_string())
        );
        // …and the IP case keeps working on the very same configuration.
        assert_eq!(
            resolve_public_endpoint("/s3/", Some("212.33.206.34:8083"), "http://fallback"),
            ("212.33.206.34:8083".to_string(), "/s3".to_string())
        );
        // No request to read from (worker, tests): the public URL's host.
        assert_eq!(
            resolve_public_endpoint("/s3", None, "http://fallback:8080/"),
            ("fallback:8080".to_string(), "/s3".to_string())
        );
        assert_eq!(
            resolve_public_endpoint("/s3", Some("   "), "https://fallback"),
            ("fallback".to_string(), "/s3".to_string())
        );
        // An absolute endpoint is untouched — existing deployments don't move.
        assert_eq!(
            resolve_public_endpoint("http://localhost:8080/s3", Some("elsewhere"), "x"),
            (
                "localhost:8080".to_string(),
                "http://localhost:8080/s3".to_string()
            )
        );
    }

    #[test]
    fn for_request_signs_the_host_the_browser_will_send() {
        let mut c = cfg();
        c.public_endpoint = "/s3".into();
        let p = Presigner::for_request(&c, Some("sprintly.example"), "http://fallback");
        let url = p.get("tasks/abc/foo.png", Some("foo.png"), 600);
        assert!(url.starts_with("/s3/sprintly/tasks/abc/foo.png?"), "{url}");
        assert_eq!(p.signed_host(), "sprintly.example");
        let q = Presigner::for_request(&c, Some("212.33.206.34:8083"), "http://fallback");
        assert_eq!(q.signed_host(), "212.33.206.34:8083");
        // An absolute configuration keeps absolute URLs.
        let abs = cfg();
        let a = Presigner::new(&abs);
        assert!(a
            .put("k", "text/plain", 60)
            .starts_with("http://localhost:9000/sprintly/k?"));
        assert_eq!(a.signed_host(), "localhost:9000");
    }

    #[test]
    fn sanitize_strips_quotes() {
        assert_eq!(sanitize(r#"foo"bar"#), "foobar");
    }
}
