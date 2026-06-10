//! F7 — project label registry CRUD.

use sprintly_api::domain::labels;
use sqlx::PgPool;
use uuid::Uuid;

async fn make_project(pool: &PgPool) -> Uuid {
    let owner = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role, status)
           VALUES ($1, $2, $3, 'T', 'x', 'member', 'active')"#,
    )
    .bind(owner)
    .bind(format!("{}@x.test", owner.simple()))
    .bind(format!("u{}", &owner.simple().to_string()[..10]))
    .execute(pool)
    .await
    .unwrap();
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'LBL', 'Labels', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    pid
}

#[sqlx::test(migrations = "./migrations")]
async fn crud_and_case_insensitive_uniqueness(pool: PgPool) {
    let pid = make_project(&pool).await;

    let a = labels::create(&pool, pid, "backend", "#ff0000")
        .await
        .unwrap();
    assert_eq!(a.name, "backend");

    // Case-insensitive duplicate name → conflict.
    assert!(labels::create(&pool, pid, "BACKEND", "#00ff00")
        .await
        .is_err());

    assert_eq!(labels::list(&pool, pid).await.unwrap().len(), 1);

    let updated = labels::update(&pool, a.id, pid, None, Some("#0000ff"))
        .await
        .unwrap();
    assert_eq!(updated.color, "#0000ff");

    labels::delete(&pool, a.id, pid).await.unwrap();
    assert_eq!(labels::list(&pool, pid).await.unwrap().len(), 0);

    // Deleting again → NotFound.
    assert!(labels::delete(&pool, a.id, pid).await.is_err());
}
