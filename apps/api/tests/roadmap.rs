//! F6 — roadmap: epic/milestone CRUD, task→epic association, progress rollup.

use chrono::NaiveDate;
use sprintly_api::domain::roadmap;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'T', 'x', 'member', 'active')"#,
    )
    .bind(id)
    .bind(format!("{}@x.test", id.simple()))
    // Full simple() — v7 UUIDs minted in the same ms share leading hex digits.
    .bind(format!("u{}", id.simple()))
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Returns (project_id, board_id, todo_column_id).
async fn make_project(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $2, $3)"#)
        .bind(pid)
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
    let col = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'To do', 'todo', 1024.0)"#,
    )
    .bind(col)
    .bind(board)
    .execute(pool)
    .await
    .unwrap();
    (pid, board, col)
}

async fn make_task(
    pool: &PgPool,
    pid: Uuid,
    board: Uuid,
    col: Uuid,
    key: &str,
    status: &str,
    epic_id: Option<Uuid>,
) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks (id, project_id, board_id, column_id, key, title, status, epic_id, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 't', $6, $7, 1024.0)"#,
    )
    .bind(id)
    .bind(pid)
    .bind(board)
    .bind(col)
    .bind(key)
    .bind(status)
    .bind(epic_id)
    .execute(pool)
    .await
    .unwrap();
    id
}

fn date(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

#[sqlx::test(migrations = "./migrations")]
async fn epic_and_milestone_crud(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project(&pool, "RM", owner).await;

    let e = roadmap::epic_create(
        &pool,
        pid,
        "Checkout v2",
        "#10b981",
        Some(date("2026-07-01")),
        Some(date("2026-07-31")),
    )
    .await
    .unwrap();
    assert_eq!(e.name, "Checkout v2");
    assert_eq!(e.start_date, Some(date("2026-07-01")));
    assert_eq!(e.task_count, 0);

    let m = roadmap::milestone_create(&pool, pid, "Beta cut", date("2026-07-15"))
        .await
        .unwrap();
    assert_eq!(m.name, "Beta cut");

    assert_eq!(roadmap::epics_list(&pool, pid).await.unwrap().len(), 1);
    assert_eq!(roadmap::milestones_list(&pool, pid).await.unwrap().len(), 1);

    // Update: clear the end date (double-option "set to null"), rename.
    let e = roadmap::epic_update(
        &pool,
        e.id,
        pid,
        Some("Checkout"),
        None,
        false,
        None,
        true,
        None,
    )
    .await
    .unwrap();
    assert_eq!(e.name, "Checkout");
    assert_eq!(e.start_date, Some(date("2026-07-01")), "start untouched");
    assert_eq!(e.end_date, None, "end cleared");

    let m = roadmap::milestone_update(&pool, m.id, pid, None, Some(date("2026-08-01")))
        .await
        .unwrap();
    assert_eq!(m.due_date, date("2026-08-01"));

    roadmap::epic_delete(&pool, e.id, pid).await.unwrap();
    roadmap::milestone_delete(&pool, m.id, pid).await.unwrap();
    assert!(roadmap::epics_list(&pool, pid).await.unwrap().is_empty());
    assert!(roadmap::epic_delete(&pool, e.id, pid).await.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn progress_is_done_over_total(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project(&pool, "PR", owner).await;
    let e = roadmap::epic_create(&pool, pid, "E", "#7c5cff", None, None)
        .await
        .unwrap();

    make_task(&pool, pid, board, col, "PR-1", "done", Some(e.id)).await;
    make_task(&pool, pid, board, col, "PR-2", "done", Some(e.id)).await;
    make_task(&pool, pid, board, col, "PR-3", "in_progress", Some(e.id)).await;
    // A task in the project but in no epic doesn't count toward this epic.
    make_task(&pool, pid, board, col, "PR-4", "done", None).await;
    // A soft-deleted task is excluded.
    let gone = make_task(&pool, pid, board, col, "PR-5", "done", Some(e.id)).await;
    sqlx::query("UPDATE tasks SET deleted_at = now() WHERE id = $1")
        .bind(gone)
        .execute(&pool)
        .await
        .unwrap();

    let epics = roadmap::epics_list(&pool, pid).await.unwrap();
    assert_eq!(epics.len(), 1);
    assert_eq!(epics[0].task_count, 3, "3 live tasks in the epic");
    assert_eq!(epics[0].done_count, 2, "2 of them done");
}

#[sqlx::test(migrations = "./migrations")]
async fn assign_and_unassign_and_cascade(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project(&pool, "AS", owner).await;
    let e = roadmap::epic_create(&pool, pid, "E", "#7c5cff", None, None)
        .await
        .unwrap();
    let task = make_task(&pool, pid, board, col, "AS-1", "todo", None).await;

    async fn epic_of(pool: &PgPool, task: Uuid) -> Option<Uuid> {
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT epic_id FROM tasks WHERE id = $1")
            .bind(task)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    roadmap::assign_task_epic(&pool, task, Some(e.id))
        .await
        .unwrap();
    assert_eq!(epic_of(&pool, task).await, Some(e.id));
    assert_eq!(
        roadmap::epics_list(&pool, pid).await.unwrap()[0].task_count,
        1
    );

    // Unassign.
    roadmap::assign_task_epic(&pool, task, None).await.unwrap();
    assert_eq!(epic_of(&pool, task).await, None);

    // Deleting an epic with tasks unassigns them (ON DELETE SET NULL).
    roadmap::assign_task_epic(&pool, task, Some(e.id))
        .await
        .unwrap();
    roadmap::epic_delete(&pool, e.id, pid).await.unwrap();
    assert_eq!(epic_of(&pool, task).await, None, "cascade clears the FK");
}
