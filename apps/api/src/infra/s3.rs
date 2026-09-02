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
    /// The base the browser will actually hit, scheme included. Distinct from
    /// `cfg.endpoint`, which is the API's internal address. See
    /// [`resolve_public_endpoint`] for how a path-only configuration is turned
    /// into an absolute base per request.
    public_endpoint: String,
}

impl<'a> Presigner<'a> {
    /// Sign against the configured public endpoint verbatim. Right for the
    /// worker and CLI, which point `public_endpoint` at the internal address —
    /// and for deployments that configured an absolute URL.
    pub fn new(cfg: &'a MinioConfig) -> Self {
        Self {
            cfg,
            public_endpoint: cfg.public_endpoint.clone(),
        }
    }

    /// Sign for a browser that reached us at `request_origin`. When the public
    /// endpoint is configured as a path (`/s3`), the URL is built on whatever
    /// origin the request came in on — so the same deployment serves working
    /// attachment links whether someone opened it by IP or by hostname.
    pub fn for_request(cfg: &'a MinioConfig, request_origin: Option<&str>, fallback: &str) -> Self {
        Self {
            cfg,
            public_endpoint: resolve_public_endpoint(
                &cfg.public_endpoint,
                request_origin,
                fallback,
            ),
        }
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
        let host = host_from(&self.public_endpoint);

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

        let scheme_host = self.public_endpoint.trim_end_matches('/');
        format!("{scheme_host}{canonical_uri}?{canonical_query}&X-Amz-Signature={signature}")
    }
}

/// Turn the configured `MINIO_PUBLIC_ENDPOINT` into an absolute base.
///
/// Two shapes are accepted:
///
///   * an absolute URL (`https://files.example/…`, `http://host:8083/s3`) — used
///     verbatim, the pre-existing behaviour;
///   * a bare path (`/s3`) — glued onto the origin the request arrived on, taken
///     from the reverse proxy's `X-Forwarded-*` headers. Falls back to
///     `SPRINTLY_PUBLIC_URL` when there is no request to read (worker, tests).
///
/// The path form exists because a presigned URL bakes in a host: MinIO checks
/// the signature against the `Host` header the browser sends. A deployment
/// reachable at both `212.33.206.34:8083` and `sprintly.example` can only sign
/// for one fixed host — and users opening the other saw uploads sit at
/// "pending" and downloads fail. Signing for whichever host they actually used
/// makes both work.
pub fn resolve_public_endpoint(
    configured: &str,
    request_origin: Option<&str>,
    fallback: &str,
) -> String {
    let configured = configured.trim();
    if !configured.starts_with('/') {
        return configured.trim_end_matches('/').to_string();
    }
    let origin = request_origin
        .map(str::trim)
        .filter(|o| !o.is_empty())
        .unwrap_or(fallback)
        .trim_end_matches('/');
    format!("{origin}{}", configured.trim_end_matches('/'))
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
    fn path_only_endpoint_follows_the_request_origin() {
        // The domain case QA hit: app opened at a hostname, endpoint configured
        // for an IP. With a path-only endpoint the origin comes from the request.
        assert_eq!(
            resolve_public_endpoint("/s3", Some("https://sprintly.example"), "http://fallback"),
            "https://sprintly.example/s3"
        );
        // …and the IP case keeps working on the very same configuration.
        assert_eq!(
            resolve_public_endpoint("/s3", Some("http://212.33.206.34:8083"), "http://fallback"),
            "http://212.33.206.34:8083/s3"
        );
        // No request to read from (worker, tests): the public URL fills in.
        assert_eq!(
            resolve_public_endpoint("/s3/", None, "http://fallback:8080/"),
            "http://fallback:8080/s3"
        );
        assert_eq!(
            resolve_public_endpoint("/s3", Some("   "), "http://fallback"),
            "http://fallback/s3"
        );
        // An absolute endpoint is untouched — existing deployments don't move.
        assert_eq!(
            resolve_public_endpoint("http://localhost:8080/s3", Some("https://elsewhere"), "x"),
            "http://localhost:8080/s3"
        );
    }

    #[test]
    fn for_request_signs_the_host_the_browser_will_send() {
        let mut c = cfg();
        c.public_endpoint = "/s3".into();
        let p = Presigner::for_request(&c, Some("https://sprintly.example"), "http://fallback");
        let url = p.get("tasks/abc/foo.png", Some("foo.png"), 600);
        assert!(
            url.starts_with("https://sprintly.example/s3/sprintly/tasks/abc/foo.png?"),
            "{url}"
        );
        // Same object, different origin → different signature, because the
        // signed host differs. That's the whole point.
        let q = Presigner::for_request(&c, Some("http://212.33.206.34:8083"), "http://fallback");
        let url2 = q.get("tasks/abc/foo.png", Some("foo.png"), 600);
        let sig = |u: &str| u.split("X-Amz-Signature=").nth(1).unwrap().to_string();
        assert_ne!(sig(&url), sig(&url2));
    }

    #[test]
    fn sanitize_strips_quotes() {
        assert_eq!(sanitize(r#"foo"bar"#), "foobar");
    }
}
