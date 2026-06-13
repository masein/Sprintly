//! F8 — saved board views: CRUD + access scoping (own + shared visibility,
//! owner-only mutation).

use serde_json::json;
use sprintly_api::domain::board_views;
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
    .bind(format!("u{}", id.simple()))
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project(pool: &PgPool, owner: Uuid) -> Uuid {
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"INSERT INTO projects (id, key, name, created_by) VALUES ($1, 'BV', 'Views', $2)"#,
    )
    .bind(pid)
    .bind(owner)
    .execute(pool)
    .await
    .unwrap();
    pid
}

#[sqlx::test(migrations = "./migrations")]
async fn create_round_trips_filter_and_grouping(pool: PgPool) {
    let owner = make_user(&pool).await;
    let pid = make_project(&pool, owner).await;

    let filter =
        json!([{ "key": "assignee", "value": "me" }, { "key": "label", "value": "backend" }]);
    let v = board_views::create(&pool, pid, owner, "My focus", &filter, "assignee", false)
        .await
        .unwrap();
    assert_eq!(v.name, "My focus");
    assert_eq!(v.group_by, "assignee");
    assert!(!v.shared);
    assert!(v.is_mine);
    // The opaque filter round-trips byte-for-byte.
    assert_eq!(v.filter, filter);

    let listed = board_views::list(&pool, pid, owner).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].filter, filter);
}

#[sqlx::test(migrations = "./migrations")]
async fn visibility_is_own_plus_shared(pool: PgPool) {
    let alice = make_user(&pool).await;
    let bob = make_user(&pool).await;
    let pid = make_project(&pool, alice).await;

    let empty = json!([]);
    board_views::create(&pool, pid, alice, "alice-private", &empty, "none", false)
        .await
        .unwrap();
    board_views::create(&pool, pid, alice, "alice-shared", &empty, "priority", true)
        .await
        .unwrap();
    board_views::create(&pool, pid, bob, "bob-private", &empty, "none", false)
        .await
        .unwrap();

    // Alice sees both of hers; not bob's private one.
    let alice_sees = board_views::list(&pool, pid, alice).await.unwrap();
    let names: Vec<&str> = alice_sees.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, vec!["alice-private", "alice-shared"]);
    assert!(alice_sees.iter().all(|v| v.is_mine));

    // Bob sees his own + alice's shared one, with is_mine set correctly.
    let bob_sees = board_views::list(&pool, pid, bob).await.unwrap();
    let bob_names: Vec<&str> = bob_sees.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(bob_names, vec!["alice-shared", "bob-private"]);
    let shared = bob_sees.iter().find(|v| v.name == "alice-shared").unwrap();
    assert!(!shared.is_mine, "alice's view is not bob's to edit");
}

#[sqlx::test(migrations = "./migrations")]
async fn mutation_is_owner_scoped(pool: PgPool) {
    let alice = make_user(&pool).await;
    let bob = make_user(&pool).await;
    let pid = make_project(&pool, alice).await;

    let v = board_views::create(&pool, pid, alice, "v", &json!([]), "none", true)
        .await
        .unwrap();

    // Bob can't edit or delete alice's view — NotFound, not a silent success.
    assert!(
        board_views::update(&pool, v.id, bob, Some("hax"), None, None, None)
            .await
            .is_err()
    );
    assert!(board_views::delete(&pool, v.id, bob).await.is_err());

    // Alice can.
    let updated = board_views::update(
        &pool,
        v.id,
        alice,
        Some("renamed"),
        Some(&json!([{ "key": "priority", "value": "p0" }])),
        Some("label"),
        Some(false),
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.group_by, "label");
    assert!(!updated.shared);
    assert_eq!(updated.filter[0]["value"], "p0");

    board_views::delete(&pool, v.id, alice).await.unwrap();
    assert!(board_views::list(&pool, pid, alice)
        .await
        .unwrap()
        .is_empty());
    // Deleting again → NotFound.
    assert!(board_views::delete(&pool, v.id, alice).await.is_err());
}
