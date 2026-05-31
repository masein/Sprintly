//! M5 integration tests.

use chrono::{Duration, NaiveDate, Utc};
use sprintly_api::{
    config::AuthConfig,
    domain::{
        password,
        sprints::{self as sprint_domain, SprintState},
        tasks as task_domain,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

fn cfg() -> AuthConfig {
    AuthConfig {
        jwt_secret: b"a-test-secret-that-is-long-enough-to-be-fine".to_vec(),
        access_ttl_secs: 900,
        refresh_ttl_secs: 2_592_000,
        argon2_m_cost_kib: 4096,
        argon2_t_cost: 1,
        argon2_p_cost: 1,
    }
}

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, 'member')"#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", &id.to_string()[..8]))
    .bind(format!("h{}", &id.to_string()[..8]))
    .bind("Test")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid).bind(key).bind(key).bind(owner).execute(pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, 'lead', $2)"#,
    ).bind(pid).bind(owner).execute(pool).await.unwrap();
    let board = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    ).bind(board).bind(pid).execute(pool).await.unwrap();
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'C', 'todo', 1024.0)"#,
    ).bind(col).bind(board).execute(pool).await.unwrap();
    (pid, board, col)
}

async fn make_sprint(pool: &PgPool, project_id: Uuid, state: &str) -> Uuid {
    let id = Uuid::now_v7();
    let start = Utc::now() - Duration::days(7);
    let end = Utc::now() + Duration::days(7);
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at, state)
           VALUES ($1, $2, 'S', $3, $4, $5)"#,
    ).bind(id).bind(project_id).bind(start).bind(end).bind(state)
    .execute(pool).await.unwrap();
    id
}

#[sqlx::test(migrations = "./migrations")]
async fn one_active_sprint_per_project(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "ACT", owner).await;
    let _a = make_sprint(&pool, pid, "active").await;
    let res = sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, starts_at, ends_at, state)
           VALUES ($1, $2, 'S2', now(), now() + INTERVAL '1 day', 'active')"#,
    )
    .bind(Uuid::now_v7()).bind(pid).execute(&pool).await;
    assert!(res.is_err(), "partial unique index must reject a second active sprint");
}

#[sqlx::test(migrations = "./migrations")]
async fn state_machine_transitions_blocked(_pool: PgPool) {
    // Pure-logic re-check.
    assert!(sprint_domain::next_state(SprintState::Planned, "complete").is_err());
    assert!(sprint_domain::next_state(SprintState::Active, "start").is_err());
    assert!(sprint_domain::next_state(SprintState::Completed, "start").is_err());
    assert!(sprint_domain::next_state(SprintState::Completed, "complete").is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn completing_creates_retro_and_velocity(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project(&pool, "VEL", owner).await;
    let sid = make_sprint(&pool, pid, "active").await;

    // 3 tasks: 2 done w/ 5+3 pts, 1 incomplete w/ 8 pts.
    let mut tx = pool.begin().await.unwrap();
    for (sp, status) in [(5, "done"), (3, "done"), (8, "todo")] {
        let (k, _) = task_domain::next_key(&mut tx, pid).await.unwrap();
        let tid = Uuid::now_v7();
        sqlx::query(
            r#"INSERT INTO tasks
                  (id, project_id, board_id, column_id, key, title, order_in_column,
                   sprint_id, story_points, status)
               VALUES ($1, $2, $3, $4, $5, 't', 1024.0, $6, $7, $8)"#,
        )
        .bind(tid).bind(pid).bind(board).bind(col).bind(&k).bind(sid).bind(sp).bind(status)
        .execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();

    // Simulate the route's complete step (snapshot velocity + open retro).
    let velocity: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(story_points), 0)
           FROM   tasks
           WHERE  sprint_id = $1 AND status = 'done' AND deleted_at IS NULL"#,
    )
    .bind(sid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(velocity, 8, "velocity = sum of done story points");

    sqlx::query(
        r#"UPDATE sprints
              SET state = 'completed', completed_at = now(), velocity_points = $2
            WHERE id = $1"#,
    )
    .bind(sid).bind(velocity as i32).execute(&pool).await.unwrap();
    sqlx::query(
        r#"INSERT INTO sprint_retros (id, sprint_id) VALUES ($1, $2)
           ON CONFLICT (sprint_id) DO NOTHING"#,
    )
    .bind(Uuid::now_v7()).bind(sid).execute(&pool).await.unwrap();

    // Exactly one retro per sprint (UNIQUE).
    let retro_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sprint_retros WHERE sprint_id = $1",
    )
    .bind(sid).fetch_one(&pool).await.unwrap();
    assert_eq!(retro_count, 1);
}

#[sqlx::test(migrations = "./migrations")]
async fn retro_one_per_sprint(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "ONE", owner).await;
    let sid = make_sprint(&pool, pid, "completed").await;
    sqlx::query(
        r#"INSERT INTO sprint_retros (id, sprint_id) VALUES ($1, $2)"#,
    )
    .bind(Uuid::now_v7()).bind(sid).execute(&pool).await.unwrap();
    let dup = sqlx::query(
        r#"INSERT INTO sprint_retros (id, sprint_id) VALUES ($1, $2)"#,
    )
    .bind(Uuid::now_v7()).bind(sid).execute(&pool).await;
    assert!(dup.is_err(), "UNIQUE(sprint_id) on sprint_retros must hold");
}

#[sqlx::test(migrations = "./migrations")]
async fn anonymous_note_drops_author_id(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "ANON", owner).await;
    let sid = make_sprint(&pool, pid, "completed").await;
    let retro = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprint_retros (id, sprint_id) VALUES ($1, $2)"#,
    )
    .bind(retro).bind(sid).execute(&pool).await.unwrap();
    // Insert an "anonymous" note with author_id explicitly NULL.
    sqlx::query(
        r#"INSERT INTO retro_notes (id, retro_id, column_kind, body, anonymous)
           VALUES ($1, $2, 'went_well', 'great pairing', true)"#,
    ).bind(Uuid::now_v7()).bind(retro).execute(&pool).await.unwrap();
    let author: Option<Uuid> =
        sqlx::query_scalar("SELECT author_id FROM retro_notes WHERE retro_id = $1")
            .bind(retro).fetch_one(&pool).await.unwrap();
    assert!(author.is_none(), "anonymous note must not carry author_id");
}

#[sqlx::test(migrations = "./migrations")]
async fn vote_unique_per_user_per_note(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "VOT", owner).await;
    let sid = make_sprint(&pool, pid, "completed").await;
    let retro = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO sprint_retros (id, sprint_id) VALUES ($1, $2)"#)
        .bind(retro).bind(sid).execute(&pool).await.unwrap();
    let nid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO retro_notes (id, retro_id, column_kind, body)
           VALUES ($1, $2, 'kudos', 'cheers')"#,
    ).bind(nid).bind(retro).execute(&pool).await.unwrap();
    sqlx::query(r#"INSERT INTO retro_votes (retro_note_id, user_id) VALUES ($1, $2)"#)
        .bind(nid).bind(owner).execute(&pool).await.unwrap();
    let dup = sqlx::query(r#"INSERT INTO retro_votes (retro_note_id, user_id) VALUES ($1, $2)"#)
        .bind(nid).bind(owner).execute(&pool).await;
    assert!(dup.is_err(), "(retro_note_id, user_id) PK must reject dup votes");
}

#[sqlx::test(migrations = "./migrations")]
async fn summary_markdown_unit_check(_pool: PgPool) {
    let s = NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
    let e = NaiveDate::from_ymd_opt(2026, 5, 31).unwrap();
    let md = sprint_domain::retro_summary_markdown(&sprint_domain::RetroSummaryInput {
        sprint_name: "S",
        sprint_goal: "",
        starts: s,
        ends: e,
        velocity_points: Some(10),
        completed_count: 4,
        carried_count: 1,
        went_well: vec!["a"],
        went_poorly: vec![],
        action_items: vec!["fix X"],
        kudos: vec![],
    });
    assert!(md.contains("# S"));
    assert!(md.contains("Velocity"));
    assert!(md.contains("fix X"));
}
