//! Integration tests for import/export (F16): a dry-run resolves everything but
//! writes nothing; a real run creates the columns + tasks; export reflects them.

use sprintly_api::{
    config::AuthConfig,
    domain::{import_export as ie, import_export::ImportFormat, password},
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
    .bind(format!("u{}@x.test", id.simple()))
    .bind(format!("h{}", id.simple()))
    .bind("Test")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Project + default board with a single "To do" column.
async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid)
        .bind(key)
        .bind(key)
        .bind(owner)
        .execute(pool)
        .await
        .unwrap();
    let board = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    )
    .bind(board)
    .bind(pid)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'To do', 'todo', 1024.0)"#,
    )
    .bind(Uuid::now_v7())
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    (pid, board)
}

const CSV: &str = "Name,Description,List,Labels\n\
                   Build the thing,does stuff,In Progress,\"backend; urgent\"\n\
                   Plan it,,To do,planning\n";

#[sqlx::test(migrations = "./migrations")]
async fn dry_run_resolves_but_writes_nothing(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board) = make_project(&pool, "IMP", owner).await;

    let plan = ie::parse(CSV, ImportFormat::Csv).unwrap();
    let report = ie::apply_import(&pool, pid, board, &plan, true)
        .await
        .unwrap();

    assert!(report.dry_run);
    assert_eq!(report.tasks_created, 2);
    assert_eq!(report.columns_created, vec!["In Progress"]);
    assert_eq!(report.columns_reused, vec!["To do"]);
    assert_eq!(report.labels_created.len(), 3); // backend, urgent, planning

    // Nothing actually persisted.
    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 0, "dry run must not write tasks");
    let cols: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM board_columns WHERE board_id = $1")
        .bind(board)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(cols, 1, "dry run must not write columns");
    let seq: i64 = sqlx::query_scalar("SELECT next_task_seq FROM projects WHERE id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(seq, 1, "key sequence rolled back");
}

#[sqlx::test(migrations = "./migrations")]
async fn apply_creates_rows_and_export_round_trips(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board) = make_project(&pool, "EXP", owner).await;

    let plan = ie::parse(CSV, ImportFormat::Csv).unwrap();
    let report = ie::apply_import(&pool, pid, board, &plan, false)
        .await
        .unwrap();
    assert!(!report.dry_run);
    assert_eq!(report.tasks_created, 2);

    // Rows persisted: 2 tasks, the new "In Progress" column, 3 labels.
    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 2);
    let in_progress: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE project_id = $1 AND status = 'in_progress'",
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        in_progress, 1,
        "the In Progress card got status in_progress"
    );

    // Export reflects the imported data.
    let bundle = ie::export_bundle(&pool, pid).await.unwrap();
    assert_eq!(bundle.project.key, "EXP");
    assert_eq!(bundle.tasks.len(), 2);
    assert!(bundle.columns.iter().any(|c| c.name == "In Progress"));
    let build = bundle
        .tasks
        .iter()
        .find(|t| t.title == "Build the thing")
        .unwrap();
    assert_eq!(build.column, "In Progress");
    assert_eq!(build.status, "in_progress");
    assert!(build.labels.contains(&"backend".to_string()));

    let csv = ie::export_csv(&bundle);
    assert!(csv.starts_with("key,title,status"));
    assert!(csv.contains("Build the thing"));
    assert!(csv.contains(&build.key));
}
