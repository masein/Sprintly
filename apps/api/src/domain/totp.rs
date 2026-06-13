//! TOTP (RFC 6238) + recovery codes for two-factor auth (F11). Pure crypto and
//! encoding — no DB. We use HMAC-SHA1 / 6 digits / 30s, the de-facto standard
//! authenticator apps expect (Google Authenticator ignores the algorithm
//! parameter and always uses SHA1, so SHA1 it is).

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;
use sha2::{Digest, Sha256};

const DIGITS: u32 = 6;
const PERIOD: u64 = 30;
const SECRET_LEN: usize = 20; // 160-bit, RFC 6238 recommendation for SHA1
const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// A fresh 160-bit TOTP secret.
pub fn generate_secret() -> [u8; SECRET_LEN] {
    let mut s = [0u8; SECRET_LEN];
    rand::thread_rng().fill_bytes(&mut s);
    s
}

/// RFC 4648 base32, no padding — what authenticator apps want in the URI.
pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    for chunk in data.chunks(5) {
        let mut buf = [0u8; 5];
        buf[..chunk.len()].copy_from_slice(chunk);
        let bits = u64::from_be_bytes([0, 0, 0, buf[0], buf[1], buf[2], buf[3], buf[4]]);
        // 5 input bytes → 8 base32 chars; emit only the chars covering real bits.
        let chars = (chunk.len() * 8).div_ceil(5);
        for i in 0..chars {
            let shift = 35 - 5 * i;
            out.push(BASE32[((bits >> shift) & 0x1f) as usize] as char);
        }
    }
    out
}

/// `otpauth://` URI for QR/manual entry. `issuer` and `account` are shown in
/// the user's authenticator app.
pub fn provisioning_uri(secret: &[u8], issuer: &str, account: &str) -> String {
    let label = urlencode(&format!("{issuer}:{account}"));
    let iss = urlencode(issuer);
    format!(
        "otpauth://totp/{label}?secret={}&issuer={iss}&algorithm=SHA1&digits={DIGITS}&period={PERIOD}",
        base32_encode(secret)
    )
}

/// HOTP value for a counter (RFC 4226 dynamic truncation).
fn hotp(secret: &[u8], counter: u64) -> u32 {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let bin = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);
    bin % 10u32.pow(DIGITS)
}

/// The 6-digit code for a unix timestamp.
pub fn code_at(secret: &[u8], unix_secs: u64) -> String {
    format!(
        "{:0width$}",
        hotp(secret, unix_secs / PERIOD),
        width = DIGITS as usize
    )
}

/// Verify `code` against `secret` at `now_unix`, tolerating `window` steps of
/// clock skew on each side (window = 1 → ±30s). Constant-time digit compare.
pub fn verify(secret: &[u8], code: &str, now_unix: u64, window: i64) -> bool {
    let code = code.trim();
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let step = (now_unix / PERIOD) as i64;
    use subtle::ConstantTimeEq;
    (-window..=window).any(|d| {
        let counter = (step + d).max(0) as u64;
        let candidate = format!("{:0width$}", hotp(secret, counter), width = DIGITS as usize);
        bool::from(candidate.as_bytes().ct_eq(code.as_bytes()))
    })
}

// ─── recovery codes ──────────────────────────────────────────────────────────

/// `n` single-use recovery codes, formatted `xxxxx-xxxxx` (lowercase base32).
pub fn generate_recovery_codes(n: usize) -> Vec<String> {
    let alpha = b"abcdefghijkmnpqrstuvwxyz23456789"; // no l/o/0/1 — easy to read
    let mut rng = rand::thread_rng();
    (0..n)
        .map(|_| {
            let pick = |rng: &mut rand::rngs::ThreadRng| {
                (0..5)
                    .map(|_| alpha[(rng.next_u32() % alpha.len() as u32) as usize] as char)
                    .collect::<String>()
            };
            format!("{}-{}", pick(&mut rng), pick(&mut rng))
        })
        .collect()
}

/// Canonical hash of a recovery code (sha256 hex of the normalised code), so
/// formatting/casing on input doesn't matter and we never store the plaintext.
pub fn hash_recovery_code(code: &str) -> String {
    let normalised: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    let mut h = Sha256::new();
    h.update(normalised.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

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
    fn base32_matches_rfc4648_vectors() {
        // RFC 4648 §10 test vectors (unpadded).
        assert_eq!(base32_encode(b""), "");
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_encode(b"fo"), "MZXQ");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b"foob"), "MZXW6YQ");
        assert_eq!(base32_encode(b"fooba"), "MZXW6YTB");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn totp_matches_rfc6238_sha1_vector() {
        // RFC 6238 Appendix B: secret "12345678901234567890" (ASCII), SHA1.
        let secret = b"12345678901234567890";
        // T = 59s → counter 1 → code 94287082.
        assert_eq!(code_at(secret, 59), "287082");
        // T = 1111111109 → 07081804.
        assert_eq!(code_at(secret, 1_111_111_109), "081804");
    }

    #[test]
    fn verify_tolerates_one_step_of_skew() {
        let secret = generate_secret();
        let now = 1_700_000_000u64;
        let code = code_at(&secret, now);
        assert!(verify(&secret, &code, now, 1));
        // One period earlier/later still accepted with window 1.
        assert!(verify(&secret, &code, now + PERIOD, 1));
        assert!(verify(&secret, &code, now.saturating_sub(PERIOD), 1));
        // Two periods out is rejected.
        assert!(!verify(&secret, &code, now + 2 * PERIOD, 1));
        // Garbage rejected.
        assert!(!verify(&secret, "000000", now, 1) || code == "000000");
        assert!(!verify(&secret, "12345", now, 1));
        assert!(!verify(&secret, "abcdef", now, 1));
    }

    #[test]
    fn recovery_codes_are_distinct_and_hash_is_format_insensitive() {
        let codes = generate_recovery_codes(10);
        assert_eq!(codes.len(), 10);
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert_eq!(unique.len(), 10, "codes are distinct");

        // Hashing ignores dashes/casing/whitespace.
        let c = &codes[0];
        assert_eq!(
            hash_recovery_code(c),
            hash_recovery_code(&c.to_uppercase().replace('-', " "))
        );
        assert_ne!(hash_recovery_code(&codes[0]), hash_recovery_code(&codes[1]));
    }
}
