//! CSRF — double-submit cookie pattern.
//!
//! On login/refresh we set a non-HttpOnly cookie `sprintly_csrf` containing
//! a random nonce. JS in the browser reads that cookie and mirrors it as
//! the `X-CSRF-Token` header on every write request.
//!
//! For a request to be accepted by this middleware:
//!   • Method is in {GET, HEAD, OPTIONS}                          → pass-through
//!   • Path is on the exempt list (login, register, refresh,
//!     password-reset request/confirm)                            → pass-through
//!   • Request uses Authorization: Bearer (no cookie at all)      → pass-through
//!   • Otherwise: header value must equal cookie value, constant-time.
//!
//! Why exempt `/auth/refresh` — the user might not have an active session
//! when refresh fires (just expired access token). The refresh cookie itself
//! is `SameSite=Lax + Path=/api/v1/auth` so it's already CSRF-resistant for
//! the modern browsers we care about.

use axum::{
    extract::Request,
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;

const CSRF_COOKIE: &str = "sprintly_csrf";
const CSRF_HEADER: &str = "x-csrf-token";

const EXEMPT_PATHS: &[&str] = &[
    "/api/v1/auth/login",
    // 2FA step-up: presents a signed challenge token, not a cookie session yet.
    "/api/v1/auth/2fa",
    "/api/v1/auth/register",
    "/api/v1/auth/refresh",
    "/api/v1/auth/password/reset/request",
    "/api/v1/auth/password/reset/confirm",
    // Force-reset spends a signed challenge (no cookie session yet), like 2FA.
    "/api/v1/auth/password/change",
    // Inbound webhook authenticates via HMAC signature, not a cookie.
    "/api/v1/integrations/github/webhook",
    // probes are GETs, but defense in depth:
    "/api/v1/healthz",
    "/api/v1/readyz",
    "/healthz",
    "/readyz",
];

/// Per-connection inbound git webhooks: `/api/v1/integrations/<provider>/webhook/<id>`.
/// They authenticate via provider signatures against the connection's secret.
fn is_provider_webhook(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/api/v1/integrations/") else {
        return false;
    };
    let mut parts = rest.split('/');
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(_provider), Some("webhook"), Some(id), None) if !id.is_empty()
    )
}

pub async fn csrf_guard(req: Request, next: Next) -> Response {
    if !is_write(req.method()) || is_exempt(req.uri().path()) || is_bearer(&req) {
        return next.run(req).await;
    }

    let header_val = req
        .headers()
        .get(CSRF_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let cookie_val = req
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| {
            raw.split(';')
                .map(|kv| kv.trim())
                .find_map(|kv| kv.strip_prefix(&format!("{CSRF_COOKIE}=")))
                .map(|s| s.to_string())
        });

    match (header_val, cookie_val) {
        (Some(h), Some(c)) if !h.is_empty() && bool::from(h.as_bytes().ct_eq(c.as_bytes())) => {
            next.run(req).await
        }
        _ => (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": {
                    "code": "csrf",
                    "message": "Missing or mismatched CSRF token."
                }
            })),
        )
            .into_response(),
    }
}

fn is_write(method: &Method) -> bool {
    matches!(
        *method,
        Method::POST | Method::PATCH | Method::PUT | Method::DELETE
    )
}

fn is_exempt(path: &str) -> bool {
    EXEMPT_PATHS.contains(&path) || is_provider_webhook(path)
}

fn is_bearer(req: &Request) -> bool {
    req.headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.starts_with("Bearer "))
        .unwrap_or(false)
}

/// Generate a fresh CSRF nonce (base64-url, 24 bytes of entropy).
pub fn fresh_nonce() -> String {
    use base64::Engine as _;
    use rand::RngCore;
    let mut raw = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut raw);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_webhook_paths_are_exempt() {
        assert!(is_provider_webhook(
            "/api/v1/integrations/gitlab/webhook/0190b50e-aaaa-bbbb-cccc-ddddeeeeffff"
        ));
        assert!(!is_provider_webhook("/api/v1/integrations/github/webhook")); // legacy: exact list
        assert!(!is_provider_webhook("/api/v1/integrations/github/webhook/"));
        assert!(!is_provider_webhook(
            "/api/v1/integrations/github/webhook/x/extra"
        ));
        assert!(!is_provider_webhook("/api/v1/projects/x/integrations"));
    }
}
