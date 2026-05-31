//! Typed configuration loaded from environment variables.
//!
//! Anything that can be missing or invalid should fail loudly at boot, not at
//! the first request that needs it. If you add a new env var, add it here.

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

impl Config {
    pub fn from_env() -> Result<Self> {
        let env = match std::env::var("SPRINTLY_ENV").as_deref() {
            Ok("prod") => Environment::Prod,
            _ => Environment::Dev,
        };

        let api_bind: SocketAddr = required("SPRINTLY_API_BIND")?
            .parse()
            .context("SPRINTLY_API_BIND must be host:port")?;

        let jwt_secret = decode_base64("SPRINTLY_JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            return Err(anyhow!(
                "SPRINTLY_JWT_SECRET must decode to at least 32 bytes"
            ));
        }

        let master = decode_base64("SPRINTLY_VAULT_MASTER_KEY")?;
        if master.len() != 32 {
            return Err(anyhow!(
                "SPRINTLY_VAULT_MASTER_KEY must decode to exactly 32 bytes"
            ));
        }
        let mut master_key = [0u8; 32];
        master_key.copy_from_slice(&master);

        Ok(Self {
            env,
            public_url: required("SPRINTLY_PUBLIC_URL")?,
            api_bind,
            open_signup: optional("SPRINTLY_OPEN_SIGNUP")
                .map(|v| v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),

            database_url: required("DATABASE_URL")?,
            redis_url: required("REDIS_URL")?,

            minio: MinioConfig {
                endpoint: required("MINIO_ENDPOINT")?,
                public_endpoint: required("MINIO_PUBLIC_ENDPOINT")?,
                access_key: required("MINIO_ROOT_USER")?,
                secret_key: required("MINIO_ROOT_PASSWORD")?,
                bucket: required("MINIO_BUCKET")?,
                region: optional("MINIO_REGION").unwrap_or_else(|| "us-east-1".into()),
            },

            auth: AuthConfig {
                jwt_secret,
                access_ttl_secs: required_parse("SPRINTLY_ACCESS_TTL_SECS")?,
                refresh_ttl_secs: required_parse("SPRINTLY_REFRESH_TTL_SECS")?,
                argon2_m_cost_kib: required_parse("SPRINTLY_ARGON2_M_COST_KIB")?,
                argon2_t_cost: required_parse("SPRINTLY_ARGON2_T_COST")?,
                argon2_p_cost: required_parse("SPRINTLY_ARGON2_P_COST")?,
            },

            vault: VaultConfig {
                master_key,
                key_version: required_parse("SPRINTLY_VAULT_KEY_VERSION")?,
            },
        })
    }

    pub fn is_dev(&self) -> bool {
        self.env == Environment::Dev
    }
}

fn required(name: &str) -> Result<String> {
    std::env::var(name).map_err(|_| anyhow!("missing required env var: {name}"))
}

fn optional(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.is_empty())
}

fn required_parse<T: std::str::FromStr>(name: &str) -> Result<T>
where
    T::Err: std::fmt::Display,
{
    required(name)?
        .parse()
        .map_err(|e| anyhow!("env {name} failed to parse: {e}"))
}

fn decode_base64(name: &str) -> Result<Vec<u8>> {
    let raw = required(name)?;
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .with_context(|| format!("env {name} is not valid base64"))
}
