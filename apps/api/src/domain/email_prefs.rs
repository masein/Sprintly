//! Per-user email preferences, and the one decision everything else hangs off:
//! *should this notification become an email right now?*
//!
//! The rules, in one place because they're the part people will argue about:
//!
//!   * `mode = off`       → never. The unsubscribe link sets this.
//!   * `mode = immediate` → yes, if the kind is enabled.
//!   * `mode = digest`    → not now; the daily digest job picks it up.
//!
//! `kinds` gates *what* is eligible in either sending mode, so "digest, but
//! only mentions" is expressible. A kind missing from the map counts as
//! disabled — new notification kinds don't start mailing people the day they
//! ship, they wait to be turned on.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Off,
    Immediate,
    Digest,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Immediate => "immediate",
            Mode::Digest => "digest",
        }
    }

    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s {
            "off" => Mode::Off,
            "immediate" => Mode::Immediate,
            "digest" => Mode::Digest,
            _ => return None,
        })
    }
}

/// The notification kinds that can become email. Mirrors the
/// `notifications_kind_check` constraint — a kind the DB won't store is a kind
/// we can't be asked to mail.
pub const KINDS: [&str; 3] = ["mention", "assigned", "comment"];

#[derive(Debug, Clone)]
pub struct Prefs {
    pub mode: Mode,
    pub kinds: Value,
    pub digest_hour: i16,
}

impl Default for Prefs {
    /// Matches the column defaults: things aimed at you personally mail
    /// immediately, comments wait to be asked for.
    fn default() -> Self {
        Self {
            mode: Mode::Immediate,
            kinds: serde_json::json!({ "mention": true, "assigned": true, "comment": false }),
            digest_hour: 8,
        }
    }
}

impl Prefs {
    /// Is this kind enabled at all? Absent means no.
    pub fn kind_enabled(&self, kind: &str) -> bool {
        self.kinds
            .get(kind)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    /// Should a notification of `kind` be emailed *now*?
    pub fn wants_immediate(&self, kind: &str) -> bool {
        self.mode == Mode::Immediate && self.kind_enabled(kind)
    }

    /// Should it wait for the daily digest instead?
    pub fn wants_digest(&self, kind: &str) -> bool {
        self.mode == Mode::Digest && self.kind_enabled(kind)
    }
}

/// Read a user's prefs, falling back to the defaults when no row exists yet.
/// A missing row is normal for an account created before this table existed and
/// never touched since — the migration backfills, but a fallback beats a 500.
pub async fn load(db: &PgPool, user_id: Uuid) -> AppResult<Prefs> {
    let row = sqlx::query!(
        r#"
        SELECT mode        AS "mode!: String",
               kinds       AS "kinds!: Value",
               digest_hour AS "digest_hour!: i16"
        FROM   email_prefs
        WHERE  user_id = $1
        "#,
        user_id
    )
    .fetch_optional(db)
    .await?;

    Ok(match row {
        Some(r) => Prefs {
            mode: Mode::parse(&r.mode).unwrap_or(Mode::Immediate),
            kinds: r.kinds,
            digest_hour: r.digest_hour,
        },
        None => Prefs::default(),
    })
}

/// Make sure a row exists and hand back its unsubscribe token. Called before
/// sending, because every email we send has to carry a working opt-out.
pub async fn ensure_row(db: &PgPool, user_id: Uuid) -> AppResult<Vec<u8>> {
    let token: Vec<u8> = sqlx::query_scalar!(
        r#"
        INSERT INTO email_prefs (user_id, unsubscribe_token)
        VALUES ($1, gen_random_bytes(32))
        ON CONFLICT (user_id) DO UPDATE SET user_id = email_prefs.user_id
        RETURNING unsubscribe_token AS "unsubscribe_token!: Vec<u8>"
        "#,
        user_id
    )
    .fetch_one(db)
    .await?;
    Ok(token)
}

/// Turn mail off for whoever holds this token. Returns false when the token
/// doesn't match anything.
///
/// This only ever sets `off`. A leaked or guessed link can silence someone's
/// mail — annoying but recoverable in the settings page — and can never be
/// used to switch mail *on* for an address, which is the direction that would
/// make us a spam vector.
pub async fn unsubscribe(db: &PgPool, token: &[u8]) -> AppResult<bool> {
    let done = sqlx::query!(
        r#"
        UPDATE email_prefs
        SET    mode = 'off', updated_at = now()
        WHERE  unsubscribe_token = $1
        "#,
        token
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected() > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mail_the_personal_kinds_and_not_comments() {
        let p = Prefs::default();
        assert!(p.wants_immediate("mention"));
        assert!(p.wants_immediate("assigned"));
        assert!(
            !p.wants_immediate("comment"),
            "comments are the chatty kind; they should be opt-in"
        );
    }

    #[test]
    fn off_means_off_for_everything() {
        let p = Prefs {
            mode: Mode::Off,
            kinds: serde_json::json!({ "mention": true, "assigned": true, "comment": true }),
            ..Prefs::default()
        };
        for k in KINDS {
            assert!(!p.wants_immediate(k), "{k} still wanted mail");
            assert!(!p.wants_digest(k), "{k} still wanted a digest");
        }
    }

    #[test]
    fn digest_mode_defers_rather_than_declines() {
        let p = Prefs {
            mode: Mode::Digest,
            ..Prefs::default()
        };
        assert!(!p.wants_immediate("mention"), "digest shouldn't send now");
        assert!(p.wants_digest("mention"), "…but should send later");
        // Still gated by `kinds`: digest doesn't override an off switch.
        assert!(!p.wants_digest("comment"));
    }

    #[test]
    fn an_unknown_kind_is_off_not_on() {
        let p = Prefs::default();
        // A kind we ship next year mustn't start mailing people on deploy day.
        assert!(!p.wants_immediate("sprint_completed"));
        assert!(!p.wants_digest("sprint_completed"));
    }

    #[test]
    fn mode_round_trips_through_its_string_form() {
        for m in [Mode::Off, Mode::Immediate, Mode::Digest] {
            assert_eq!(Mode::parse(m.as_str()), Some(m));
        }
        assert_eq!(Mode::parse("sometimes"), None);
    }
}
