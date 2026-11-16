//! Postgres connection pool. Tuning kept boring; reach for pgbouncer later if
//! we need it.

use anyhow::Result;
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    PgPool,
};
use std::str::FromStr;
use std::time::Duration;

use crate::config::Config;

pub async fn connect(cfg: &Config) -> Result<PgPool> {
    connect_url(&cfg.database_url).await
}

/// Connect from a bare URL — used by the `migrate` subcommand, which doesn't
/// build the full `Config`.
pub async fn connect_url(database_url: &str) -> Result<PgPool> {
    let opts = PgConnectOptions::from_str(database_url)?.application_name("sprintly-api");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(300)))
        .test_before_acquire(true)
        .connect_with(opts)
        .await?;

    Ok(pool)
}

/// Split a `postgres://user:password@host/db` URL into the URL *without* its
/// password and the (percent-decoded) password, for running libpq tools
/// (`pg_dump`, `pg_restore`). Pass the first as the argument and the second
/// as `PGPASSWORD` in the child's environment: a password in argv is visible
/// to every process on the host via `ps` / `/proc/<pid>/cmdline`, while the
/// environment is readable only by the same user (and root).
///
/// URLs without a password (or that don't parse as `scheme://…@…`) come back
/// unchanged with `None`.
pub fn libpq_target(database_url: &str) -> (String, Option<String>) {
    let Some((scheme, rest)) = database_url.split_once("://") else {
        return (database_url.to_string(), None);
    };
    let auth_end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(auth_end);
    let Some((userinfo, hostport)) = authority.rsplit_once('@') else {
        return (database_url.to_string(), None);
    };
    let Some((user, password)) = userinfo.split_once(':') else {
        return (database_url.to_string(), None);
    };
    (
        format!("{scheme}://{user}@{hostport}{tail}"),
        Some(percent_decode(password)),
    )
}

/// Minimal %XX decoding for the userinfo part of a URL. Malformed escapes are
/// kept literally rather than rejected.
fn percent_decode(s: &str) -> String {
    fn hex(b: u8) -> Option<u8> {
        match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            b'A'..=b'F' => Some(b - b'A' + 10),
            _ => None,
        }
    }
    // Byte-wise on purpose: slicing the &str after a '%' could split a
    // multi-byte character and panic.
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_is_split_out_of_the_url() {
        let (url, pw) = libpq_target("postgres://sprintly:s3cr3t@postgres:5432/sprintly");
        assert_eq!(url, "postgres://sprintly@postgres:5432/sprintly");
        assert_eq!(pw.as_deref(), Some("s3cr3t"));
    }

    #[test]
    fn percent_encoded_passwords_are_decoded() {
        let (url, pw) = libpq_target("postgresql://u:p%40ss%2Fw%3Ad@db/x?sslmode=require");
        assert_eq!(url, "postgresql://u@db/x?sslmode=require");
        assert_eq!(pw.as_deref(), Some("p@ss/w:d"));
    }

    #[test]
    fn urls_without_a_password_pass_through() {
        for u in ["postgres://u@db/x", "postgres://db/x", "not a url"] {
            assert_eq!(libpq_target(u), (u.to_string(), None), "{u}");
        }
    }

    #[test]
    fn malformed_escapes_are_kept_literally() {
        assert_eq!(percent_decode("a%zzb%4"), "a%zzb%4");
        assert_eq!(percent_decode("%41%42"), "AB");
        // A '%' before a multi-byte character must not panic.
        assert_eq!(percent_decode("x%é%"), "x%é%");
    }
}
