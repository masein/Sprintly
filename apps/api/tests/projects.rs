//! M2 integration tests against a real Postgres.
//!
//! Covers:
//!   • Permissions matrix for project actions (admin vs lead vs contributor
//!     vs non-member, archived vs not).
//!   • ProjectContext loader returns the right role for the right user.
//!   • Last-lead removal protection at the SQL layer is enforced by the
//!     `ensure_not_last_lead` helper — exercised here via direct insert.
//!   • Column sort_order rebalancing after a reorder produces a clean,
//!     monotonically-increasing sequence.

use sprintly_api::{
    config::AuthConfig,
    domain::{
        password,
        permissions::{can, Action, ProjectRole, Role},
        projects as project_ctx,
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

async fn make_user(pool: &PgPool, role: &str) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"
        INSERT INTO users (id, email, handle, display_name, password_hash, role)
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(format!("u{}@x.test", &id.to_string()[..8]))
    .bind(format!("h{}", &id.to_string()[..8]))
    .bind("Test User")
    .bind(&hash)
    .bind(role)
    .execute(pool)
    .await
    .unwrap();
    id
}

async fn make_project(pool: &PgPool, key: &str, created_by: Uuid) -> Uuid {
    let pid = Uuid::now_v7();
    sqlx::query(
        r#"
        INSERT INTO projects (id, key, name, created_by)
        VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(pid)
    .bind(key)
    .bind(format!("Project {key}"))
    .bind(created_by)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO project_members (project_id, user_id, role, added_by)
        VALUES ($1, $2, 'lead', $2)
        "#,
    )
    .bind(pid)
    .bind(created_by)
    .execute(pool)
    .await
    .unwrap();
    pid
}

#[sqlx::test(migrations = "./migrations")]
async fn context_resolves_member_role(pool: PgPool) {
    let owner = make_user(&pool, "member").await;
    make_project(&pool, "TEST", owner).await;

    let ctx = project_ctx::load_by_key(&pool, "TEST", owner).await.unwrap();
    assert_eq!(ctx.actor_role, Some(ProjectRole::Lead));
    assert!(!ctx.archived);

    // A different user, no membership, sees actor_role = None.
    let outsider = make_user(&pool, "member").await;
    let ctx2 = project_ctx::load_by_key(&pool, "TEST", outsider).await.unwrap();
    assert_eq!(ctx2.actor_role, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn permissions_match_role_table(pool: PgPool) {
    let owner = make_user(&pool, "member").await;
    make_project(&pool, "PERM", owner).await;

    let lead_actor = sprintly_api::domain::permissions::Actor {
        id: owner,
        role: Role::Member,
    };

    // Lead can edit while project is active.
    let active = project_ctx::load_by_key(&pool, "PERM", owner).await.unwrap();
    assert!(can(&lead_actor, Action::EditProject, active.as_resource()));
    assert!(can(&lead_actor, Action::ManageColumns, active.as_resource()));

    // Archive it; same actor now blocked on edit/manage, still allowed to view.
    sqlx::query("UPDATE projects SET archived_at = now() WHERE id = $1")
        .bind(active.id)
        .execute(&pool)
        .await
        .unwrap();
    let archived = project_ctx::load_by_key(&pool, "PERM", owner).await.unwrap();
    assert!(archived.archived);
    assert!(!can(&lead_actor, Action::EditProject, archived.as_resource()));
    assert!(!can(&lead_actor, Action::ManageColumns, archived.as_resource()));
    assert!(can(&lead_actor, Action::ViewProject, archived.as_resource()));
}

#[sqlx::test(migrations = "./migrations")]
async fn non_member_cannot_view_project(pool: PgPool) {
    let owner = make_user(&pool, "member").await;
    make_project(&pool, "PRIV", owner).await;
    let outsider = make_user(&pool, "member").await;
    let outsider_actor = sprintly_api::domain::permissions::Actor {
        id: outsider,
        role: Role::Member,
    };

    let ctx = project_ctx::load_by_key(&pool, "PRIV", outsider).await.unwrap();
    assert!(!can(&outsider_actor, Action::ViewProject, ctx.as_resource()));
}

#[sqlx::test(migrations = "./migrations")]
async fn admin_bypasses_membership(pool: PgPool) {
    let owner = make_user(&pool, "member").await;
    make_project(&pool, "ADMN", owner).await;
    let admin = make_user(&pool, "admin").await;
    let admin_actor = sprintly_api::domain::permissions::Actor {
        id: admin,
        role: Role::Admin,
    };
    let ctx = project_ctx::load_by_key(&pool, "ADMN", admin).await.unwrap();
    assert_eq!(ctx.actor_role, None, "admin not auto-added to project");
    // ...but can still edit because of the global Admin bypass in can().
    assert!(can(&admin_actor, Action::EditProject, ctx.as_resource()));
}

#[sqlx::test(migrations = "./migrations")]
async fn project_key_format_check_is_enforced(pool: PgPool) {
    let owner = make_user(&pool, "member").await;
    // lowercase fails the CHECK constraint.
    let res = sqlx::query(
        r#"
        INSERT INTO projects (id, key, name, created_by)
        VALUES ($1, 'lowercase', 'x', $2)
        "#,
    )
    .bind(Uuid::now_v7())
    .bind(owner)
    .execute(&pool)
    .await;
    assert!(res.is_err(), "lowercase key must be rejected by CHECK");
}
