//! Typed configuration loaded from environment variables.
//!
//! Anything that can be missing or invalid should fail loudly at boot, not at
//! the first request that needs it. If you add a new env var, add it here.
//!
//! Failures name the offending variable. `main` prints them to stderr before
//! the tracing subscriber is up, and `sprintly-api check-config` validates the
//! environment and prints a redacted summary.

use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};
use base64::Engine as _;

#[derive(Debug, Clone)]
pub struct Config {
    pub env: Environment,
    pub public_url: String,
    pub api_bind: SocketAddr,
    pub open_signup: bool,

    pub database_url: String,
    pub redis_url: String,

    pub minio: MinioConfig,
    pub auth: AuthConfig,
    pub vault: VaultConfig,
    pub email: EmailConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Dev,
    Prod,
}

impl std::fmt::Display for Environment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dev => f.write_str("dev"),
            Self::Prod => f.write_str("prod"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MinioConfig {
    pub endpoint: String,
    pub public_endpoint: String,
    pub access_key: String,
    pub secret_key: String,
    pub bucket: String,
    pub region: String,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Decoded JWT signing secret (>= 32 bytes).
    pub jwt_secret: Vec<u8>,
    pub access_ttl_secs: u64,
    pub refresh_ttl_secs: u64,
    pub argon2_m_cost_kib: u32,
    pub argon2_t_cost: u32,
    pub argon2_p_cost: u32,
}

#[derive(Debug, Clone)]
pub struct VaultConfig {
    /// 32-byte master key.
    pub master_key: [u8; 32],
    pub key_version: i32,
}

#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// SMTP connection URL (e.g. `smtps://user:pass@host:465`). When unset, the
    /// app logs outbound mail instead of sending it.
    pub smtp_url: Option<String>,
    /// `From` header, e.g. `Sprintly <noreply@example.com>`.
    pub mail_from: String,
}

impl Config {
    /// Load config from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|k| std::env::var(k).ok())
    }

    /// Load config from an arbitrary lookup. `get(name)` returns `Some(value)`
    /// when the variable is set (even if empty) and `None` when unset. Lets us
    /// unit-test parsing without touching the process environment.
    pub fn from_lookup<F>(get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let env = match get("SPRINTLY_ENV").as_deref() {
            Some("prod") => Environment::Prod,
            _ => Environment::Dev,
        };

        let api_bind: SocketAddr = required(&get, "SPRINTLY_API_BIND")?
            .parse()
            .context("SPRINTLY_API_BIND must be host:port")?;

        let jwt_secret = decode_base64(&get, "SPRINTLY_JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            return Err(anyhow!(
                "SPRINTLY_JWT_SECRET must decode to at least 32 bytes (got {})",
                jwt_secret.len()
            ));
        }

        let master = decode_base64(&get, "SPRINTLY_VAULT_MASTER_KEY")?;
        if master.len() != 32 {
            return Err(anyhow!(
                "SPRINTLY_VAULT_MASTER_KEY must decode to exactly 32 bytes (got {})",
                master.len()
            ));
        }
        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&master);

        Ok(Self {
            env,
            public_url: required(&get, "SPRINTLY_PUBLIC_URL")?,
            api_bind,
            open_signup: optional(&get, "SPRINTLY_OPEN_SIGNUP")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),

            database_url: required(&get, "DATABASE_URL")?,
            redis_url: required(&get, "REDIS_URL")?,

            minio: MinioConfig {
                endpoint: required(&get, "MINIO_ENDPOINT")?,
                public_endpoint: required(&get, "MINIO_PUBLIC_ENDPOINT")?,
                access_key: required(&get, "MINIO_ROOT_USER")?,
                secret_key: required(&get, "MINIO_ROOT_PASSWORD")?,
                bucket: required(&get, "MINIO_BUCKET")?,
                region: optional(&get, "MINIO_REGION").unwrap_or_else(|| "us-east-1".into()),
            },

            auth: AuthConfig {
                jwt_secret,
                access_ttl_secs: required_parse(&get, "SPRINTLY_ACCESS_TTL_SECS")?,
                refresh_ttl_secs: required_parse(&get, "SPRINTLY_REFRESH_TTL_SECS")?,
                argon2_m_cost_kib: required_parse(&get, "SPRINTLY_ARGON2_M_COST_KIB")?,
                argon2_t_cost: required_parse(&get, "SPRINTLY_ARGON2_T_COST")?,
                argon2_p_cost: required_parse(&get, "SPRINTLY_ARGON2_P_COST")?,
            },

            vault: VaultConfig {
                master_key,
                key_version: required_parse(&get, "SPRINTLY_VAULT_KEY_VERSION")?,
            },

            email: EmailConfig {
                smtp_url: optional(&get, "SPRINTLY_SMTP_URL"),
                mail_from: optional(&get, "SPRINTLY_MAIL_FROM")
                    .unwrap_or_else(|| "Sprintly <noreply@localhost>".into()),
            },
        })
    }

    pub fn is_dev(&self) -> bool {
        self.env == Environment::Dev
    }

    /// A summary safe to print — lengths and non-secret fields only, with URL
    /// credentials masked. Used by `check-config`.
    pub fn redacted_summary(&self) -> String {
        [
            format!("env              = {}", self.env),
            format!("public_url       = {}", self.public_url),
            format!("api_bind         = {}", self.api_bind),
            format!("open_signup      = {}", self.open_signup),
            format!("jwt_secret_bytes = {}", self.auth.jwt_secret.len()),
            format!("vault_key_bytes  = 32 (version {})", self.vault.key_version),
            format!("database_url     = {}", mask_url(&self.database_url)),
            format!("redis_url        = {}", mask_url(&self.redis_url)),
            format!("minio_endpoint   = {}", self.minio.endpoint),
            format!(
                "minio_bucket     = {} (region {})",
                self.minio.bucket, self.minio.region
            ),
            format!(
                "email            = {} (from {})",
                if self.email.smtp_url.is_some() {
                    "smtp"
                } else {
                    "log-only"
                },
                self.email.mail_from
            ),
        ]
        .join("\n")
    }
}

fn required<F>(get: &F, name: &str) -> Result<String>
where
    F: Fn(&str) -> Option<String>,
{
    get(name).ok_or_else(|| anyhow!("missing required env var: {name}"))
}

fn optional<F>(get: &F, name: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    get(name).filter(|s| !s.is_empty())
}

fn required_parse<F, T>(get: &F, name: &str) -> Result<T>
where
    F: Fn(&str) -> Option<String>,
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    required(get, name)?
        .parse()
        .map_err(|e| anyhow!("env {name} failed to parse: {e}"))
}

fn decode_base64<F>(get: &F, name: &str) -> Result<Vec<u8>>
where
    F: Fn(&str) -> Option<String>,
{
    decode_base64_value(name, &required(get, name)?)
}

/// Decode a base64 secret, tolerating embedded ASCII whitespace (a wrapped or
/// newline-padded paste). Names the offending var on failure.
fn decode_base64_value(name: &str, raw: &str) -> Result<Vec<u8>> {
    let cleaned: String = raw.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .with_context(|| format!("env {name}: expected base64, but it has invalid characters"))
}

/// Mask the password in a URL's userinfo (`scheme://user:pass@host` →
/// `scheme://user:***@host`). Best-effort; returns "(set)" if it doesn't parse.
fn mask_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "(set)".into();
    };
    match rest.split_once('/') {
        Some((authority, tail)) => format!("{scheme}://{}/{tail}", mask_authority(authority)),
        None => format!("{scheme}://{}", mask_authority(rest)),
    }
}

fn mask_authority(authority: &str) -> String {
    match authority.split_once('@') {
        Some((userinfo, host)) => {
            let user = userinfo.split_once(':').map_or(userinfo, |(u, _)| u);
            format!("{user}:***@{host}")
        }
        None => authority.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn valid_env() -> HashMap<String, String> {
        let pairs = [
            ("SPRINTLY_API_BIND", "127.0.0.1:8081"),
            ("SPRINTLY_PUBLIC_URL", "http://localhost:8080"),
            // 64 'a' chars → 48 decoded bytes (>= 32).
            ("SPRINTLY_JWT_SECRET", &"a".repeat(64)),
            // base64 of 32 ASCII zeros → exactly 32 bytes.
            (
                "SPRINTLY_VAULT_MASTER_KEY",
                "MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA=",
            ),
            ("SPRINTLY_VAULT_KEY_VERSION", "1"),
            ("DATABASE_URL", "postgres://u:p@db:5432/s"),
            ("REDIS_URL", "redis://r:6379/0"),
            ("MINIO_ENDPOINT", "http://minio:9000"),
            ("MINIO_PUBLIC_ENDPOINT", "http://localhost:9000"),
            ("MINIO_ROOT_USER", "x"),
            ("MINIO_ROOT_PASSWORD", "y"),
            ("MINIO_BUCKET", "b"),
            ("SPRINTLY_ACCESS_TTL_SECS", "900"),
            ("SPRINTLY_REFRESH_TTL_SECS", "2592000"),
            ("SPRINTLY_ARGON2_M_COST_KIB", "4096"),
            ("SPRINTLY_ARGON2_T_COST", "1"),
            ("SPRINTLY_ARGON2_P_COST", "1"),
        ];
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn cfg_from(m: &HashMap<String, String>) -> Result<Config> {
        Config::from_lookup(|k| m.get(k).cloned())
    }

    #[test]
    fn valid_env_parses() {
        assert!(cfg_from(&valid_env()).is_ok());
    }

    #[test]
    fn whitespace_in_secret_is_tolerated() {
        let mut m = valid_env();
        // Wrapped + space-padded base64 of the same 48 bytes.
        m.insert(
            "SPRINTLY_JWT_SECRET".into(),
            format!("  {}\n {} \n", "a".repeat(32), "a".repeat(32)),
        );
        assert!(
            cfg_from(&m).is_ok(),
            "wrapped/padded base64 should still parse"
        );
    }

    #[test]
    fn malformed_secret_names_the_var() {
        let mut m = valid_env();
        m.insert("SPRINTLY_JWT_SECRET".into(), "not valid base64 %%%".into());
        let e = cfg_from(&m).unwrap_err();
        assert!(format!("{e:#}").contains("SPRINTLY_JWT_SECRET"));
    }

    #[test]
    fn short_jwt_secret_is_named() {
        let mut m = valid_env();
        m.insert("SPRINTLY_JWT_SECRET".into(), "YWJj".into()); // "abc" = 3 bytes
        let e = cfg_from(&m).unwrap_err();
        assert!(format!("{e:#}").contains("SPRINTLY_JWT_SECRET"));
    }

    #[test]
    fn wrong_vault_key_length_is_named() {
        let mut m = valid_env();
        m.insert("SPRINTLY_VAULT_MASTER_KEY".into(), "YWJj".into()); // 3 bytes, not 32
        let e = cfg_from(&m).unwrap_err();
        assert!(format!("{e:#}").contains("SPRINTLY_VAULT_MASTER_KEY"));
    }

    #[test]
    fn missing_required_var_is_named() {
        let mut m = valid_env();
        m.remove("DATABASE_URL");
        let e = cfg_from(&m).unwrap_err();
        assert!(format!("{e:#}").contains("DATABASE_URL"));
    }

    #[test]
    fn mask_url_hides_password() {
        assert_eq!(
            mask_url("postgres://user:secret@host:5432/db"),
            "postgres://user:***@host:5432/db"
        );
        assert!(!mask_url("redis://u:topsecret@r:6379/0").contains("topsecret"));
    }
}
