//! Backup retention + restore support (F15). Builds on the existing
//! `create_backup` worker job: this module adds the retention *policy* (which
//! completed backups to prune) and small DB helpers. The pruning I/O (deleting
//! the MinIO object) and the scheduled-backup tick live in the worker; the
//! restore one-shot lives in the CLI.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppResult;

/// Retention policy, read from env. Pruning runs only when at least one bound
/// is set — an unconfigured instance keeps every backup forever (the old
/// behaviour).
#[derive(Debug, Clone, Copy, Default)]
pub struct RetentionPolicy {
    /// Keep at most this many of the most recent completed backups.
    pub keep_count: Option<usize>,
    /// Keep backups newer than this many days.
    pub keep_days: Option<i64>,
}

impl RetentionPolicy {
    pub fn from_env() -> Self {
        Self {
            keep_count: env_var("SPRINTLY_BACKUP_RETENTION_COUNT").and_then(|v| v.parse().ok()),
            keep_days: env_var("SPRINTLY_BACKUP_RETENTION_DAYS").and_then(|v| v.parse().ok()),
        }
    }

    pub fn is_active(&self) -> bool {
        self.keep_count.is_some() || self.keep_days.is_some()
    }
}

/// How often (seconds) the worker should auto-create a backup. `None` (or a
/// non-positive value) disables scheduled backups — manual still works.
pub fn schedule_secs() -> Option<i64> {
    env_var("SPRINTLY_BACKUP_SCHEDULE_SECS")
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|n| *n > 0)
}

#[derive(Debug, Clone)]
pub struct BackupMeta {
    pub id: Uuid,
    pub storage_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Choose which completed backups to prune under `policy`, given `done` sorted
/// newest-first. A backup is **kept** if it is within `keep_count` of the
/// newest OR newer than `keep_days`; everything else is pruned. We always keep
/// at least the single most recent backup as a safety floor, so an aggressive
/// `keep_days` can never leave you with nothing.
pub fn select_prunable<'a>(
    done: &'a [BackupMeta],
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
) -> Vec<&'a BackupMeta> {
    let floor = policy.keep_count.unwrap_or(1).max(1);
    done.iter()
        .enumerate()
        .filter(|(rank, b)| {
            let within_count = *rank < floor;
            let within_days = policy
                .keep_days
                .map(|d| now - b.created_at <= Duration::days(d))
                .unwrap_or(false);
            !(within_count || within_days)
        })
        .map(|(_, b)| b)
        .collect()
}

/// All completed backups, newest first.
pub async fn load_done_backups(db: &PgPool) -> AppResult<Vec<BackupMeta>> {
    let rows = sqlx::query_as::<_, (Uuid, Option<String>, DateTime<Utc>)>(
        r#"SELECT id, storage_key, created_at
             FROM backups
            WHERE status = 'done'
            ORDER BY created_at DESC"#,
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, storage_key, created_at)| BackupMeta {
            id,
            storage_key,
            created_at,
        })
        .collect())
}

pub async fn delete_backup_row(db: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM backups WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// The storage key of a completed backup, for restore. `None` if the id is
/// unknown or the backup never finished.
pub async fn storage_key_of(db: &PgPool, id: Uuid) -> AppResult<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT storage_key FROM backups WHERE id = $1 AND status = 'done'")
            .bind(id)
            .fetch_optional(db)
            .await?
            .flatten(),
    )
}

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(rank_days_old: i64, now: DateTime<Utc>) -> BackupMeta {
        BackupMeta {
            id: Uuid::now_v7(),
            storage_key: Some(format!("backups/x/{rank_days_old}.dump")),
            created_at: now - Duration::days(rank_days_old),
        }
    }

    // Newest-first list of backups aged 0,1,2,…,n-1 days.
    fn series(n: i64, now: DateTime<Utc>) -> Vec<BackupMeta> {
        (0..n).map(|d| meta(d, now)).collect()
    }

    fn now() -> DateTime<Utc> {
        "2026-06-13T00:00:00Z".parse().unwrap()
    }

    #[test]
    fn keep_count_only_prunes_beyond_n() {
        let s = series(5, now());
        let policy = RetentionPolicy {
            keep_count: Some(2),
            keep_days: None,
        };
        let pruned = select_prunable(&s, &policy, now());
        // Keep ranks 0,1; prune ranks 2,3,4.
        assert_eq!(pruned.len(), 3);
        assert!(pruned
            .iter()
            .all(|b| b.created_at <= now() - Duration::days(2)));
    }

    #[test]
    fn keep_days_only_prunes_old_but_floors_at_one() {
        let s = series(5, now()); // ages 0..4 days
        let policy = RetentionPolicy {
            keep_count: None,
            keep_days: Some(2),
        };
        let pruned = select_prunable(&s, &policy, now());
        // Within 2 days: ages 0,1,2 kept; ages 3,4 pruned.
        assert_eq!(pruned.len(), 2);

        // Even if EVERYTHING is older than the window, the newest survives.
        let old = series(3, now())
            .into_iter()
            .map(|mut b| {
                b.created_at -= Duration::days(30);
                b
            })
            .collect::<Vec<_>>();
        let pruned = select_prunable(&old, &policy, now());
        assert_eq!(pruned.len(), 2, "floor keeps the single most recent");
    }

    #[test]
    fn both_bounds_keep_union() {
        let s = series(10, now());
        let policy = RetentionPolicy {
            keep_count: Some(2),
            keep_days: Some(5),
        };
        let pruned = select_prunable(&s, &policy, now());
        // Kept = within 2 newest (ranks 0,1) OR within 5 days (ages 0..5).
        // So ages 0..5 kept (6 of them), ages 6..9 pruned (4).
        assert_eq!(pruned.len(), 4);
    }

    #[test]
    fn empty_and_inactive() {
        assert!(select_prunable(&[], &RetentionPolicy::default(), now()).is_empty());
        // Default policy is inactive — callers skip pruning entirely.
        assert!(!RetentionPolicy::default().is_active());
        assert!(RetentionPolicy {
            keep_count: Some(1),
            keep_days: None
        }
        .is_active());
    }
}
