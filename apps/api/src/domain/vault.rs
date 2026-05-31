//! Vault crypto.
//!
//!   master key (env)
//!         │
//!         ▼   HKDF-SHA256
//!         │   salt   = project_id (16 bytes)
//!         │   info   = b"sprintly-vault-v1"
//!         ▼   okm    = 32 bytes
//!   per-project key
//!         │
//!         ▼   XChaCha20-Poly1305
//!         │   nonce  = 24 random bytes per write (NEVER reused)
//!         ▼
//!   ciphertext (BYTEA on disk)
//!
//! Properties we rely on:
//!   * 24-byte nonce → birthday collision becomes a non-event at our scale.
//!   * AEAD authentication tag means any tamper to ciphertext, nonce, AAD,
//!     or key fails the decrypt loud — caller gets `AppError::Crypto`.
//!   * `key_version` is recorded per row so we can rotate the master and
//!     re-encrypt lazily without breaking already-stored items.
//!
//! What this module DOES NOT do:
//!   * Audit logging — that's the route layer's job.
//!   * Permission checks — domain layer is HTTP-blind.
//!   * Plaintext logging or tracing. The only places plaintext lives are
//!     the caller's stack and the response body.

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng, Payload},
    XChaCha20Poly1305, XNonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;
use uuid::Uuid;

use crate::{AppError, AppResult};

/// 32-byte key derived for a specific project from the master.
pub struct ProjectKey([u8; 32]);

impl ProjectKey {
    /// HKDF-SHA256(master, salt=project_id, info="sprintly-vault-v{version}").
    /// The version is baked into `info` so future rotations get a distinct
    /// derivation namespace — same master + same project + same version =
    /// same key, deterministically.
    pub fn derive(master: &[u8; 32], project_id: Uuid, key_version: i32) -> Self {
        let info = format!("sprintly-vault-v{key_version}");
        let salt = project_id.as_bytes();
        let hk = Hkdf::<Sha256>::new(Some(salt), master);
        let mut okm = [0u8; 32];
        hk.expand(info.as_bytes(), &mut okm)
            .expect("32-byte output fits HKDF-SHA256 OKM bound");
        Self(okm)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(self.0.as_ref().into())
    }
}

impl Drop for ProjectKey {
    /// Best-effort zeroize. We could pull in `zeroize` for a more rigorous
    /// volatile-write guarantee; for v1 this prevents key bytes from
    /// lingering after the struct drops in most release builds.
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            *b = 0;
        }
    }
}

/// AEAD-encrypt `plaintext` under `key`. The 24-byte nonce is sampled fresh
/// every call via OsRng. The optional `aad` (e.g. vault_item.id) is bound
/// into the authentication tag — flipping it later fails decrypt.
pub fn encrypt(key: &ProjectKey, plaintext: &[u8], aad: &[u8]) -> AppResult<(Vec<u8>, [u8; 24])> {
    let mut nonce_bytes = [0u8; 24];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let ciphertext = key
        .cipher()
        .encrypt(nonce, Payload { msg: plaintext, aad })
        .map_err(|_| AppError::Crypto("vault encrypt failed"))?;
    Ok((ciphertext, nonce_bytes))
}

/// AEAD-decrypt under `key`. Any tamper to ciphertext, nonce, or aad fails.
pub fn decrypt(
    key: &ProjectKey,
    ciphertext: &[u8],
    nonce: &[u8],
    aad: &[u8],
) -> AppResult<Vec<u8>> {
    if nonce.len() != 24 {
        return Err(AppError::Crypto("nonce length must be 24"));
    }
    let nonce = XNonce::from_slice(nonce);
    key.cipher()
        .decrypt(nonce, Payload { msg: ciphertext, aad })
        .map_err(|_| AppError::Crypto("vault decrypt failed (key, nonce, or ciphertext invalid)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn master() -> [u8; 32] {
        let mut m = [0u8; 32];
        for (i, b) in m.iter_mut().enumerate() {
            *b = i as u8;
        }
        m
    }

    #[test]
    fn round_trip_recovers_plaintext() {
        let key = ProjectKey::derive(&master(), Uuid::now_v7(), 1);
        let aad = b"item:abc";
        let pt = b"correct horse battery staple";
        let (ct, n) = encrypt(&key, pt, aad).unwrap();
        let got = decrypt(&key, &ct, &n, aad).unwrap();
        assert_eq!(got, pt);
    }

    #[test]
    fn different_project_keys_dont_decrypt() {
        let m = master();
        let p1 = ProjectKey::derive(&m, Uuid::now_v7(), 1);
        let p2 = ProjectKey::derive(&m, Uuid::now_v7(), 1);
        let aad = b"x";
        let (ct, n) = encrypt(&p1, b"hello", aad).unwrap();
        let err = decrypt(&p2, &ct, &n, aad).err().unwrap();
        assert!(matches!(err, AppError::Crypto(_)));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = ProjectKey::derive(&master(), Uuid::now_v7(), 1);
        let aad = b"x";
        let (mut ct, n) = encrypt(&key, b"hi", aad).unwrap();
        let last = ct.len() - 1;
        ct[last] ^= 0x01;
        let err = decrypt(&key, &ct, &n, aad).err().unwrap();
        assert!(matches!(err, AppError::Crypto(_)));
    }

    #[test]
    fn tampered_nonce_fails() {
        let key = ProjectKey::derive(&master(), Uuid::now_v7(), 1);
        let aad = b"x";
        let (ct, mut n) = encrypt(&key, b"hi", aad).unwrap();
        n[0] ^= 0x01;
        let err = decrypt(&key, &ct, &n, aad).err().unwrap();
        assert!(matches!(err, AppError::Crypto(_)));
    }

    #[test]
    fn changed_aad_fails() {
        let key = ProjectKey::derive(&master(), Uuid::now_v7(), 1);
        let (ct, n) = encrypt(&key, b"hi", b"correct").unwrap();
        let err = decrypt(&key, &ct, &n, b"wrong").err().unwrap();
        assert!(matches!(err, AppError::Crypto(_)));
    }

    #[test]
    fn wrong_nonce_length_rejected_without_aead_call() {
        let key = ProjectKey::derive(&master(), Uuid::now_v7(), 1);
        let err = decrypt(&key, &[0u8; 32], &[0u8; 12], b"x").err().unwrap();
        assert!(matches!(err, AppError::Crypto(_)));
    }

    #[test]
    fn key_version_changes_derivation() {
        let m = master();
        let p = Uuid::now_v7();
        let k_v1 = ProjectKey::derive(&m, p, 1);
        let k_v2 = ProjectKey::derive(&m, p, 2);
        assert_ne!(k_v1.0, k_v2.0, "different versions → different keys");
    }
}
