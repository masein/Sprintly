//! Auth endpoints.
//!
//!   POST /auth/register
//!   POST /auth/login
//!   POST /auth/logout
//!   POST /auth/refresh
//!   POST /auth/password/reset/request
//!   POST /auth/password/reset/confirm
//!
//! Cookies in play:
//!   sprintly_access  — JWT, HttpOnly, SameSite=Lax, ~15min TTL.
//!   sprintly_refresh — opaque, HttpOnly, SameSite=Lax, ~30d TTL, path=/api/v1/auth.
//!
//! Both are marked Secure when SPRINTLY_PUBLIC_URL starts with https://.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use base64::Engine as _;
use chrono::{Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::{
    config::Config,
    domain::{password, sessions, tokens},
    infra::AppState,
    middleware::{client_ip, csrf, rate_limit, CurrentUser},
    AppError, AppResult,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/refresh", post(refresh))
        .route("/auth/password/reset/request", post(password_reset_request))
        .route("/auth/password/reset/confirm", post(password_reset_confirm))
}

// ─── DTOs ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterReq {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 3, max = 32))]
    pub handle: String,
    #[validate(length(min = 1, max = 80))]
    pub display_name: String,
    #[validate(length(min = 10, max = 200))]
    pub password: String,
    /// Required after the first user, unless SPRINTLY_OPEN_SIGNUP=true.
    pub invite_token: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginReq {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1))]
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub user: UserDto,
}

#[derive(Debug, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub handle: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Deserialize)]
pub struct PasswordResetRequestReq {
    pub email: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct PasswordResetConfirmReq {
    pub token: String,
    #[validate(length(min = 10, max = 200))]
    pub new_password: String,
}

// ─── handlers ───────────────────────────────────────────────────────────────

async fn register(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<RegisterReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    // Decide whether registration is allowed for THIS request.
    //   1. If there are no users yet, anyone can register and becomes admin.
    //   2. Else, if SPRINTLY_OPEN_SIGNUP=true, anyone can register as member.
    //   3. Else, an invite token is required.
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE deleted_at IS NULL")
        .fetch_one(&state.db)
        .await?;

    let (assigned_role, consumed_invite_id): (String, Option<Uuid>) = if user_count == 0 {
        ("admin".into(), None)
    } else if state.cfg.open_signup {
        ("member".into(), None)
    } else {
        let Some(token) = req.invite_token.as_deref() else {
            return Err(AppError::Forbidden);
        };
        let (role, invite_id) = consume_invite(&state.db, token).await?;
        (role, Some(invite_id))
    };

    // Hash password.
    let hash = password::hash(&state.cfg.auth, &req.password)?;
    let user_id = Uuid::now_v7();

    // Insert user. Friendly conflict errors for duplicate email/handle.
    let insert = sqlx::query(
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(user_id)
    .bind(&req.email)
    .bind(&req.handle)
    .bind(&req.display_name)
    .bind(&hash)
    .bind(&assigned_role)
    .execute(&state.db)
    .await;

    if let Err(sqlx::Error::Database(db_err)) = &insert {
        if db_err.is_unique_violation() {
            return Err(AppError::Conflict(
                "that email or handle is already taken".into(),
            ));
        }
    }
    insert?;

    // If we consumed an invite, stamp it. Done outside the user-insert tx for
    // simplicity — a failed stamp doesn't undo registration, but the invite
    // is marked consumed atomically.
    if let Some(invite_id) = consumed_invite_id {
        sqlx::query(
            r#"
            UPDATE invite_tokens
               SET consumed_by = $1, consumed_at = now()
             WHERE id = $2 AND consumed_at IS NULL
            "#,
        )
        .bind(user_id)
        .bind(invite_id)
        .execute(&state.db)
        .await?;
    }

    // Auto-login: mint a session immediately so the UX is "register → in".
    issue_session_response(&state, user_id, &assigned_role, &headers).await
}

async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    info: ConnectInfo<SocketAddr>,
    Json(req): Json<LoginReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    // Throttle credential guessing — by source IP and by target email.
    let ip = client_ip(&headers, info);
    rate_limit::hit(
        &state,
        &format!("sprintly:rl:login:ip:{ip}"),
        rate_limit::login_ip_per_min(),
        60,
    )
    .await?;
    rate_limit::hit(
        &state,
        &format!("sprintly:rl:login:email:{}", req.email.to_lowercase()),
        rate_limit::login_email_per_min(),
        60,
    )
    .await?;

    let row = sqlx::query!(
        r#"
        SELECT id            AS "id!: Uuid",
               password_hash AS "password_hash!: String",
               role          AS "role!: String",
               status        AS "status!: String"
        FROM   users
        WHERE  email = $1 AND deleted_at IS NULL
        "#,
        req.email
    )
    .fetch_optional(&state.db)
    .await?;

    // Always run a verify even on miss to keep the timing constant-ish.
    // The argon2 verify against a dummy hash takes ~the same time as a real
    // one; that's enough to defeat naive email-enumeration timing attacks.
    let (matched, user) = match row {
        Some(u) => {
            let ok = password::verify(&u.password_hash, &req.password)?;
            (ok, Some(u))
        }
        None => {
            // Dummy verify against a known hash. We don't actually mint a
            // hash on every miss because that would be slow; instead we run
            // verify against a baked PHC string.
            const DUMMY: &str = "$argon2id$v=19$m=4096,t=1,p=1$YWFhYWFhYWFhYWFhYWFhYQ$X9DkxLgGCgnLEXJ2v0+TJoLg+8iX/qsAqK1zybvk7n0";
            let _ = password::verify(DUMMY, &req.password);
            (false, None)
        }
    };

    if !matched {
        return Err(AppError::Unauthorized);
    }
    let user = user.expect("matched implies Some");

    if user.status != "active" {
        return Err(AppError::Forbidden);
    }

    issue_session_response(&state, user.id, &user.role, &headers).await
}

async fn logout(State(state): State<AppState>, user: CurrentUser) -> AppResult<impl IntoResponse> {
    // API tokens have no session to revoke; they're managed in settings.
    let session_id = user.session_id.ok_or(AppError::BadRequest(
        "nothing to log out — API tokens are revoked from settings".into(),
    ))?;
    sessions::revoke(&state.db, session_id, "logout").await?;
    let headers = clear_auth_cookies(&state.cfg);
    Ok((StatusCode::NO_CONTENT, headers))
}

async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<impl IntoResponse> {
    // Refresh cookie ONLY — never accept a refresh token in the body. That
    // way it can't be CSRF-stolen out of a request log.
    let refresh_plain = read_cookie(&headers, "sprintly_refresh").ok_or(AppError::Unauthorized)?;

    let (session_id, user_id, role, refresh) =
        match sessions::rotate(&state.db, &state.cfg.auth, &refresh_plain).await? {
            sessions::RotateOutcome::Rotated {
                session_id,
                user_id,
                role,
                refresh,
            } => (session_id, user_id, role, refresh),
        };

    let access = tokens::mint_access(&state.cfg.auth, user_id, session_id, &role)?;
    let cookies = set_auth_cookies(&state.cfg, &access, &refresh.plaintext);

    let user = load_user_dto(&state.db, user_id).await?;
    Ok((
        StatusCode::OK,
        cookies,
        Json(AuthResponse {
            access_token: access,
            user,
        }),
    ))
}

async fn password_reset_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    info: ConnectInfo<SocketAddr>,
    Json(req): Json<PasswordResetRequestReq>,
) -> AppResult<impl IntoResponse> {
    // Throttle reset-spam by source IP and by target email. 429 reveals nothing
    // about account existence (same for any email).
    let ip = client_ip(&headers, info);
    rate_limit::hit(
        &state,
        &format!("sprintly:rl:reset:ip:{ip}"),
        rate_limit::reset_ip_per_hour(),
        3600,
    )
    .await?;
    rate_limit::hit(
        &state,
        &format!("sprintly:rl:reset:email:{}", req.email.to_lowercase()),
        rate_limit::reset_email_per_hour(),
        3600,
    )
    .await?;

    // We always respond 200 with a generic body — never reveal whether the
    // email exists. In v1 the actual token URL is rendered in the admin UI;
    // email delivery is out of scope.
    let user_id: Option<Uuid> =
        sqlx::query_scalar(r#"SELECT id FROM users WHERE email = $1 AND deleted_at IS NULL"#)
            .bind(&req.email)
            .fetch_optional(&state.db)
            .await?;

    let mut payload = serde_json::json!({
        "message": "If that account exists, a reset link has been generated."
    });

    if let Some(uid) = user_id {
        let (plain, hash) = random_token_pair();
        let expires_at = Utc::now() + Duration::minutes(30);
        sqlx::query(
            r#"
            INSERT INTO password_reset_tokens (id, user_id, token_hash, expires_at)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(uid)
        .bind(hash.as_slice())
        .bind(expires_at)
        .execute(&state.db)
        .await?;

        // Email the reset link (best-effort; spawned so it never blocks the
        // response, and a down mail server can't 500 the request).
        crate::infra::email::spawn_send(
            state.mailer.clone(),
            crate::infra::email::password_reset(&state.cfg.public_url, &plain, &req.email),
        );

        // Also return the token in the response in dev — handy when no SMTP is
        // configured. In prod an admin can still surface it via the admin UI.
        if state.cfg.is_dev() {
            payload["dev_token"] = serde_json::Value::String(plain);
        }
    }
    Ok(Json(payload))
}

async fn password_reset_confirm(
    State(state): State<AppState>,
    Json(req): Json<PasswordResetConfirmReq>,
) -> AppResult<impl IntoResponse> {
    req.validate()
        .map_err(|e| AppError::Validation(e.to_string()))?;

    let presented = hash_token(&req.token);
    let row = sqlx::query!(
        r#"
        SELECT id        AS "id!: Uuid",
               user_id   AS "user_id!: Uuid",
               consumed_at,
               expires_at AS "expires_at!: chrono::DateTime<chrono::Utc>"
        FROM   password_reset_tokens
        WHERE  token_hash = $1
        "#,
        presented.as_slice()
    )
    .fetch_optional(&state.db)
    .await?;

    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };
    if row.consumed_at.is_some() || row.expires_at <= Utc::now() {
        return Err(AppError::Unauthorized);
    }

    let new_hash = password::hash(&state.cfg.auth, &req.new_password)?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = $1 WHERE id = $2")
        .bind(&new_hash)
        .bind(row.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE password_reset_tokens SET consumed_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(&mut *tx)
        .await?;
    // Belt-and-suspenders: kill all the user's sessions so a stolen reset
    // link can't coexist with the attacker's prior session.
    sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'password_reset' WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(row.user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(row.user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

// ─── helpers ────────────────────────────────────────────────────────────────

async fn issue_session_response(
    state: &AppState,
    user_id: Uuid,
    role: &str,
    headers: &HeaderMap,
) -> AppResult<(StatusCode, HeaderMap, Json<AuthResponse>)> {
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let issued = sessions::create(&state.db, &state.cfg.auth, user_id, ua.as_deref(), None).await?;
    let access = tokens::mint_access(&state.cfg.auth, user_id, issued.session_id, role)?;
    // Stamp last_seen so the user list shows "active now".
    sqlx::query("UPDATE users SET last_seen_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await?;

    let cookies = set_auth_cookies(&state.cfg, &access, &issued.refresh.plaintext);
    let user = load_user_dto(&state.db, user_id).await?;

    Ok((
        StatusCode::OK,
        cookies,
        Json(AuthResponse {
            access_token: access,
            user,
        }),
    ))
}

async fn load_user_dto(db: &PgPool, user_id: Uuid) -> AppResult<UserDto> {
    let u = sqlx::query!(
        r#"
        SELECT id           AS "id!: Uuid",
               email        AS "email!: String",
               handle       AS "handle!: String",
               display_name AS "display_name!: String",
               role         AS "role!: String"
        FROM   users WHERE id = $1
        "#,
        user_id
    )
    .fetch_one(db)
    .await?;
    Ok(UserDto {
        id: u.id,
        email: u.email,
        handle: u.handle,
        display_name: u.display_name,
        role: u.role,
    })
}

async fn consume_invite(db: &PgPool, token_plain: &str) -> AppResult<(String, Uuid)> {
    let presented = hash_token(token_plain);
    let row = sqlx::query!(
        r#"
        SELECT id              AS "id!: Uuid",
               suggested_role  AS "suggested_role!: String",
               expires_at      AS "expires_at!: chrono::DateTime<chrono::Utc>",
               consumed_at
        FROM   invite_tokens
        WHERE  token_hash = $1
        "#,
        presented.as_slice()
    )
    .fetch_optional(db)
    .await?
    .ok_or(AppError::Forbidden)?;

    if row.consumed_at.is_some() || row.expires_at <= Utc::now() {
        return Err(AppError::Forbidden);
    }
    Ok((row.suggested_role, row.id))
}

fn random_token_pair() -> (String, [u8; 32]) {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    let mut h = Sha256::new();
    h.update(raw);
    (
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw),
        h.finalize().into(),
    )
}

fn hash_token(plain: &str) -> [u8; 32] {
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(plain)
        .unwrap_or_default();
    let mut h = Sha256::new();
    h.update(raw);
    h.finalize().into()
}

fn cookie_secure(cfg: &Config) -> &'static str {
    if cfg.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    }
}

fn set_auth_cookies(cfg: &Config, access: &str, refresh: &str) -> HeaderMap {
    let secure = cookie_secure(cfg);
    let access_cookie = format!(
        "sprintly_access={access}; Path=/; HttpOnly; SameSite=Lax; Max-Age={ttl}{secure}",
        ttl = cfg.auth.access_ttl_secs
    );
    // Refresh cookie scoped to /api/v1/auth so it's never sent on regular
    // API calls or page loads.
    let refresh_cookie = format!(
        "sprintly_refresh={refresh}; Path=/api/v1/auth; HttpOnly; SameSite=Lax; Max-Age={ttl}{secure}",
        ttl = cfg.auth.refresh_ttl_secs
    );
    // CSRF cookie is intentionally NOT HttpOnly — the browser JS needs to
    // read it and echo it as a header (double-submit pattern).
    let csrf_nonce = csrf::fresh_nonce();
    let csrf_cookie = format!(
        "sprintly_csrf={csrf_nonce}; Path=/; SameSite=Lax; Max-Age={ttl}{secure}",
        ttl = cfg.auth.access_ttl_secs
    );

    let mut h = HeaderMap::new();
    h.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&access_cookie).expect("ascii"),
    );
    h.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&refresh_cookie).expect("ascii"),
    );
    h.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&csrf_cookie).expect("ascii"),
    );
    h
}

fn clear_auth_cookies(cfg: &Config) -> HeaderMap {
    let secure = cookie_secure(cfg);
    let mut h = HeaderMap::new();
    for c in [
        format!("sprintly_access=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}"),
        format!("sprintly_refresh=; Path=/api/v1/auth; HttpOnly; SameSite=Lax; Max-Age=0{secure}"),
        format!("sprintly_csrf=; Path=/; SameSite=Lax; Max-Age=0{secure}"),
    ] {
        h.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&c).expect("ascii"),
        );
    }
    h
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    for kv in raw.split(';') {
        let kv = kv.trim();
        if let Some(rest) = kv.strip_prefix(&format!("{name}=")) {
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}
