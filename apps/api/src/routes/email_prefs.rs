//! Email delivery preferences, and the unsubscribe link every email carries.
//!
//!   GET   /users/me/email-prefs
//!   PATCH /users/me/email-prefs   { mode?, kinds?, digest_hour? }
//!   GET   /email/unsubscribe?token=…   — no session; that's the point
//!
//! Preferences live in their own table rather than `users.settings` because
//! `settings` is client-writable and the unsubscribe token must not be.

use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    domain::email_prefs::{self, Mode, KINDS},
    infra::AppState,
    middleware::CurrentUser,
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users/me/email-prefs", get(read).patch(update))
        .route("/email/unsubscribe", get(unsubscribe))
}

#[derive(Debug, Serialize)]
pub struct PrefsDto {
    pub mode: String,
    pub kinds: Value,
    pub digest_hour: i16,
    /// The kinds the server knows about, so the UI doesn't hardcode them.
    pub available_kinds: Vec<&'static str>,
    /// True when mail is only written to the API log — the operator hasn't
    /// configured SMTP. Worth saying out loud in the UI: otherwise these
    /// switches look broken rather than unplugged.
    pub delivery_configured: bool,
}

#[derive(Debug, Deserialize)]
struct UpdateReq {
    mode: Option<String>,
    /// Partial map — only the kinds named are changed.
    kinds: Option<Value>,
    digest_hour: Option<i16>,
}

async fn read(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    let p = email_prefs::load(&state.db, user.id).await?;
    Ok(Json(PrefsDto {
        mode: p.mode.as_str().to_string(),
        kinds: p.kinds,
        digest_hour: p.digest_hour,
        available_kinds: KINDS.to_vec(),
        delivery_configured: state.cfg.email.smtp_url.is_some(),
    }))
}

async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(body): Json<UpdateReq>,
) -> AppResult<impl IntoResponse> {
    let mode = match body.mode.as_deref() {
        Some(m) => Some(Mode::parse(m).ok_or_else(|| {
            AppError::BadRequest("mode must be off, immediate, or digest".into())
        })?),
        None => None,
    };

    if let Some(h) = body.digest_hour {
        if !(0..=23).contains(&h) {
            return Err(AppError::BadRequest("digest_hour must be 0–23".into()));
        }
    }

    // Only accept keys we know, and only booleans. An unknown kind would sit
    // in the blob forever meaning nothing.
    if let Some(k) = &body.kinds {
        let Some(map) = k.as_object() else {
            return Err(AppError::BadRequest("kinds must be an object".into()));
        };
        for (key, val) in map {
            if !KINDS.contains(&key.as_str()) {
                return Err(AppError::BadRequest(format!(
                    "unknown notification kind `{key}`"
                )));
            }
            if !val.is_boolean() {
                return Err(AppError::BadRequest(format!(
                    "`{key}` must be true or false"
                )));
            }
        }
    }

    // Make sure the row exists (and thus has an unsubscribe token) before
    // updating it — accounts predating this table have no row until now.
    email_prefs::ensure_row(&state.db, user.id).await?;

    let row = sqlx::query!(
        r#"
        UPDATE email_prefs
        SET    mode        = COALESCE($2, mode),
               -- Shallow-merge the kinds map, same contract as user settings:
               -- a client toggling one switch can't drop the others.
               kinds       = kinds || COALESCE($3, '{}'::jsonb),
               digest_hour = COALESCE($4, digest_hour),
               updated_at  = now()
        WHERE  user_id = $1
        RETURNING mode        AS "mode!: String",
                  kinds       AS "kinds!: Value",
                  digest_hour AS "digest_hour!: i16"
        "#,
        user.id,
        mode.map(|m| m.as_str()),
        body.kinds,
        body.digest_hour,
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(PrefsDto {
        mode: row.mode,
        kinds: row.kinds,
        digest_hour: row.digest_hour,
        available_kinds: KINDS.to_vec(),
        delivery_configured: state.cfg.email.smtp_url.is_some(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct UnsubQuery {
    pub token: String,
}

/// One-click unsubscribe. Deliberately unauthenticated — it's followed from an
/// email client, where there's no session — and deliberately one-way: it can
/// only ever set `off`. A leaked link can silence someone's mail (recoverable
/// in settings); it can never switch mail *on* for an address.
async fn unsubscribe(
    State(state): State<AppState>,
    Query(q): Query<UnsubQuery>,
) -> AppResult<impl IntoResponse> {
    let raw = decode_hex(&q.token)
        .ok_or_else(|| AppError::BadRequest("that unsubscribe link is malformed".into()))?;
    if raw.len() != 32 {
        return Err(AppError::BadRequest(
            "that unsubscribe link is malformed".into(),
        ));
    }

    // Same answer either way: a valid-looking token that matches nothing
    // shouldn't be distinguishable from one that does.
    let _ = email_prefs::unsubscribe(&state.db, &raw).await?;

    // This link is clicked from a mail client, so it answers with a page, not
    // JSON. Self-contained — an email reader has no reason to trust us with
    // asset requests, and half of them would block them anyway.
    let settings = format!("{}/settings", state.cfg.public_url.trim_end_matches('/'));
    Ok(Html(format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex">
<title>Unsubscribed · Sprintly</title>
<style>
  body {{ margin:0; min-height:100vh; display:grid; place-items:center;
         background:#0b0b10; color:#e7e7ea;
         font:14px/1.6 ui-monospace,SFMono-Regular,Menlo,monospace; }}
  main {{ max-width:34rem; padding:2rem; }}
  h1 {{ font-size:1.25rem; margin:0 0 .75rem; }}
  p {{ color:#a1a1aa; margin:.5rem 0; }}
  a {{ color:#7c5cff; }}
</style></head>
<body><main>
  <h1>Done. No more Sprintly email.</h1>
  <p>You'll still see everything in the app — the bell in the header keeps
     working. This only switched off the email copies.</p>
  <p>Changed your mind, or want just the important ones?
     <a href="{settings}">Email settings</a>.</p>
</main></body></html>"#
    )))
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || s.len() > 128 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::decode_hex;

    #[test]
    fn hex_decoding_rejects_junk() {
        assert_eq!(decode_hex("00ff"), Some(vec![0, 255]));
        assert_eq!(decode_hex("0"), None, "odd length");
        assert_eq!(decode_hex("zz"), None, "not hex");
        assert_eq!(decode_hex(&"a".repeat(200)), None, "absurdly long");
    }
}
