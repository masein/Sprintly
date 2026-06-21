//! M3 phase A integration tests.

use deadpool_redis::{Config as RedisConfig, Runtime};
use sprintly_api::{
    config::AuthConfig,
    domain::{notifications, password, tasks as task_domain},
};
use sqlx::PgPool;
use uuid::Uuid;

fn redis_pool() -> deadpool_redis::Pool {
    let url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379/0".into());
    RedisConfig::from_url(url)
        .create_pool(Some(Runtime::Tokio1))
        .expect("redis pool")
}

async fn add_member(pool: &PgPool, project_id: Uuid, user_id: Uuid, role: &str) {
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, $3, $2)"#,
    )
    .bind(project_id)
    .bind(user_id)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
}

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
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, $4, $5, 'member')
        "#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", id.simple()))
    .bind(format!("h{}", id.simple()))
    .bind("Test User")
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project_with_board(pool: &PgPool, key: &str, owner: Uuid) -> (Uuid, Uuid, Uuid) {
    let pid = Uuid::now_v7();
    sqlx::query(r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, $2, $3, $4)"#)
        .bind(pid)
        .bind(key)
        .bind(key)
        .bind(owner)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(
        r#"INSERT INTO project_members (project_id, user_id, role, added_by)
           VALUES ($1, $2, 'lead', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();

    let board_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO boards (id, project_id, name, is_default) VALUES ($1, $2, 'B', true)"#,
    )
    .bind(board_id)
    .bind(pid)
    .execute(pool)
    .await
    .unwrap();

    let col_id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
           VALUES ($1, $2, 'To do', 'todo', 1024.0)"#,
    )
    .bind(col_id)
    .bind(board_id)
    .execute(pool)
    .await
    .unwrap();
    (pid, board_id, col_id)
}

async fn make_task(
    pool: &PgPool,
    project_id: Uuid,
    board_id: Uuid,
    column_id: Uuid,
    order_in_column: f64,
) -> (Uuid, String) {
    let mut tx = pool.begin().await.unwrap();
    let (key, _) = task_domain::next_key(&mut tx, project_id).await.unwrap();
    let task_id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(task_id)
    .bind(project_id)
    .bind(board_id)
    .bind(column_id)
    .bind(&key)
    .bind("Test")
    .bind(order_in_column)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (task_id, key)
}

// QA F2/F3: assignee + labels on the task detail.

/// Assigning to a teammate persists, creates the F5 "assigned" notification for
/// them (the exact path the task PATCH reuses), and unassign clears it.
#[sqlx::test(migrations = "./migrations")]
async fn assign_persists_notifies_and_unassign_clears(pool: PgPool) {
    let owner = make_user(&pool).await;
    let teammate = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "ASG", owner).await;
    add_member(&pool, pid, teammate, "contributor").await;
    let (task_id, key) = make_task(&pool, pid, board, col, 1024.0).await;

    // Assign to the teammate + fire the F5 notification (mirrors edit_task).
    sqlx::query("UPDATE tasks SET assignee_id = $2 WHERE id = $1")
        .bind(task_id)
        .bind(teammate)
        .execute(&pool)
        .await
        .unwrap();
    notifications::notify(
        &pool,
        &redis_pool(),
        teammate,
        owner,
        "assigned",
        &format!("You were assigned {key}"),
        None,
        Some(&format!("/tasks/{key}")),
        Some(task_id),
    )
    .await
    .unwrap();

    let assignee: Option<Uuid> = sqlx::query_scalar("SELECT assignee_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(assignee, Some(teammate));

    let notified: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM notifications
            WHERE user_id = $1 AND actor_id = $2 AND kind = 'assigned' AND task_id = $3"#,
    )
    .bind(teammate)
    .bind(owner)
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(notified, 1, "the assignee gets an F5 notification");

    // Unassign clears it.
    sqlx::query("UPDATE tasks SET assignee_id = NULL WHERE id = $1")
        .bind(task_id)
        .execute(&pool)
        .await
        .unwrap();
    let assignee: Option<Uuid> = sqlx::query_scalar("SELECT assignee_id FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(assignee, None);
}

/// `notify` never notifies you about your own action — assigning a task to
/// yourself is silent.
#[sqlx::test(migrations = "./migrations")]
async fn self_assignment_is_not_notified(pool: PgPool) {
    let me = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "SLF", me).await;
    let (task_id, key) = make_task(&pool, pid, board, col, 1024.0).await;
    notifications::notify(
        &pool,
        &redis_pool(),
        me,
        me,
        "assigned",
        &format!("You were assigned {key}"),
        None,
        None,
        Some(task_id),
    )
    .await
    .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id = $1")
        .bind(me)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

/// The members list (the assignee picker's source) is scoped to one project.
#[sqlx::test(migrations = "./migrations")]
async fn member_list_is_scoped_to_project(pool: PgPool) {
    let owner_a = make_user(&pool).await;
    let teammate_a = make_user(&pool).await;
    let owner_b = make_user(&pool).await;
    let (proj_a, _, _) = make_project_with_board(&pool, "MEMA", owner_a).await;
    add_member(&pool, proj_a, teammate_a, "contributor").await;
    let (_proj_b, _, _) = make_project_with_board(&pool, "MEMB", owner_b).await;

    // The exact query the GET /projects/:key/members route runs.
    let members: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT pm.user_id
             FROM project_members pm JOIN users u ON u.id = pm.user_id
            WHERE pm.project_id = $1 AND u.deleted_at IS NULL
            ORDER BY pm.role, u.handle"#,
    )
    .bind(proj_a)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.contains(&owner_a) && members.contains(&teammate_a));
    assert!(
        !members.contains(&owner_b),
        "members must not leak across projects"
    );
}

/// Labels are a TEXT[] on the task: setting the new array attaches, sending a
/// shorter array detaches — exactly what the detail panel's multi-select sends.
#[sqlx::test(migrations = "./migrations")]
async fn labels_attach_and_detach(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "LBL", owner).await;
    let (task_id, _) = make_task(&pool, pid, board, col, 1024.0).await;

    sqlx::query("UPDATE tasks SET labels = $2 WHERE id = $1")
        .bind(task_id)
        .bind(vec!["backend".to_string(), "urgent".to_string()])
        .execute(&pool)
        .await
        .unwrap();
    let labels: Vec<String> = sqlx::query_scalar("SELECT labels FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(labels, vec!["backend", "urgent"]);

    sqlx::query("UPDATE tasks SET labels = $2 WHERE id = $1")
        .bind(task_id)
        .bind(vec!["backend".to_string()])
        .execute(&pool)
        .await
        .unwrap();
    let labels: Vec<String> = sqlx::query_scalar("SELECT labels FROM tasks WHERE id = $1")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(labels, vec!["backend"], "detaching removes the label");
}

#[sqlx::test(migrations = "./migrations")]
async fn task_keys_increment_per_project(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (p1, _, _) = make_project_with_board(&pool, "ALPHA", owner).await;
    let (p2, _, _) = make_project_with_board(&pool, "BETA", owner).await;

    let mut tx = pool.begin().await.unwrap();
    let (a1, _) = task_domain::next_key(&mut tx, p1).await.unwrap();
    let (a2, _) = task_domain::next_key(&mut tx, p1).await.unwrap();
    let (b1, _) = task_domain::next_key(&mut tx, p2).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(a1, "ALPHA-1");
    assert_eq!(a2, "ALPHA-2");
    assert_eq!(b1, "BETA-1", "second project resets the sequence");
}

#[sqlx::test(migrations = "./migrations")]
async fn task_keys_unique_per_project(pool: PgPool) {
    // The (project_id, key) composite uniqueness must hold even if someone
    // forces a manual insert. The unique partial index is the contract here.
    let owner = make_user(&pool).await;
    let (p1, board, col) = make_project_with_board(&pool, "DUP", owner).await;
    make_task(&pool, p1, board, col, 1024.0).await;

    // Manually insert with a colliding key.
    let res = sqlx::query(
        r#"
        INSERT INTO tasks (id, project_id, board_id, column_id, key, title, order_in_column)
        VALUES ($1, $2, $3, $4, 'DUP-1', 'collides', 2048.0)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(p1)
    .bind(board)
    .bind(col)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "duplicate (project, key) must be rejected");
}

#[sqlx::test(migrations = "./migrations")]
async fn resolve_position_between_two_tasks(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "POS", owner).await;
    let (t1, _) = make_task(&pool, pid, board, col, 1024.0).await;
    let (t2, _) = make_task(&pool, pid, board, col, 2048.0).await;

    let between = task_domain::resolve_position(&pool, col, Some(t1), None)
        .await
        .unwrap();
    assert!(
        between > 1024.0 && between < 2048.0,
        "drop-after-t1 must land strictly between t1 and t2 (got {between})"
    );

    let before_t2 = task_domain::resolve_position(&pool, col, None, Some(t2))
        .await
        .unwrap();
    assert!(
        before_t2 > 1024.0 && before_t2 < 2048.0,
        "drop-before-t2 must also land between t1 and t2 (got {before_t2})"
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn resolve_position_append_when_no_hints(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "APP", owner).await;
    let (_, _) = make_task(&pool, pid, board, col, 1024.0).await;
    let (_, _) = make_task(&pool, pid, board, col, 2048.0).await;

    let appended = task_domain::resolve_position(&pool, col, None, None)
        .await
        .unwrap();
    assert!(appended > 2048.0);
}

// QA F4/F8: subtask hierarchy.

async fn make_subtask(
    pool: &PgPool,
    project_id: Uuid,
    board_id: Uuid,
    column_id: Uuid,
    parent_id: Uuid,
    sprint: Option<Uuid>,
    order: f64,
) -> (Uuid, String) {
    let mut tx = pool.begin().await.unwrap();
    let (key, _) = task_domain::next_key(&mut tx, project_id).await.unwrap();
    let id = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO tasks
             (id, project_id, board_id, column_id, key, title, parent_task_id, sprint_id, order_in_column)
           VALUES ($1, $2, $3, $4, $5, 'sub', $6, $7, $8)"#,
    )
    .bind(id)
    .bind(project_id)
    .bind(board_id)
    .bind(column_id)
    .bind(&key)
    .bind(parent_id)
    .bind(sprint)
    .bind(order)
    .execute(&mut *tx)
    .await
    .unwrap();
    tx.commit().await.unwrap();
    (id, key)
}

/// Subtasks are excluded from the top-level board + backlog, but still listed
/// under their parent. (Mirrors the list_tasks / backlog / list_subtasks WHERE.)
#[sqlx::test(migrations = "./migrations")]
async fn board_and_backlog_exclude_subtasks(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "SUB", owner).await;
    let (parent_id, _) = make_task(&pool, pid, board, col, 1024.0).await;
    make_subtask(&pool, pid, board, col, parent_id, None, 2048.0).await;

    // Board (list_tasks) lists only the top-level card.
    let board_ids: Vec<Uuid> = sqlx::query_scalar(
        r#"SELECT id FROM tasks
            WHERE project_id = $1 AND deleted_at IS NULL AND parent_task_id IS NULL"#,
    )
    .bind(pid)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        board_ids,
        vec![parent_id],
        "the board shows only the parent"
    );

    // Backlog excludes the subtask too.
    let backlog: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM tasks
            WHERE project_id = $1 AND sprint_id IS NULL AND deleted_at IS NULL
              AND status <> 'done' AND parent_task_id IS NULL"#,
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(backlog, 1);

    // But the subtask IS listed under its parent.
    let children: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE parent_task_id = $1 AND deleted_at IS NULL",
    )
    .bind(parent_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(children, 1);
}

/// Sprint task counts roll subtasks up under their parent — they don't add a
/// second item to the tally.
#[sqlx::test(migrations = "./migrations")]
async fn sprint_count_excludes_subtasks(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "SPC", owner).await;
    let sprint = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, state, starts_at, ends_at)
           VALUES ($1, $2, 'S', 'active', now() - interval '1 day', now() + interval '6 days')"#,
    )
    .bind(sprint)
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap();
    let (parent_id, _) = make_task(&pool, pid, board, col, 1024.0).await;
    sqlx::query("UPDATE tasks SET sprint_id = $2 WHERE id = $1")
        .bind(parent_id)
        .bind(sprint)
        .execute(&pool)
        .await
        .unwrap();
    make_subtask(&pool, pid, board, col, parent_id, Some(sprint), 2048.0).await;

    let count: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM tasks
            WHERE sprint_id = $1 AND deleted_at IS NULL AND parent_task_id IS NULL"#,
    )
    .bind(sprint)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 1, "the sprint counts top-level tasks only");
}

/// The task read resolves the parent's key for a subtask (for the breadcrumb /
/// "parent" link) and NULL for a top-level task. (Mirrors fetch_task's join.)
#[sqlx::test(migrations = "./migrations")]
async fn fetch_task_exposes_parent_key(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "PAR", owner).await;
    let (parent_id, parent_key) = make_task(&pool, pid, board, col, 1024.0).await;
    let (_sub, sub_key) = make_subtask(&pool, pid, board, col, parent_id, None, 2048.0).await;

    let parent_of = |key: String| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, Option<String>>(
                r#"SELECT parent.key
                     FROM tasks t
                     LEFT JOIN tasks parent ON parent.id = t.parent_task_id
                    WHERE t.key = $1 AND t.project_id = $2 AND t.deleted_at IS NULL"#,
            )
            .bind(key)
            .bind(pid)
            .fetch_one(&pool)
            .await
            .unwrap()
        }
    };

    assert_eq!(
        parent_of(sub_key).await.as_deref(),
        Some(parent_key.as_str())
    );
    assert_eq!(parent_of(parent_key.clone()).await, None);
}

/// The board scope (F10): with a sprint selected, list_tasks returns only that
/// sprint's tasks; with no scope ("all") it returns the whole project; and
/// `active_sprint_id` resolves the active sprint (or None before one starts).
#[sqlx::test(migrations = "./migrations")]
async fn board_scopes_to_the_active_sprint(pool: PgPool) {
    use sprintly_api::domain::sprints as sprint_domain;

    let owner = make_user(&pool).await;
    let (pid, board, col) = make_project_with_board(&pool, "SCO", owner).await;

    // No active sprint yet → the helper returns None (board stays "all tasks").
    assert!(sprint_domain::active_sprint_id(&pool, pid)
        .await
        .unwrap()
        .is_none());

    let sprint = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO sprints (id, project_id, name, state, starts_at, ends_at)
           VALUES ($1, $2, 'S', 'active', now() - interval '1 day', now() + interval '6 days')"#,
    )
    .bind(sprint)
    .bind(pid)
    .execute(&pool)
    .await
    .unwrap();

    // One task in the sprint, one left in the backlog (no sprint).
    let (in_sprint, in_sprint_key) = make_task(&pool, pid, board, col, 1024.0).await;
    sqlx::query("UPDATE tasks SET sprint_id = $2 WHERE id = $1")
        .bind(in_sprint)
        .bind(sprint)
        .execute(&pool)
        .await
        .unwrap();
    let (_backlog, backlog_key) = make_task(&pool, pid, board, col, 2048.0).await;

    // The helper now resolves the active sprint.
    assert_eq!(
        sprint_domain::active_sprint_id(&pool, pid).await.unwrap(),
        Some(sprint)
    );

    // Mirror the list_tasks board WHERE, parameterised by the resolved scope.
    let board_keys = |scope: Option<Uuid>| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(
                r#"SELECT key FROM tasks
                    WHERE project_id = $1
                      AND deleted_at IS NULL
                      AND parent_task_id IS NULL
                      AND ($2::uuid IS NULL OR sprint_id = $2)
                    ORDER BY key"#,
            )
            .bind(pid)
            .bind(scope)
            .fetch_all(&pool)
            .await
            .unwrap()
        }
    };

    // Scoped to the active sprint → only the sprint task.
    assert_eq!(board_keys(Some(sprint)).await, vec![in_sprint_key.clone()]);
    // No scope ("all tasks") → the whole project, sprint + backlog.
    let mut all = board_keys(None).await;
    all.sort();
    let mut expected = vec![in_sprint_key, backlog_key];
    expected.sort();
    assert_eq!(all, expected);
}
