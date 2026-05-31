//! Password hashing.
//!
//! Argon2id with the parameters configured at boot. The `verify` path is
//! constant-time courtesy of the `argon2` crate. We deliberately do NOT
//! return distinct error variants for "no such user" vs "wrong password" —
//! the route layer collapses both into `AppError::Unauthorized`.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};

use crate::{config::AuthConfig, AppError, AppResult};

/// Build an Argon2id hasher with our configured cost.
fn hasher(cfg: &AuthConfig) -> AppResult<Argon2<'static>> {
    let params = Params::new(
        cfg.argon2_m_cost_kib,
        cfg.argon2_t_cost,
        cfg.argon2_p_cost,
        None,
    )
    .map_err(|_| AppError::Crypto("invalid argon2 params"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hash a plaintext password. The returned string includes the algorithm
/// identifier, parameters, salt, and hash — store it as-is.
pub fn hash(cfg: &AuthConfig, password: &str) -> AppResult<String> {
    let salt = SaltString::generate(&mut OsRng);
    let h = hasher(cfg)?
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| AppError::Crypto("argon2 hash failed"))?;
    Ok(h.to_string())
}

/// Verify a plaintext against a stored hash. Returns Ok(true) on match,
/// Ok(false) on mismatch, Err for malformed hashes.
pub fn verify(stored: &str, password: &str) -> AppResult<bool> {
    let parsed =
        PasswordHash::new(stored).map_err(|_| AppError::Crypto("malformed password hash"))?;
    // Argon2 default is fine for *verify* — the params live in the hash.
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cheap_cfg() -> AuthConfig {
        AuthConfig {
            jwt_secret: vec![0u8; 32],
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
            // Fast params for tests. Real config sits much higher.
            argon2_m_cost_kib: 4096,
            argon2_t_cost: 1,
            argon2_p_cost: 1,
        }
    }

    #[test]
    fn hash_then_verify_succeeds() {
        let cfg = cheap_cfg();
        let h = hash(&cfg, "hunter2-correct-horse").unwrap();
        assert!(verify(&h, "hunter2-correct-horse").unwrap());
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let cfg = cheap_cfg();
        let h = hash(&cfg, "hunter2").unwrap();
        assert!(!verify(&h, "hunter3").unwrap());
    }

    #[test]
    fn malformed_hash_errors() {
        let err = verify("not-a-real-phc-string", "x").unwrap_err();
        matches!(err, AppError::Crypto(_));
    }
}
