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
    /// The host that users will actually hit (browser-reachable). For local
    /// dev this is `localhost:9000`; in prod, your S3 hostname. Distinct from
    /// `cfg.endpoint` which is the API's internal address.
    public_endpoint: &'a str,
}

impl<'a> Presigner<'a> {
    pub fn new(cfg: &'a MinioConfig) -> Self {
        Self {
            cfg,
            public_endpoint: &cfg.public_endpoint,
        }
    }

    pub fn put(&self, key: &str, content_type: &str, expires_secs: u32) -> String {
        self.sign("PUT", key, expires_secs, Some(content_type), None)
    }

    pub fn get(&self, key: &str, filename: Option<&str>, expires_secs: u32) -> String {
        let disposition = filename.map(|f| format!("attachment; filename=\"{}\"", sanitize(f)));
        self.sign("GET", key, expires_secs, None, disposition.as_deref())
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

        // Host = whatever the user's browser will hit, no scheme/port stripping —
        // SigV4 includes the port in the host header iff non-default.
        let host = host_from(self.public_endpoint);

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

fn host_from(endpoint: &str) -> String {
    // Strip scheme.
    endpoint
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
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
    fn get_url_includes_disposition() {
        let c = cfg();
        let p = Presigner::new(&c);
        let url = p.get("tasks/abc/foo.png", Some("My File.png"), 600);
        // The disposition value gets uri-encoded; just check the marker exists.
        assert!(url.contains("response-content-disposition="));
    }

    #[test]
    fn sanitize_strips_quotes() {
        assert_eq!(sanitize(r#"foo"bar"#), "foobar");
    }
}
