//! Token primitives.
//!
//!   • Access tokens are JWTs (HS256) with a short TTL. They carry user_id,
//!     role, and session_id so a single SELECT can validate them without
//!     hitting the DB for the user row — but handlers do still hit the DB
//!     when they need to enforce non-cached state (e.g. `status='suspended'`
//!     should kick the user out immediately).
//!
//!   • Refresh tokens are opaque random bytes. We store SHA-256(secret), not
//!     the secret itself. They rotate on every use, and we keep the rotation
//!     chain so reuse of a stale token can be detected and used to revoke the
//!     entire session family.
//!
//!  Format on the wire for refresh tokens: base64url(no-padding) of 32 random
//!  bytes. The DB stores the SHA-256 of those 32 bytes.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{config::AuthConfig, AppError, AppResult};

/// JWT claims for the access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessClaims {
    pub sub: Uuid,    // user_id
    pub sid: Uuid,    // session_id
    pub role: String, // 'admin' | 'member' | 'viewer'
    pub iat: i64,
    pub exp: i64,
}

/// Mint a fresh access token.
pub fn mint_access(
    cfg: &AuthConfig,
    user_id: Uuid,
    session_id: Uuid,
    role: &str,
) -> AppResult<String> {
    let now = Utc::now();
    let claims = AccessClaims {
        sub: user_id,
        sid: session_id,
        role: role.to_string(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(cfg.access_ttl_secs as i64)).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&cfg.jwt_secret),
    )
    .map_err(|_| AppError::Crypto("jwt encode failed"))
}

/// Verify and decode an access token. Returns claims on success, Unauthorized
/// for anything off (bad signature, expired, malformed).
pub fn verify_access(cfg: &AuthConfig, token: &str) -> AppResult<AccessClaims> {
    let mut validation = Validation::default();
    // 60s leeway is industry-standard and handles minor clock skew.
    validation.leeway = 60;
    decode::<AccessClaims>(
        token,
        &DecodingKey::from_secret(&cfg.jwt_secret),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(|_| AppError::Unauthorized)
}

/// Claims for the short-lived "you passed the password, now prove the second
/// factor" challenge token (F11). Issued by `/auth/login` when the account has
/// 2FA enabled and spent at `/auth/2fa`. The `purpose` field is an explicit
/// guard so an access token can never be replayed here and vice versa.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwoFactorChallenge {
    pub sub: Uuid, // user_id
    pub role: String,
    pub purpose: String, // always "2fa"
    pub iat: i64,
    pub exp: i64,
}

const TWOFA_CHALLENGE_TTL_SECS: i64 = 300; // 5 minutes to enter a code

/// Mint a 2FA step-up challenge for `user_id`.
pub fn mint_2fa_challenge(cfg: &AuthConfig, user_id: Uuid, role: &str) -> AppResult<String> {
    let now = Utc::now();
    let claims = TwoFactorChallenge {
        sub: user_id,
        role: role.to_string(),
        purpose: "2fa".into(),
        iat: now.timestamp(),
        exp: (now + Duration::seconds(TWOFA_CHALLENGE_TTL_SECS)).timestamp(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&cfg.jwt_secret),
    )
    .map_err(|_| AppError::Crypto("jwt encode failed"))
}

/// Verify a 2FA challenge token. Unauthorized for anything off, including an
/// access token presented in its place (wrong claim shape / purpose).
pub fn verify_2fa_challenge(cfg: &AuthConfig, token: &str) -> AppResult<TwoFactorChallenge> {
    let mut validation = Validation::default();
    validation.leeway = 60;
    let claims = decode::<TwoFactorChallenge>(
        token,
        &DecodingKey::from_secret(&cfg.jwt_secret),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(|_| AppError::Unauthorized)?;
    if claims.purpose != "2fa" {
        return Err(AppError::Unauthorized);
    }
    Ok(claims)
}

/// A freshly minted refresh token: the plaintext to hand the client, and the
/// hash to store.
#[derive(Debug, Clone)]
pub struct RefreshSecret {
    /// The opaque string the user gets in their cookie.
    pub plaintext: String,
    /// SHA-256 of the raw bytes, suitable for DB storage.
    pub hash: [u8; 32],
}

/// Generate a fresh 32-byte refresh token.
pub fn mint_refresh() -> RefreshSecret {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let plaintext = URL_SAFE_NO_PAD.encode(raw);
    let hash = sha256(&raw);
    RefreshSecret { plaintext, hash }
}

/// Hash a refresh-token plaintext to compare with what we stored.
pub fn hash_refresh(plaintext: &str) -> AppResult<[u8; 32]> {
    let raw = URL_SAFE_NO_PAD
        .decode(plaintext)
        .map_err(|_| AppError::Unauthorized)?;
    if raw.len() != 32 {
        return Err(AppError::Unauthorized);
    }
    Ok(sha256(&raw))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> AuthConfig {
        AuthConfig {
            jwt_secret: b"a-test-secret-that-is-long-enough-to-be-fine".to_vec(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
            argon2_m_cost_kib: 4096,
            argon2_t_cost: 1,
            argon2_p_cost: 1,
        }
    }

    #[test]
    fn access_round_trip() {
        let c = cfg();
        let uid = Uuid::now_v7();
        let sid = Uuid::now_v7();
        let token = mint_access(&c, uid, sid, "member").unwrap();
        let claims = verify_access(&c, &token).unwrap();
        assert_eq!(claims.sub, uid);
        assert_eq!(claims.sid, sid);
        assert_eq!(claims.role, "member");
    }

    #[test]
    fn tampered_token_fails() {
        let c = cfg();
        let token = mint_access(&c, Uuid::now_v7(), Uuid::now_v7(), "member").unwrap();
        let mut bytes = token.into_bytes();
        // Flip the last char of the signature.
        *bytes.last_mut().unwrap() ^= 0x01;
        let tampered = String::from_utf8(bytes).unwrap();
        assert!(matches!(
            verify_access(&c, &tampered),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn twofa_challenge_round_trips_and_rejects_access_tokens() {
        let c = cfg();
        let uid = Uuid::now_v7();
        let challenge = mint_2fa_challenge(&c, uid, "member").unwrap();
        let claims = verify_2fa_challenge(&c, &challenge).unwrap();
        assert_eq!(claims.sub, uid);
        assert_eq!(claims.purpose, "2fa");

        // An access token must not pass as a challenge (no `purpose` field).
        let access = mint_access(&c, uid, Uuid::now_v7(), "member").unwrap();
        assert!(matches!(
            verify_2fa_challenge(&c, &access),
            Err(AppError::Unauthorized)
        ));
        // …and the challenge must not pass as an access token (no sid).
        assert!(matches!(
            verify_access(&c, &challenge),
            Err(AppError::Unauthorized)
        ));
    }

    #[test]
    fn refresh_mint_hash_matches() {
        let RefreshSecret { plaintext, hash } = mint_refresh();
        let computed = hash_refresh(&plaintext).unwrap();
        assert_eq!(hash, computed);
    }

    #[test]
    fn refresh_hash_rejects_garbage() {
        assert!(matches!(
            hash_refresh("not-base64-url!!"),
            Err(AppError::Unauthorized)
        ));
        // Wrong length.
        let short = URL_SAFE_NO_PAD.encode([0u8; 8]);
        assert!(matches!(hash_refresh(&short), Err(AppError::Unauthorized)));
    }
}
