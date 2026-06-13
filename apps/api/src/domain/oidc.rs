//! OIDC single sign-on (F10). See docs/adr/0003.
//!
//! Split into three layers:
//!   • pure crypto/flow: PKCE, the signed flow-state cookie, RS256 `id_token`
//!     validation against a JWKS, the email-domain allowlist — all unit-tested
//!     with a self-minted token, no network.
//!   • DB: `upsert_user` create-or-link by federated identity.
//!   • I/O: discovery / token / JWKS over the curl-subprocess machinery
//!     (ADR 0001), with the token request body — which carries the client
//!     secret and PKCE verifier — piped via stdin so it never hits argv.

use std::process::Stdio;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use jsonwebtoken::{
    decode, decode_header, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{config::AuthConfig, domain::password, AppError, AppResult};

const FLOW_TTL_SECS: i64 = 600; // 10 minutes to complete the redirect dance
const SCOPE: &str = "openid email profile";

// ─── discovery / JWKS / token types ──────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct Discovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Jwk {
    #[serde(default)]
    pub kid: Option<String>,
    pub kty: String,
    /// RSA modulus / exponent, base64url (present for `kty == "RSA"`).
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: String,
}

/// The subset of `id_token` claims we consume. `aud`/`iss`/`exp` are also
/// validated by `jsonwebtoken` from the raw token regardless of this struct.
#[derive(Debug, Clone, Deserialize)]
pub struct IdClaims {
    pub sub: String,
    pub iss: String,
    pub exp: i64,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default, deserialize_with = "de_flexible_bool")]
    pub email_verified: bool,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub nonce: Option<String>,
}

/// Some IdPs send `email_verified` as a JSON bool, others as the string
/// `"true"`. Accept either; absent → false.
fn de_flexible_bool<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize as _;
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Bool(b) => b,
        serde_json::Value::String(s) => s.eq_ignore_ascii_case("true"),
        _ => false,
    })
}

// ─── flow state (signed, browser-bound cookie) ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowState {
    pub state: String,
    pub nonce: String,
    pub verifier: String,
    exp: i64,
}

/// Fresh per-login secrets: opaque `state` + `nonce`, and a high-entropy PKCE
/// `verifier`.
pub fn new_flow() -> FlowState {
    FlowState {
        state: random_token(24),
        nonce: random_token(24),
        verifier: random_token(32),
        exp: (Utc::now() + Duration::seconds(FLOW_TTL_SECS)).timestamp(),
    }
}

/// Sign the flow state into a short-lived HS256 token for the HttpOnly cookie.
pub fn encode_flow(secret: &[u8], flow: &FlowState) -> AppResult<String> {
    encode(&Header::default(), flow, &EncodingKey::from_secret(secret))
        .map_err(|_| AppError::Crypto("flow encode failed"))
}

/// Verify and decode the flow cookie. Unauthorized for tampered/expired.
pub fn decode_flow(secret: &[u8], token: &str) -> AppResult<FlowState> {
    let mut v = Validation::default();
    v.leeway = 30;
    decode::<FlowState>(token, &DecodingKey::from_secret(secret), &v)
        .map(|d| d.claims)
        .map_err(|_| AppError::Unauthorized)
}

/// PKCE S256 challenge for a verifier (RFC 7636).
pub fn pkce_challenge(verifier: &str) -> String {
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(h.finalize())
}

/// Build the IdP authorization-endpoint redirect URL.
pub fn authorize_url(
    disco: &Discovery,
    client_id: &str,
    redirect_uri: &str,
    flow: &FlowState,
) -> String {
    let challenge = pkce_challenge(&flow.verifier);
    format!(
        "{ep}?response_type=code&client_id={cid}&redirect_uri={ru}&scope={scope}\
         &state={st}&nonce={nc}&code_challenge={ch}&code_challenge_method=S256",
        ep = disco.authorization_endpoint,
        cid = enc(client_id),
        ru = enc(redirect_uri),
        scope = enc(SCOPE),
        st = enc(&flow.state),
        nc = enc(&flow.nonce),
        ch = enc(&challenge),
    )
}

// ─── id_token validation ─────────────────────────────────────────────────────

/// Validate an `id_token`: RS256 signature against the JWKS (matched by `kid`),
/// plus `iss`, `aud`, `exp`, and `nonce`. Returns the claims on success,
/// Unauthorized for anything off.
pub fn validate_id_token(
    jwks: &Jwks,
    token: &str,
    issuer: &str,
    client_id: &str,
    expected_nonce: &str,
) -> AppResult<IdClaims> {
    let header = decode_header(token).map_err(|_| AppError::Unauthorized)?;
    let jwk = pick_key(jwks, header.kid.as_deref()).ok_or(AppError::Unauthorized)?;
    if jwk.kty != "RSA" {
        return Err(AppError::Unauthorized);
    }
    let (n, e) = (jwk.n.as_deref(), jwk.e.as_deref());
    let (n, e) = match (n, e) {
        (Some(n), Some(e)) => (n, e),
        _ => return Err(AppError::Unauthorized),
    };
    let key = DecodingKey::from_rsa_components(n, e).map_err(|_| AppError::Unauthorized)?;

    let mut v = Validation::new(Algorithm::RS256);
    v.leeway = 60;
    v.set_issuer(&[issuer]);
    v.set_audience(&[client_id]);

    let claims = decode::<IdClaims>(token, &key, &v)
        .map(|d| d.claims)
        .map_err(|_| AppError::Unauthorized)?;

    // Replay/binding: the token's nonce must match the one we issued.
    if claims.nonce.as_deref() != Some(expected_nonce) {
        return Err(AppError::Unauthorized);
    }
    Ok(claims)
}

fn pick_key<'a>(jwks: &'a Jwks, kid: Option<&str>) -> Option<&'a Jwk> {
    match kid {
        Some(kid) => jwks.keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
        // No kid in the header — only safe if the issuer publishes one key.
        None if jwks.keys.len() == 1 => jwks.keys.first(),
        None => None,
    }
}

/// Is `email` within the allowlist? Empty allowlist = any domain.
pub fn email_allowed(email: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }
    match email.rsplit_once('@') {
        Some((_, domain)) => {
            let domain = domain.to_lowercase();
            allowed.contains(&domain)
        }
        None => false,
    }
}

// ─── claims → user (create or link) ──────────────────────────────────────────

/// Resolve the federated identity to a local user, creating or linking as
/// needed. Returns `(user_id, role)`.
///
/// Order: existing user by (issuer, subject) → existing **active** user by
/// **verified** email (link) → create a fresh member. An existing account with
/// an unverified email match is rejected, so a hostile IdP can't take over a
/// local account by asserting its address.
pub async fn upsert_user(
    db: &PgPool,
    auth_cfg: &AuthConfig,
    issuer: &str,
    claims: &IdClaims,
) -> AppResult<(Uuid, String)> {
    // 1. Already federated — the common case on repeat logins.
    if let Some((id, role)) = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT id, role FROM users
            WHERE oidc_issuer = $1 AND oidc_subject = $2 AND deleted_at IS NULL"#,
    )
    .bind(issuer)
    .bind(&claims.sub)
    .fetch_optional(db)
    .await?
    {
        touch_last_seen(db, id).await?;
        return Ok((id, role));
    }

    let email = claims
        .email
        .as_deref()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .ok_or_else(|| AppError::BadRequest("identity provider did not supply an email".into()))?;

    // 2. Existing local account with this email.
    if let Some((id, role)) = sqlx::query_as::<_, (Uuid, String)>(
        r#"SELECT id, role FROM users WHERE email = $1 AND deleted_at IS NULL"#,
    )
    .bind(&email)
    .fetch_optional(db)
    .await?
    {
        if !claims.email_verified {
            return Err(AppError::Conflict(
                "an account with this email already exists; sign in with your password".into(),
            ));
        }
        // Link the federation onto the existing account.
        sqlx::query(
            r#"UPDATE users SET oidc_issuer = $2, oidc_subject = $3, last_seen_at = now()
                WHERE id = $1"#,
        )
        .bind(id)
        .bind(issuer)
        .bind(&claims.sub)
        .execute(db)
        .await?;
        return Ok((id, role));
    }

    // 3. Create a fresh federated user.
    let id = Uuid::now_v7();
    let handle = unique_handle(db, &email).await?;
    let display_name = claims
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user"))
        .to_string();
    // Federated users authenticate via SSO; give them an unusable random
    // password so the NOT NULL column is satisfied and local login never hits.
    let random_pw = random_token(32);
    let pw_hash = password::hash(auth_cfg, &random_pw)?;

    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role,
                              oidc_issuer, oidc_subject, last_seen_at)
           VALUES ($1, $2, $3, $4, $5, 'member', $6, $7, now())"#,
    )
    .bind(id)
    .bind(&email)
    .bind(&handle)
    .bind(&display_name)
    .bind(&pw_hash)
    .bind(issuer)
    .bind(&claims.sub)
    .execute(db)
    .await?;

    Ok((id, "member".to_string()))
}

async fn touch_last_seen(db: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE users SET last_seen_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// A handle seeded from the email local-part, made unique with a short random
/// suffix (handle is UNIQUE among live users).
async fn unique_handle(db: &PgPool, email: &str) -> AppResult<String> {
    let prefix: String = email
        .split('@')
        .next()
        .unwrap_or("user")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(20)
        .collect::<String>()
        .to_lowercase();
    let prefix = if prefix.len() >= 2 {
        prefix
    } else {
        "user".into()
    };
    for _ in 0..5 {
        let candidate = format!("{prefix}-{}", &Uuid::now_v7().simple().to_string()[..6]);
        let taken: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM users WHERE handle = $1 AND deleted_at IS NULL)"#,
        )
        .bind(&candidate)
        .fetch_one(db)
        .await?;
        if !taken {
            return Ok(candidate);
        }
    }
    Err(AppError::Internal(anyhow::anyhow!(
        "could not allocate a unique handle"
    )))
}

// ─── outbound I/O (curl subprocess, ADR 0001) ────────────────────────────────

/// Fetch `{issuer}/.well-known/openid-configuration`.
pub async fn fetch_discovery(issuer: &str) -> AppResult<Discovery> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let body = curl_get(&url).await?;
    serde_json::from_str(&body)
        .map_err(|_| AppError::Internal(anyhow::anyhow!("bad discovery doc")))
}

/// Fetch the issuer's JWKS.
pub async fn fetch_jwks(jwks_uri: &str) -> AppResult<Jwks> {
    let body = curl_get(jwks_uri).await?;
    serde_json::from_str(&body).map_err(|_| AppError::Internal(anyhow::anyhow!("bad jwks")))
}

/// Exchange an authorization code for tokens. The body (client secret + PKCE
/// verifier) is sent via stdin, not argv.
pub async fn exchange_code(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> AppResult<String> {
    let body = format!(
        "grant_type=authorization_code&code={code}&redirect_uri={ru}\
         &client_id={cid}&client_secret={cs}&code_verifier={cv}",
        code = enc(code),
        ru = enc(redirect_uri),
        cid = enc(client_id),
        cs = enc(client_secret),
        cv = enc(verifier),
    );
    let resp = curl_post_form(token_endpoint, &body).await?;
    let parsed: TokenResponse = serde_json::from_str(&resp).map_err(|_| AppError::Unauthorized)?;
    Ok(parsed.id_token)
}

async fn curl_get(url: &str) -> AppResult<String> {
    run_curl(
        &[
            "-sS",
            "-X",
            "GET",
            url,
            "-w",
            "\n%{http_code}",
            "--max-time",
            "10",
        ],
        None,
    )
    .await
}

async fn curl_post_form(url: &str, body: &str) -> AppResult<String> {
    run_curl(
        &[
            "-sS",
            "-X",
            "POST",
            url,
            "-H",
            "Content-Type: application/x-www-form-urlencoded",
            "-H",
            "Accept: application/json",
            "--data-binary",
            "@-",
            "-w",
            "\n%{http_code}",
            "--max-time",
            "10",
        ],
        Some(body),
    )
    .await
}

/// Run curl, returning the response body. Errors on a non-2xx status or a
/// transport failure. `stdin_body`, when present, is written to curl's stdin
/// (used with `--data-binary @-`) so secrets never appear in the argv.
async fn run_curl(args: &[&str], stdin_body: Option<&str>) -> AppResult<String> {
    let mut cmd = tokio::process::Command::new("curl");
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    if stdin_body.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Internal(anyhow::anyhow!("spawn curl: {e}")))?;
    if let Some(body) = stdin_body {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin
            .write_all(body.as_bytes())
            .await
            .map_err(|e| AppError::Internal(anyhow::anyhow!("write curl stdin: {e}")))?;
        // Drop closes the pipe → curl sees EOF and sends the request.
        drop(stdin);
    }
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("curl wait: {e}")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Body and the appended status code are separated by the final newline.
    let (body, status) = stdout.rsplit_once('\n').unwrap_or(("", stdout.trim()));
    if !status.starts_with('2') {
        return Err(AppError::Internal(anyhow::anyhow!(
            "oidc http {status} from {}",
            args.last().copied().unwrap_or("?")
        )));
    }
    Ok(body.to_string())
}

// ─── small helpers ───────────────────────────────────────────────────────────

fn random_token(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}

/// Percent-encode a query-parameter value (RFC 3986 unreserved set kept).
fn enc(s: &str) -> String {
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

    // A throwaway RSA keypair for tests only.
    const TEST_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQDLLu6pC2gnlnpy
iAoQ76COrf0hKDnuVE5eM+vsJ88nkKKgra5RlPWmb+jrId/nY3FyQ5oLMv8OCzRu
ctm+gx+h2B3BCYZYQVIWID8XMuWz/R8++1VcPPjTZuxovpghZJOiUB2AnQ4vqmaH
/vOt17Y+2qKSL3hdWAh3Hmve/e5Evc7D8vPaGRWpfdMgEMzeZsE3q/wiL1SX0GqB
uT454Kt4o2mdwYeoyKxpjzYmRl/Xf8wpLLqAzBVtD6rDKC3ySnjqfZHPUC2cS5IT
CofRxoUopkqX4ea9GGULm+BdsyZfOjwYNmI6ZAah14lQ6meviPWX6AhFudH+cK7G
JhFPVXWhAgMBAAECggEAJBMtJK51y7GYBAXLY75oD20s6FowDvTBBVDKrp9S1H+F
oGm17Z45D1gHTtgw3PB3EAaYryxaxK+Qm5ugtYaqcx3gCooaZEkUvgDzsrbCufZT
Oed9/GaG92Hqz54nfKZS4BrBYjiAcE4c7kCCG3eVUAuZmcL75/bdaejo1irXxzRq
8BGWSUtpAlcpUMea2Y7ARa2aiJoZDojtU6QjjUh/6m5kY3ayq9+BbOzfyzk51rtJ
1Eqlkc9C7Nir8hyk3lov8ugqIhZ1QZ4TWZFxDmd0tdrwahAyRUzBctWTqHZE7AzP
JMuKcn2TtK+X4wprmDx3FqWLNZsqxXw+OVstsr9vwQKBgQD1mG8WKn85TIJJzrgK
DRrwf4b1dfHyeSIznqtW2BeV3bgU/GTh/8/2h5aJMto7lwI6K3DevQLwxH/iuMNA
GqHNNNOUq1Upec4+RaNt73oDmmyIrOL5gecIER6/ELKnygI59FlT2U2PXOGzP48s
cLx3SdTbnXYZbjzI5U3Dj1dAzQKBgQDTyoZHOdDmW2UPK9ZZXVx+3ghVH2vmdkmh
W5X5TaG4hZJiwgfxBZcsw9LoFik8dB+SLSgmHfhibxrO7nwCGunrDCsQ5EDnnsOe
yd1+vQ/6y2UuULarSMREb0o1rC2yip7QNQ2P9m2KUPGv/h76xNNfWPksPGisfhiJ
FpVxQr14JQKBgQDGcvHwW06KIkR1F1Cm7ogCJUoMNc3XPAJi66dPeTU1p/8DFh7+
bxLABjehjTHCTPdDwa5mjRw/KMidjuZWei6b/j+pNfiOlxoMP4EbaBKTrshceDa/
njPs1MifYwK5igXahpNXqZN+cHL/wDAUnNPtH/+bpdS0H9uGCaOYjc7XUQKBgG75
W8rOfKt3kEiWy8YfqWvAo9UWlc84g/RMrRTonKi3NLESwl6Ec2Y9ZbG+ivTmU/Sg
PL1cTt4lIYL0a34e5BsJUTeUon27Lv1xAOJ75nefQ/E00cKGanEBb30YLwmyoOyw
H98jXNpw93MkUM9NewQm9sk7Dg30NJ8AemXSdr0RAoGBAJ/WnKyLJcRnIJmzqSPX
fHAxJbfbOyQWmjGtV1eGujPCtpr7GZbTJTbNlpPlwfX67pLBYQz6Jup14zuAx1e2
vt4BX3gCAN/NFMakYffZRD8nbrzh2uI9difjpP6Lz2luheWfTPvpv+SjlIyVtq88
3occyeqg8/+zWqKeK1xSrF6p
-----END PRIVATE KEY-----";
    const TEST_N: &str = "yy7uqQtoJ5Z6cogKEO-gjq39ISg57lROXjPr7CfPJ5CioK2uUZT1pm_o6yHf52NxckOaCzL_Dgs0bnLZvoMfodgdwQmGWEFSFiA_FzLls_0fPvtVXDz402bsaL6YIWSTolAdgJ0OL6pmh_7zrde2Ptqiki94XVgIdx5r3v3uRL3Ow_Lz2hkVqX3TIBDM3mbBN6v8Ii9Ul9Bqgbk-OeCreKNpncGHqMisaY82JkZf13_MKSy6gMwVbQ-qwygt8kp46n2Rz1AtnEuSEwqH0caFKKZKl-HmvRhlC5vgXbMmXzo8GDZiOmQGodeJUOpnr4j1l-gIRbnR_nCuxiYRT1V1oQ";
    const KID: &str = "test-key-1";
    const ISS: &str = "https://idp.test";
    const AUD: &str = "sprintly-client";

    fn jwks() -> Jwks {
        Jwks {
            keys: vec![Jwk {
                kid: Some(KID.into()),
                kty: "RSA".into(),
                n: Some(TEST_N.into()),
                e: Some("AQAB".into()),
            }],
        }
    }

    fn mint(claims: serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(KID.into());
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(TEST_PEM.as_bytes()).unwrap(),
        )
        .unwrap()
    }

    fn good_claims(nonce: &str) -> serde_json::Value {
        serde_json::json!({
            "iss": ISS,
            "aud": AUD,
            "sub": "idp-user-1",
            "exp": (Utc::now() + Duration::hours(1)).timestamp(),
            "email": "dev@idp.test",
            "email_verified": true,
            "name": "Dev User",
            "nonce": nonce,
        })
    }

    #[test]
    fn pkce_matches_rfc7636_vector() {
        // RFC 7636 Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn valid_id_token_passes() {
        let token = mint(good_claims("n0nce"));
        let claims = validate_id_token(&jwks(), &token, ISS, AUD, "n0nce").unwrap();
        assert_eq!(claims.sub, "idp-user-1");
        assert_eq!(claims.email.as_deref(), Some("dev@idp.test"));
        assert!(claims.email_verified);
    }

    #[test]
    fn tampered_signature_is_rejected() {
        let token = mint(good_claims("n0nce"));
        let parts: Vec<&str> = token.split('.').collect();
        let sig = parts[2];
        // Mutate the FIRST signature char — it encodes the top 6 bits of the
        // first signature byte, so the decoded bytes always change (flipping the
        // last char can be a no-op on padding bits).
        let first = sig.chars().next().unwrap();
        let replacement = if first == 'A' { 'B' } else { 'A' };
        let tampered = format!("{}.{}.{}{}", parts[0], parts[1], replacement, &sig[1..]);
        assert!(matches!(
            validate_id_token(&jwks(), &tampered, ISS, AUD, "n0nce"),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn wrong_nonce_issuer_audience_or_expiry_rejected() {
        let token = mint(good_claims("right"));
        // nonce mismatch
        assert!(matches!(
            validate_id_token(&jwks(), &token, ISS, AUD, "wrong"),
            Err(AppError::Unauthorized)
        ));
        // issuer mismatch
        assert!(matches!(
            validate_id_token(&jwks(), &token, "https://evil.test", AUD, "right"),
            Err(AppError::Unauthorized)
        ));
        // audience mismatch
        assert!(matches!(
            validate_id_token(&jwks(), &token, ISS, "other-client", "right"),
            Err(AppError::Unauthorized)
        ));
        // expired
        let expired = mint(serde_json::json!({
            "iss": ISS, "aud": AUD, "sub": "x",
            "exp": (Utc::now() - Duration::hours(1)).timestamp(),
            "nonce": "right",
        }));
        assert!(matches!(
            validate_id_token(&jwks(), &expired, ISS, AUD, "right"),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn unknown_kid_is_rejected() {
        let token = mint(good_claims("n0nce"));
        let other = Jwks {
            keys: vec![Jwk {
                kid: Some("different".into()),
                kty: "RSA".into(),
                n: Some(TEST_N.into()),
                e: Some("AQAB".into()),
            }],
        };
        assert!(matches!(
            validate_id_token(&other, &token, ISS, AUD, "n0nce"),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn flow_cookie_round_trips_and_rejects_tampering() {
        let secret = b"a-test-secret-that-is-long-enough-here";
        let flow = new_flow();
        let token = encode_flow(secret, &flow).unwrap();
        let back = decode_flow(secret, &token).unwrap();
        assert_eq!(back.state, flow.state);
        assert_eq!(back.verifier, flow.verifier);
        // Wrong key → rejected.
        assert!(matches!(
            decode_flow(b"a-different-secret-of-sufficient-len", &token),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn allowlist_enforced() {
        let allow = vec!["aline.fit".to_string(), "idp.test".to_string()];
        assert!(email_allowed("dev@idp.test", &allow));
        assert!(email_allowed("DEV@IDP.TEST", &allow));
        assert!(!email_allowed("dev@evil.test", &allow));
        assert!(!email_allowed("not-an-email", &allow));
        // Empty allowlist allows anything.
        assert!(email_allowed("anyone@anywhere.test", &[]));
    }

    #[test]
    fn authorize_url_has_pkce_and_state() {
        let disco = Discovery {
            authorization_endpoint: "https://idp.test/authorize".into(),
            token_endpoint: "https://idp.test/token".into(),
            jwks_uri: "https://idp.test/jwks".into(),
        };
        let flow = new_flow();
        let url = authorize_url(&disco, AUD, "https://app/cb", &flow);
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains(&format!("state={}", enc(&flow.state))));
        assert!(url.contains("scope=openid%20email%20profile"));
        // The raw verifier must never appear in the redirect URL.
        assert!(!url.contains(&flow.verifier));
    }
}
