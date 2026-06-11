//! F7 — custom fields: definitions CRUD, value set/read with type
//! validation, board-filter matching, and search-tsv integration.

use sprintly_api::domain::{fields, tasks as task_domain};
use sqlx::PgPool;
use uuid::Uuid;

async fn make_user(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, 'Test User', 'x', 'member')
        "#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", id.simple()))
    .bind(format!("h{}", id.simple()))
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

fn opts(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[sqlx::test(migrations = "./migrations")]
async fn definition_crud_and_case_insensitive_uniqueness(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, _, _) = make_project_with_board(&pool, "FLD", owner).await;

    let sev = fields::create(&pool, pid, "Severity", "select", &opts(&["low", "high"]))
        .await
        .unwrap();
    assert_eq!(sev.r#type, "select");

    // Case-insensitive duplicate name → conflict.
    assert!(fields::create(&pool, pid, "SEVERITY", "text", &[])
        .await
        .is_err());

    // Type CHECK constraint rejects junk at the DB layer too.
    assert!(fields::create(&pool, pid, "Bogus", "checkbox", &[])
        .await
        .is_err());

    fields::create(&pool, pid, "Budget", "number", &[])
        .await
        .unwrap();
    let listed = fields::list(&pool, pid).await.unwrap();
    assert_eq!(listed.len(), 2);
    // Ordered by lower(name).
    assert_eq!(listed[0].name, "Budget");

    let renamed = fields::update(
        &pool,
        sev.id,
        pid,
        Some("Sev"),
        Some(&opts(&["low", "mid", "high"])),
    )
    .await
    .unwrap();
    assert_eq!(renamed.name, "Sev");
    assert_eq!(renamed.options.len(), 3);

    fields::delete(&pool, sev.id, pid).await.unwrap();
    assert_eq!(fields::list(&pool, pid).await.unwrap().len(), 1);
    // Deleting again → NotFound.
    assert!(fields::delete(&pool, sev.id, pid).await.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn values_persist_canonicalised_and_clear(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, bid, cid) = make_project_with_board(&pool, "VAL", owner).await;
    let (task_id, _) = make_task(&pool, pid, bid, cid, 1024.0).await;

    let budget = fields::create(&pool, pid, "Budget", "number", &[])
        .await
        .unwrap();
    let due = fields::create(&pool, pid, "Review by", "date", &[])
        .await
        .unwrap();

    // Numbers canonicalise: "3.50" and "3.5" are the same value.
    let canon = fields::canonical_value("number", &[], "3.50").unwrap();
    assert_eq!(canon, "3.5");
    fields::set_value(&pool, task_id, budget.id, &canon)
        .await
        .unwrap();
    fields::set_value(&pool, task_id, due.id, "2026-07-01")
        .await
        .unwrap();

    let vals = fields::list_for_task(&pool, pid, task_id).await.unwrap();
    assert_eq!(vals.len(), 2);
    let by_name = |n: &str| vals.iter().find(|v| v.name == n).unwrap();
    assert_eq!(by_name("Budget").value.as_deref(), Some("3.5"));
    assert_eq!(by_name("Review by").value.as_deref(), Some("2026-07-01"));

    // Upsert replaces.
    fields::set_value(&pool, task_id, budget.id, "8")
        .await
        .unwrap();
    let vals = fields::list_for_task(&pool, pid, task_id).await.unwrap();
    assert_eq!(
        vals.iter()
            .find(|v| v.name == "Budget")
            .unwrap()
            .value
            .as_deref(),
        Some("8")
    );

    // Clear is idempotent.
    fields::clear_value(&pool, task_id, budget.id)
        .await
        .unwrap();
    fields::clear_value(&pool, task_id, budget.id)
        .await
        .unwrap();
    let vals = fields::list_for_task(&pool, pid, task_id).await.unwrap();
    assert!(vals
        .iter()
        .find(|v| v.name == "Budget")
        .unwrap()
        .value
        .is_none());

    // Deleting a definition cascades its values.
    fields::set_value(&pool, task_id, budget.id, "8")
        .await
        .unwrap();
    fields::delete(&pool, budget.id, pid).await.unwrap();
    let count: i64 =
        sqlx::query_scalar(r#"SELECT count(*) FROM task_field_values WHERE field_id = $1"#)
            .bind(budget.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[sqlx::test(migrations = "./migrations")]
async fn filter_matching_intersects_pairs(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, bid, cid) = make_project_with_board(&pool, "FIL", owner).await;
    let (t1, _) = make_task(&pool, pid, bid, cid, 1024.0).await;
    let (t2, _) = make_task(&pool, pid, bid, cid, 2048.0).await;

    let sev = fields::create(&pool, pid, "Severity", "select", &opts(&["Low", "High"]))
        .await
        .unwrap();
    let budget = fields::create(&pool, pid, "Budget", "number", &[])
        .await
        .unwrap();

    fields::set_value(&pool, t1, sev.id, "High").await.unwrap();
    fields::set_value(&pool, t2, sev.id, "Low").await.unwrap();
    fields::set_value(&pool, t1, budget.id, "3.5")
        .await
        .unwrap();

    // Field name + select value match case-insensitively; raw values are
    // canonicalised ("3.50" → "3.5") before comparing.
    let hit = fields::matching_task_ids(
        &pool,
        pid,
        &[
            ("severity".into(), "high".into()),
            ("Budget".into(), "3.50".into()),
        ],
    )
    .await
    .unwrap();
    assert_eq!(hit.len(), 1);
    assert!(hit.contains(&t1));

    // Intersecting predicates that no single task satisfies → empty.
    let none = fields::matching_task_ids(
        &pool,
        pid,
        &[
            ("severity".into(), "low".into()),
            ("budget".into(), "3.5".into()),
        ],
    )
    .await
    .unwrap();
    assert!(none.is_empty());

    // Unknown field or unparseable value match nothing, not everything.
    assert!(
        fields::matching_task_ids(&pool, pid, &[("nope".into(), "x".into())])
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        fields::matching_task_ids(&pool, pid, &[("budget".into(), "many".into())])
            .await
            .unwrap()
            .is_empty()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn field_values_feed_task_search_tsv(pool: PgPool) {
    let owner = make_user(&pool).await;
    let (pid, bid, cid) = make_project_with_board(&pool, "TSV", owner).await;
    let (task_id, _) = make_task(&pool, pid, bid, cid, 1024.0).await;

    let env = fields::create(&pool, pid, "Environment", "text", &[])
        .await
        .unwrap();

    async fn matches(pool: &PgPool, task_id: Uuid) -> bool {
        sqlx::query_scalar::<_, bool>(
            r#"SELECT search_tsv @@ plainto_tsquery('simple', 'staging') FROM tasks WHERE id = $1"#,
        )
        .bind(task_id)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    assert!(!matches(&pool, task_id).await);
    fields::set_value(&pool, task_id, env.id, "staging")
        .await
        .unwrap();
    assert!(
        matches(&pool, task_id).await,
        "set value reindexes the task"
    );
    fields::clear_value(&pool, task_id, env.id).await.unwrap();
    assert!(!matches(&pool, task_id).await, "clear reindexes too");
}
