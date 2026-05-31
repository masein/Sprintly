//! Achievement scanner.
//!
//! One pure function per achievement: given a `PgPool`, return the set of
//! `(user_id, context_json)` pairs that should be (newly or repeatedly)
//! awarded the achievement. The route layer or background scanner picks the
//! catalog row, runs the matching function, and `INSERT ... ON CONFLICT DO
//! NOTHING`s into `user_achievements`.
//!
//! Rules are written so they're cheap to re-run on a 5-minute cadence —
//! every query is bounded by indexed columns, and the COUNT/EXISTS
//! aggregates skim small subsets of the data set.

use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppResult;

/// One eligible award: (user_id, context to write on user_achievements.context).
pub type AwardCandidate = (Uuid, Value);

/// Run every rule and return a flat list of `(code, candidates)`.
/// Callers fold this into one insert per code.
pub async fn scan_all(pool: &PgPool) -> AppResult<Vec<(&'static str, Vec<AwardCandidate>)>> {
    Ok(vec![
        ("BUG_SLAYER", bug_slayer(pool).await?),
        ("PR_WIZARD", pr_wizard(pool).await?),
        ("WATCHER_IN_WHEAT_FIELD", watcher_in_wheat(pool).await?),
        ("COFFEE_ADDICT", coffee_addict(pool).await?),
        ("SPRINT_CLOSER", sprint_closer(pool).await?),
        ("RETRO_HERO", retro_hero(pool).await?),
        ("ESTIMATOR_SUPREME", estimator_supreme(pool).await?),
    ])
}

/// Apply a batch: insert `(user, achievement.id, context)` for every
/// candidate where `achievements.code = code`. Idempotent — the PK on
/// `(user_id, achievement_id)` collapses re-runs to no-ops.
pub async fn award_batch(
    pool: &PgPool,
    code: &str,
    candidates: &[AwardCandidate],
) -> AppResult<u64> {
    if candidates.is_empty() {
        return Ok(0);
    }
    let achievement_id: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM achievements WHERE code = $1")
            .bind(code)
            .fetch_optional(pool)
            .await?;
    let Some(aid) = achievement_id else {
        return Ok(0); // catalog row missing → nothing to award
    };
    let mut inserted = 0u64;
    for (user_id, ctx) in candidates {
        let r = sqlx::query(
            r#"
            INSERT INTO user_achievements (user_id, achievement_id, context)
            VALUES ($1, $2, $3)
            ON CONFLICT (user_id, achievement_id) DO NOTHING
            "#,
        )
        .bind(user_id)
        .bind(aid)
        .bind(ctx)
        .execute(pool)
        .await?;
        inserted += r.rows_affected();
    }
    Ok(inserted)
}

// ─── individual rules ───────────────────────────────────────────────────────

async fn bug_slayer(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    let rows = sqlx::query!(
        r#"
        SELECT t.reporter_id AS "user_id?: Uuid",
               COUNT(*)::bigint AS "n!: i64"
        FROM   tasks t
        WHERE  t.type = 'bug' AND t.status = 'done'
          AND  t.deleted_at IS NULL AND t.reporter_id IS NOT NULL
        GROUP  BY t.reporter_id
        HAVING COUNT(*) >= 50
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.user_id.map(|u| (u, json!({ "count": r.n }))))
        .collect())
}

async fn pr_wizard(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    let rows = sqlx::query!(
        r#"
        SELECT t.assignee_id AS "user_id?: Uuid",
               COUNT(*)::bigint AS "n!: i64"
        FROM   tasks t
        WHERE  t.status = 'done' AND t.deleted_at IS NULL
          AND  t.assignee_id IS NOT NULL
        GROUP  BY t.assignee_id
        HAVING COUNT(*) >= 50
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.user_id.map(|u| (u, json!({ "count": r.n }))))
        .collect())
}

async fn watcher_in_wheat(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    let rows = sqlx::query!(
        r#"
        SELECT w.user_id AS "user_id!: Uuid",
               COUNT(*)::bigint AS "n!: i64"
        FROM   task_watchers w
        JOIN   tasks t ON t.id = w.task_id AND t.deleted_at IS NULL
        GROUP  BY w.user_id
        HAVING COUNT(*) >= 30
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.user_id, json!({ "count": r.n })))
        .collect())
}

async fn coffee_addict(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    // "Closed after midnight" means ended_at's UTC hour is in [0, 5).
    let rows = sqlx::query!(
        r#"
        SELECT user_id AS "user_id!: Uuid",
               COUNT(*)::bigint AS "n!: i64"
        FROM   time_logs
        WHERE  deleted_at IS NULL AND ended_at IS NOT NULL
          AND  EXTRACT(HOUR FROM ended_at AT TIME ZONE 'UTC') < 5
        GROUP  BY user_id
        HAVING COUNT(*) >= 10
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.user_id, json!({ "count": r.n })))
        .collect())
}

async fn sprint_closer(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    // The completer of the chronologically last 'completed' task of any
    // completed sprint is the closer. Approximate "who marked it done" by
    // the most recent 'moved' activity actor on that task.
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT ON (s.id)
               s.id           AS "sprint_id!: Uuid",
               t.id           AS "task_id!: Uuid",
               a.actor_id     AS "user_id?: Uuid"
        FROM   sprints s
        JOIN   tasks t ON t.sprint_id = s.id
                      AND t.status = 'done'
                      AND t.completed_at IS NOT NULL
        LEFT JOIN LATERAL (
            SELECT actor_id
            FROM   task_activity
            WHERE  task_id = t.id AND kind = 'moved'
            ORDER  BY created_at DESC
            LIMIT  1
        ) a ON TRUE
        WHERE  s.state = 'completed' AND s.deleted_at IS NULL
        ORDER  BY s.id, t.completed_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| r.user_id.map(|u| (u, json!({ "sprint_id": r.sprint_id, "task_id": r.task_id }))))
        .collect())
}

async fn retro_hero(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    // Per closed retro, the author of the note with the most votes wins,
    // provided the note has ≥ 1 vote and isn't anonymous.
    let rows = sqlx::query!(
        r#"
        SELECT DISTINCT ON (r.id)
               r.id           AS "retro_id!: Uuid",
               n.author_id    AS "user_id?: Uuid",
               (SELECT COUNT(*) FROM retro_votes v WHERE v.retro_note_id = n.id)::bigint
                              AS "votes!: i64"
        FROM   sprint_retros r
        JOIN   retro_notes n ON n.retro_id = r.id AND n.deleted_at IS NULL
                            AND n.anonymous = false
                            AND n.author_id IS NOT NULL
        WHERE  r.state = 'closed'
        ORDER  BY r.id,
                  (SELECT COUNT(*) FROM retro_votes v WHERE v.retro_note_id = n.id) DESC,
                  n.created_at ASC
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            if r.votes < 1 { return None; }
            r.user_id.map(|u| (u, json!({ "retro_id": r.retro_id, "votes": r.votes })))
        })
        .collect())
}

async fn estimator_supreme(pool: &PgPool) -> AppResult<Vec<AwardCandidate>> {
    // For each (assignee, task) where the user logged time and the task has
    // an estimate, count the ones where |sum(minutes) - estimate| / estimate ≤ 0.10.
    let rows = sqlx::query!(
        r#"
        WITH per_task AS (
            SELECT t.assignee_id AS user_id,
                   t.id          AS task_id,
                   t.estimate_minutes,
                   COALESCE(SUM(tl.duration_minutes), 0)::bigint AS logged
            FROM   tasks t
            JOIN   time_logs tl ON tl.task_id = t.id AND tl.user_id = t.assignee_id
                                 AND tl.deleted_at IS NULL AND tl.ended_at IS NOT NULL
            WHERE  t.assignee_id IS NOT NULL
              AND  t.deleted_at IS NULL
              AND  t.estimate_minutes IS NOT NULL AND t.estimate_minutes > 0
              AND  t.status = 'done'
            GROUP BY t.assignee_id, t.id, t.estimate_minutes
        )
        SELECT user_id AS "user_id!: Uuid",
               COUNT(*)::bigint AS "n!: i64"
        FROM   per_task
        WHERE  ABS(logged - estimate_minutes) <= GREATEST(1, estimate_minutes / 10)
        GROUP  BY user_id
        HAVING COUNT(*) >= 20
        "#,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| (r.user_id, json!({ "count": r.n })))
        .collect())
}
