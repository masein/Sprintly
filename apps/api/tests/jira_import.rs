//! Integration tests for the native Jira importer (extends F16).
//!
//! A representative "all fields" CSV — an epic, a story, a sub-task, an
//! assignee (matched by email), a sprint, story points, multi-label rows,
//! comments, and a multi-line quoted description — maps to the right Sprintly
//! entities; a second identical import dedupes by Jira key (updates, no dupes).

use chrono::{DateTime, TimeZone, Utc};
use sprintly_api::{
    config::AuthConfig,
    domain::{import_export as ie, jira, password},
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

async fn make_user(pool: &PgPool, email: &str, display: &str) -> Uuid {
    let id = Uuid::now_v7();
    let hash = password::hash(&cfg(), "pw-pw-pw-pw").unwrap();
    sqlx::query(
        r#"INSERT INTO users (id, email, handle, display_name, password_hash, role)
           VALUES ($1, $2, $3, $4, $5, 'member')"#,
    )
    .bind(id)
    .bind(email)
    .bind(format!("h{}", id.simple()))
    .bind(display)
    .bind(&hash)
    .execute(pool)
    .await
    .unwrap();
    id
}

/// Project + default board with the three standard columns.
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
    for (name, cat, sort) in [
        ("To Do", "todo", 1024.0),
        ("In Progress", "in_progress", 2048.0),
        ("Done", "done", 3072.0),
    ] {
        sqlx::query(
            r#"INSERT INTO board_columns (id, board_id, name, category, sort_order)
               VALUES ($1, $2, $3, $4, $5)"#,
        )
        .bind(Uuid::now_v7())
        .bind(board)
        .bind(name)
        .bind(cat)
        .bind(sort)
        .execute(pool)
        .await
        .unwrap();
    }
    (pid, board)
}

// A representative Jira "Export Excel CSV (all fields)" slice. Note: repeated
// Labels columns, a multi-line quoted Description, a Comment cell in Jira's
// `date;author;body` shape, an Epic-type row, a Story linked to it, and a
// Sub-task pointing at its Parent.
const JIRA_CSV: &str = "Issue key,Issue Type,Summary,Description,Status,Priority,Assignee,Labels,Labels,Sprint,Story Points,Epic Link,Parent,Comment,Due date\n\
EPIC-1,Epic,Checkout revamp,,To Do,High,,,,,,,,,\n\
PROJ-10,Story,\"Build the cart\",\"first line\nsecond line\",In Progress,Highest,sam@x.test,backend,urgent,Sprint 7,5,EPIC-1,,\"12/Jan/24;Sam Adams;Looking good; ship soon\",5/Jul/26\n\
PROJ-11,Sub-task,\"Wire the totals\",,To Do,Low,Jo March,backend,,Sprint 7,2,,PROJ-10,,\n";

#[sqlx::test(migrations = "./migrations")]
async fn imports_full_jira_shape(pool: PgPool) {
    let owner = make_user(&pool, "owner@x.test", "Owner").await;
    let sam = make_user(&pool, "sam@x.test", "Sam Adams").await;
    let jo = make_user(&pool, "jo@x.test", "Jo March").await;
    let (pid, board) = make_project(&pool, "JIRA", owner).await;

    let plan = jira::parse_jira_csv(JIRA_CSV).unwrap();
    let report = ie::apply_jira_import(&pool, pid, board, &plan, false)
        .await
        .unwrap();

    assert_eq!(report.source, "jira");
    assert_eq!(report.tasks_created, 2, "epic is not a task");
    assert_eq!(report.epics_created, vec!["Checkout revamp"]);
    assert_eq!(report.sprints_created, vec!["Sprint 7"]);
    assert_eq!(report.fields_created, vec!["Story Points"]);
    assert_eq!(report.comments_created, 1);

    // The story: type/priority/status/assignee/labels/description/external_ref.
    let (title, ttype, prio, status, assignee, labels, desc, ext): (
        String,
        String,
        String,
        String,
        Option<Uuid>,
        Vec<String>,
        String,
        String,
    ) = sqlx::query_as(
        r#"SELECT title, type, priority, status, assignee_id, labels, description, external_ref
             FROM tasks WHERE project_id = $1 AND external_ref = 'PROJ-10'"#,
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(title, "Build the cart");
    assert_eq!(ttype, "feature"); // Story → feature
    assert_eq!(prio, "p0"); // Highest → p0
    assert_eq!(status, "in_progress"); // In Progress column
    assert_eq!(assignee, Some(sam)); // matched by email
    assert!(labels.contains(&"backend".to_string()) && labels.contains(&"urgent".to_string()));
    assert_eq!(desc, "first line\nsecond line"); // multi-line survived
    assert_eq!(ext, "PROJ-10");

    // Story is linked to the epic + the sprint.
    let (epic_name, sprint_name): (Option<String>, Option<String>) = sqlx::query_as(
        r#"SELECT e.name, s.name
             FROM tasks t
             LEFT JOIN epics e ON e.id = t.epic_id
             LEFT JOIN sprints s ON s.id = t.sprint_id
            WHERE t.project_id = $1 AND t.external_ref = 'PROJ-10'"#,
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(epic_name.as_deref(), Some("Checkout revamp"));
    assert_eq!(sprint_name.as_deref(), Some("Sprint 7"));

    // Story points landed in the number custom field (fractional-safe storage).
    let sp: String = sqlx::query_scalar(
        r#"SELECT v.value FROM task_field_values v
             JOIN tasks t ON t.id = v.task_id
             JOIN custom_fields f ON f.id = v.field_id
            WHERE t.external_ref = 'PROJ-10' AND f.name = 'Story Points'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sp, "5");

    // The sub-task points at its parent and matched Jo by display name.
    let (parent_ext, sub_assignee): (Option<String>, Option<Uuid>) = sqlx::query_as(
        r#"SELECT p.external_ref, c.assignee_id
             FROM tasks c
             LEFT JOIN tasks p ON p.id = c.parent_task_id
            WHERE c.project_id = $1 AND c.external_ref = 'PROJ-11'"#,
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(parent_ext.as_deref(), Some("PROJ-10"));
    assert_eq!(sub_assignee, Some(jo));

    // The comment carried its author + body.
    let (c_author, c_body): (Option<Uuid>, String) = sqlx::query_as(
        r#"SELECT c.author_id, c.body FROM task_comments c
             JOIN tasks t ON t.id = c.task_id
            WHERE t.external_ref = 'PROJ-10'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(c_author, Some(sam));
    assert_eq!(c_body, "Looking good; ship soon");
}

#[sqlx::test(migrations = "./migrations")]
async fn reimport_dedupes_by_jira_key(pool: PgPool) {
    let owner = make_user(&pool, "owner2@x.test", "Owner").await;
    make_user(&pool, "sam@x.test", "Sam Adams").await;
    make_user(&pool, "jo@x.test", "Jo March").await;
    let (pid, board) = make_project(&pool, "JRA", owner).await;

    let plan = jira::parse_jira_csv(JIRA_CSV).unwrap();
    ie::apply_jira_import(&pool, pid, board, &plan, false)
        .await
        .unwrap();

    // Re-import the same export: must update in place, not duplicate.
    let report2 = ie::apply_jira_import(&pool, pid, board, &plan, false)
        .await
        .unwrap();
    assert_eq!(report2.tasks_created, 0, "no new tasks on re-import");
    assert_eq!(report2.tasks_updated, 2, "both tasks matched + updated");
    assert_eq!(report2.comments_created, 0, "comments not re-added");

    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 2, "still two tasks");
    let epics: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM epics WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(epics, 1, "epic not duplicated");
    let comments: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_comments c JOIN tasks t ON t.id = c.task_id WHERE t.project_id = $1",
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(comments, 1, "comment not duplicated");
}

#[sqlx::test(migrations = "./migrations")]
async fn dry_run_writes_nothing_and_warns_unmatched(pool: PgPool) {
    let owner = make_user(&pool, "owner3@x.test", "Owner").await;
    // Note: no sam@x.test / Jo March users → both assignees go unmatched.
    let (pid, board) = make_project(&pool, "DRY", owner).await;

    let plan = jira::parse_jira_csv(JIRA_CSV).unwrap();
    let report = ie::apply_jira_import(&pool, pid, board, &plan, true)
        .await
        .unwrap();

    assert!(report.dry_run);
    assert_eq!(report.tasks_created, 2);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("no Sprintly match")),
        "unmatched assignees are warned: {:?}",
        report.warnings
    );

    // Nothing persisted.
    for table in ["tasks", "epics", "sprints", "custom_fields"] {
        let n: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE project_id = $1"
        ))
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(n, 0, "dry run wrote {table}");
    }
}

// A *team-managed* Jira export: the epic relationship is folded into the unified
// "Parent" column (no separate "Epic Link"), and one sub-task's parent is absent
// from the export. Also exercises a rich Sprint cell (state + window) and a
// comment from someone with no Sprintly account.
const HIERARCHY_CSV: &str = "Issue key,Issue Type,Summary,Status,Parent,Sprint,Comment\n\
EPIC-1,Epic,Checkout revamp,To Do,,,\n\
PROJ-1,Story,Build the cart,In Progress,EPIC-1,\"com.atlassian.greenhopper.service.sprint.Sprint@9[id=5,state=ACTIVE,name=Sprint 9,startDate=2024-02-01T09:00:00.000Z,endDate=2024-02-14T09:00:00.000Z]\",\"1/Feb/24 9:00 AM;Casey External;ext note\"\n\
PROJ-2,Sub-task,Wire the totals,To Do,PROJ-1,,\n\
PROJ-3,Sub-task,Orphaned bit,To Do,GHOST-9,,\n";

#[sqlx::test(migrations = "./migrations")]
async fn epic_via_parent_subtask_nesting_and_absent_parent(pool: PgPool) {
    let owner = make_user(&pool, "owner4@x.test", "Owner").await;
    let (pid, board) = make_project(&pool, "HIER", owner).await;

    let plan = jira::parse_jira_csv(HIERARCHY_CSV).unwrap();
    let report = ie::apply_jira_import(&pool, pid, board, &plan, false)
        .await
        .unwrap();

    assert_eq!(report.tasks_created, 3, "3 tasks (epic is not a task)");
    assert_eq!(report.epics_created, vec!["Checkout revamp"]);

    // J2: the Story's epic came via "Parent" (team-managed) → epic_id is set,
    // and it is NOT nested under the epic as a task.
    let (epic_name, parent_task): (Option<String>, Option<Uuid>) = sqlx::query_as(
        r#"SELECT e.name, t.parent_task_id
             FROM tasks t LEFT JOIN epics e ON e.id = t.epic_id
            WHERE t.project_id = $1 AND t.external_ref = 'PROJ-1'"#,
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(epic_name.as_deref(), Some("Checkout revamp"));
    assert_eq!(parent_task, None, "epic membership is not task nesting");

    // J1: the present-parent sub-task nests under PROJ-1.
    let nested_under: Option<String> = sqlx::query_scalar(
        r#"SELECT p.external_ref FROM tasks c
             JOIN tasks p ON p.id = c.parent_task_id
            WHERE c.project_id = $1 AND c.external_ref = 'PROJ-2'"#,
    )
    .bind(pid)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(nested_under.as_deref(), Some("PROJ-1"));

    // J1: the absent-parent sub-task stays top-level and is warned about.
    let orphan_parent: Option<Uuid> = sqlx::query_scalar(
        "SELECT parent_task_id FROM tasks WHERE project_id = $1 AND external_ref = 'PROJ-3'",
    )
    .bind(pid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(orphan_parent, None, "absent parent → top-level");
    assert!(
        report.warnings.iter().any(|w| w.contains("GHOST-9")),
        "missing parent is warned: {:?}",
        report.warnings
    );

    // Sprint carried its state + window from the rich cell.
    let (state, starts_at, started_at): (String, DateTime<Utc>, Option<DateTime<Utc>>) =
        sqlx::query_as(
            "SELECT state, starts_at, started_at FROM sprints WHERE project_id = $1 AND name = 'Sprint 9'",
        )
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "active");
    assert_eq!(
        starts_at,
        Utc.with_ymd_and_hms(2024, 2, 1, 9, 0, 0).unwrap()
    );
    assert!(started_at.is_some());

    // The comment from a non-user kept its attribution + Jira timestamp.
    let (c_author, c_body, c_created): (Option<Uuid>, String, DateTime<Utc>) = sqlx::query_as(
        r#"SELECT c.author_id, c.body, c.created_at FROM task_comments c
             JOIN tasks t ON t.id = c.task_id
            WHERE t.external_ref = 'PROJ-1'"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(c_author, None, "external author has no Sprintly account");
    assert!(
        c_body.contains("Casey External"),
        "attribution preserved: {c_body}"
    );
    assert!(c_body.contains("ext note"));
    assert_eq!(
        c_created,
        Utc.with_ymd_and_hms(2024, 2, 1, 9, 0, 0).unwrap()
    );

    // Idempotent re-import: updates in place, no new tasks/epics, links hold.
    let report2 = ie::apply_jira_import(&pool, pid, board, &plan, false)
        .await
        .unwrap();
    assert_eq!(report2.tasks_created, 0);
    assert_eq!(report2.tasks_updated, 3);
    let tasks: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(tasks, 3);
    let epics: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM epics WHERE project_id = $1")
        .bind(pid)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(epics, 1);
    // The nesting still holds after re-import.
    let still_nested: Option<String> = sqlx::query_scalar(
        r#"SELECT p.external_ref FROM tasks c
             JOIN tasks p ON p.id = c.parent_task_id
            WHERE c.project_id = $1 AND c.external_ref = 'PROJ-2'"#,
    )
    .bind(pid)
    .fetch_optional(&pool)
    .await
    .unwrap();
    assert_eq!(still_nested.as_deref(), Some("PROJ-1"));
}
