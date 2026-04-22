//! Persistent store for per-user PR analyses, backed by Turso's
//! [libsql] (SQLite-compatible).
//!
//! The server defaults to a file-backed DB at `.adr/adr.db` so
//! analyses survive restarts. `--in-memory` (or `ADR_DB=:memory:`)
//! opens a throwaway connection — useful for tests and smoke runs.
//!
//! v1 schema covers two concerns:
//!
//! 1. **`users`** — identities across providers (GitHub/GitLab/Google
//!    OAuth, magic-link email, or a local placeholder). Used to scope
//!    the PR list on the landing page.
//! 2. **`pr_analyses`** — one row per (user, PR, head-sha, intent,
//!    LLM-sig) tuple. Carries the cache-key triple needed to look up
//!    the artifact JSON on disk, plus status/progress so the landing
//!    page can show "running / ready / errored" without loading the
//!    (multi-MB) artifact.
//!
//! `user_id` is nullable in v1 — the server runs single-tenant until
//! slice 2 (OAuth) lands. Analyses created pre-auth can be claimed
//! later by a signed-in user.
//!
//! [libsql]: https://crates.io/crates/libsql

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use libsql::{params, Builder, Connection, Database};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Schema version baked into the `schema_version` pragma table.
/// Bump on breaking shape changes; additive changes stay on the
/// same major.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone)]
pub struct DbStore {
    // Wrapping in Arc<Mutex<_>> so clones share the same underlying
    // connection pool without contention on non-DB work. libsql's
    // `Connection` is not `Sync` internally — the mutex keeps the
    // API sane to consume from axum handler state.
    conn: Arc<Mutex<Connection>>,
    _db: Arc<Database>, // keep alive for the lifetime of the store
}

impl DbStore {
    /// Open a file-backed DB. Creates the parent directory if needed
    /// and runs migrations up to [`SCHEMA_VERSION`] idempotently.
    pub async fn open_file(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create db parent {}", parent.display()))?;
        }
        let db = Builder::new_local(path)
            .build()
            .await
            .with_context(|| format!("opening libsql db at {}", path.display()))?;
        Self::from_db(db).await
    }

    /// Open an in-memory DB — forgotten on server exit. Used by tests
    /// and by `--in-memory` smoke runs.
    pub async fn open_in_memory() -> Result<Self> {
        let db = Builder::new_local(":memory:")
            .build()
            .await
            .context("opening in-memory libsql db")?;
        Self::from_db(db).await
    }

    async fn from_db(db: Database) -> Result<Self> {
        let conn = db.connect().context("connecting to libsql")?;
        migrate(&conn).await?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            _db: Arc::new(db),
        })
    }

    /// Upsert an analysis row, keyed by `(head_sha, intent_fp,
    /// llm_sig)` — the triple that drives the artifact cache.
    /// `user_id` is optional (single-tenant mode).
    pub async fn upsert_analysis(&self, row: &AnalysisRow) -> Result<()> {
        let conn = self.conn.lock().await;
        conn.execute(
            r#"
            INSERT INTO pr_analyses (
                id, user_id, repo, pr_number, head_sha, intent_fp, llm_sig,
                artifact_key, status, message, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)
            ON CONFLICT(head_sha, intent_fp, llm_sig) DO UPDATE SET
                status = excluded.status,
                message = excluded.message,
                artifact_key = excluded.artifact_key,
                updated_at = excluded.updated_at
            "#,
            params![
                row.id.clone(),
                row.user_id.clone(),
                row.repo.clone(),
                row.pr_number,
                row.head_sha.clone(),
                row.intent_fp.clone(),
                row.llm_sig.clone(),
                row.artifact_key.clone(),
                row.status.to_string(),
                row.message.clone(),
                row.updated_at.clone(),
            ],
        )
        .await
        .context("upsert_analysis")?;
        Ok(())
    }

    /// Fetch the artifact-key for a given cache triple, if any. Used
    /// by the pipeline to short-circuit: if we've already produced an
    /// artifact for this (sha, intent, llm) we serve it from disk.
    pub async fn find_artifact_key(
        &self,
        head_sha: &str,
        intent_fp: &str,
        llm_sig: &str,
    ) -> Result<Option<String>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT artifact_key FROM pr_analyses \
                 WHERE head_sha = ?1 AND intent_fp = ?2 AND llm_sig = ?3 \
                 AND status = 'ready' \
                 LIMIT 1",
                params![head_sha, intent_fp, llm_sig],
            )
            .await
            .context("find_artifact_key query")?;
        let Some(row) = rows.next().await.context("find_artifact_key fetch")? else {
            return Ok(None);
        };
        let key: String = row.get(0).context("find_artifact_key read col")?;
        Ok(Some(key))
    }

    /// Mark any `pending` analyses older than `stale_after_minutes`
    /// as errored. Called on server boot: pending rows whose worker
    /// was killed (server restart, crash, `Ctrl+C`) otherwise hang in
    /// the sidebar forever with a misleading "running" badge.
    ///
    /// Returns the number of rows swept. Errors if the DB itself
    /// fails, but callers typically log + continue — a sweep failure
    /// shouldn't prevent boot.
    pub async fn sweep_stale_pending(&self, stale_after_minutes: i64) -> Result<u64> {
        let now = chrono::Utc::now();
        let cutoff = now - chrono::Duration::minutes(stale_after_minutes);
        let cutoff_iso = cutoff.to_rfc3339();
        let now_iso = now.to_rfc3339();
        let conn = self.conn.lock().await;
        let affected = conn
            .execute(
                "UPDATE pr_analyses \
                 SET status = 'errored', \
                     message = COALESCE(message, '') || \
                               CASE WHEN message IS NULL OR message = '' \
                                    THEN 'interrupted (server restart)' \
                                    ELSE ' · interrupted (server restart)' \
                               END, \
                     updated_at = ?1 \
                 WHERE status = 'pending' AND updated_at < ?2",
                params![now_iso, cutoff_iso],
            )
            .await
            .context("sweep_stale_pending update")?;
        Ok(affected)
    }

    /// Delete an analysis row by id. Dismiss-from-sidebar action.
    /// Returns the number of rows removed (0 if not found).
    pub async fn delete_analysis(&self, id: &str) -> Result<u64> {
        let conn = self.conn.lock().await;
        let n = conn
            .execute("DELETE FROM pr_analyses WHERE id = ?1", params![id])
            .await
            .context("delete_analysis")?;
        Ok(n)
    }

    /// Upsert a user row — on repeat sign-ins we refresh the profile
    /// fields (display name, avatar URL, email) and the OAuth access
    /// token but keep the same internal `user_id`. The token lets us
    /// call the provider's API on the user's behalf (fetch PR bodies,
    /// list repos). `(provider, provider_user_id)` is unique.
    /// Returns the internal `user_id`.
    pub async fn upsert_user(
        &self,
        provider: &str,
        provider_user_id: &str,
        email: Option<&str>,
        display_name: Option<&str>,
        avatar_url: Option<&str>,
        access_token: Option<&str>,
    ) -> Result<String> {
        let conn = self.conn.lock().await;
        let now = chrono::Utc::now().to_rfc3339();
        // Try to find existing first — if present, update profile + reuse id.
        let mut rows = conn
            .query(
                "SELECT id FROM users WHERE provider = ?1 AND provider_user_id = ?2 LIMIT 1",
                params![provider, provider_user_id],
            )
            .await
            .context("upsert_user lookup")?;
        if let Some(row) = rows.next().await.context("upsert_user fetch")? {
            let id: String = row.get(0).context("upsert_user id col")?;
            drop(rows);
            conn.execute(
                "UPDATE users \
                 SET email = ?1, \
                     display_name = ?2, \
                     avatar_url = ?3, \
                     access_token = COALESCE(?4, access_token), \
                     access_token_updated_at = \
                         CASE WHEN ?4 IS NOT NULL THEN ?5 ELSE access_token_updated_at END \
                 WHERE id = ?6",
                params![email, display_name, avatar_url, access_token, now.clone(), id.clone()],
            )
            .await
            .context("upsert_user update")?;
            return Ok(id);
        }
        // Insert fresh.
        let new_id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO users (id, provider, provider_user_id, email, display_name, avatar_url, access_token, access_token_updated_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                new_id.clone(),
                provider,
                provider_user_id,
                email,
                display_name,
                avatar_url,
                access_token,
                access_token.map(|_| now.clone()),
                now,
            ],
        )
        .await
        .context("upsert_user insert")?;
        Ok(new_id)
    }

    /// Fetch the stored OAuth access token for a user by internal id.
    /// Returns `None` if the user doesn't exist or never stored one.
    pub async fn find_access_token(&self, user_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT access_token FROM users WHERE id = ?1 LIMIT 1",
                params![user_id],
            )
            .await
            .context("find_access_token query")?;
        let Some(row) = rows.next().await.context("find_access_token fetch")? else {
            return Ok(None);
        };
        Ok(row.get::<String>(0).ok())
    }

    /// Fetch a user by internal id. Returns `None` when no row exists
    /// — signed-cookie user_ids that don't match a row land here
    /// (session refers to a deleted account; caller clears the cookie).
    pub async fn find_user(&self, id: &str) -> Result<Option<UserRow>> {
        let conn = self.conn.lock().await;
        let mut rows = conn
            .query(
                "SELECT id, provider, provider_user_id, email, display_name, avatar_url, created_at \
                 FROM users WHERE id = ?1 LIMIT 1",
                params![id],
            )
            .await
            .context("find_user query")?;
        let Some(row) = rows.next().await.context("find_user fetch")? else {
            return Ok(None);
        };
        Ok(Some(UserRow {
            id: row.get(0).context("id")?,
            provider: row.get(1).context("provider")?,
            provider_user_id: row.get(2).context("provider_user_id")?,
            email: row.get(3).ok(),
            display_name: row.get(4).ok(),
            avatar_url: row.get(5).ok(),
            created_at: row.get(6).context("created_at")?,
        }))
    }

    /// List recent analyses for a user (or all analyses if `user_id`
    /// is `None`). Newest first, capped at `limit`.
    pub async fn list_recent(
        &self,
        user_id: Option<&str>,
        limit: u32,
    ) -> Result<Vec<AnalysisRow>> {
        let conn = self.conn.lock().await;
        let sql = if user_id.is_some() {
            "SELECT id, user_id, repo, pr_number, head_sha, intent_fp, llm_sig, \
             artifact_key, status, message, created_at, updated_at \
             FROM pr_analyses WHERE user_id = ?1 \
             ORDER BY updated_at DESC LIMIT ?2"
        } else {
            "SELECT id, user_id, repo, pr_number, head_sha, intent_fp, llm_sig, \
             artifact_key, status, message, created_at, updated_at \
             FROM pr_analyses \
             ORDER BY updated_at DESC LIMIT ?2"
        };
        let mut rows = conn
            .query(sql, params![user_id.unwrap_or(""), limit])
            .await
            .context("list_recent query")?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().await.context("list_recent fetch")? {
            out.push(AnalysisRow {
                id: row.get(0).context("id")?,
                user_id: row.get(1).ok(),
                repo: row.get(2).ok(),
                pr_number: row.get(3).ok(),
                head_sha: row.get(4).context("head_sha")?,
                intent_fp: row.get(5).context("intent_fp")?,
                llm_sig: row.get(6).context("llm_sig")?,
                artifact_key: row.get(7).ok(),
                status: AnalysisStatus::parse(&row.get::<String>(8).context("status")?),
                message: row.get(9).ok(),
                created_at: row.get(10).context("created_at")?,
                updated_at: row.get(11).context("updated_at")?,
            });
        }
        Ok(out)
    }
}

async fn migrate(conn: &Connection) -> Result<()> {
    // Single-file migrations — keep idempotent (`IF NOT EXISTS`) so
    // restarting on an already-migrated file is a no-op.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );

        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            provider TEXT NOT NULL,
            provider_user_id TEXT NOT NULL,
            email TEXT,
            display_name TEXT,
            avatar_url TEXT,
            -- OAuth access token for the provider. Let us call the
            -- provider's API on the user's behalf (read PR bodies,
            -- list repos, etc.). Refreshed on every sign-in so an
            -- expired token gets replaced. Plain-text at rest for
            -- now — file-system permissions on `.adr/adr.db` gate
            -- access; encrypt when we deploy beyond local dev.
            access_token TEXT,
            access_token_updated_at TEXT,
            created_at TEXT NOT NULL,
            UNIQUE (provider, provider_user_id)
        );

        -- Idempotent column adds for existing DBs. `ALTER TABLE …
        -- ADD COLUMN` fails if the column exists; libsql doesn't yet
        -- support `IF NOT EXISTS` on column adds, so these run and
        -- may error — we ignore the error below.

        CREATE TABLE IF NOT EXISTS pr_analyses (
            id TEXT PRIMARY KEY,
            user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
            repo TEXT,
            pr_number INTEGER,
            head_sha TEXT NOT NULL,
            intent_fp TEXT NOT NULL,
            llm_sig TEXT NOT NULL,
            artifact_key TEXT,
            status TEXT NOT NULL,
            message TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE (head_sha, intent_fp, llm_sig)
        );

        CREATE INDEX IF NOT EXISTS idx_pr_analyses_user_updated
            ON pr_analyses (user_id, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_pr_analyses_repo_pr
            ON pr_analyses (repo, pr_number, updated_at DESC);
        "#,
    )
    .await
    .context("schema migration")?;

    // Idempotent column adds for DBs that predate the column. Each
    // ALTER is attempted independently; a "duplicate column" error
    // just means the column is already there, which is fine.
    for stmt in [
        "ALTER TABLE users ADD COLUMN access_token TEXT",
        "ALTER TABLE users ADD COLUMN access_token_updated_at TEXT",
    ] {
        if let Err(e) = conn.execute(stmt, ()).await {
            let msg = format!("{e}");
            if !msg.contains("duplicate column") {
                tracing::debug!(stmt, error = %msg, "column add skipped (already present or non-fatal)");
            }
        }
    }

    // Stamp schema version. Idempotent.
    conn.execute(
        "INSERT OR IGNORE INTO schema_version (version) VALUES (?1)",
        params![SCHEMA_VERSION as i64],
    )
    .await
    .context("stamp schema version")?;

    // Sanity check: fail if on-disk schema is newer than we know.
    let mut rows = conn
        .query("SELECT MAX(version) FROM schema_version", ())
        .await
        .context("read schema version")?;
    if let Some(row) = rows.next().await.context("schema version fetch")? {
        let max: i64 = row.get(0).unwrap_or(SCHEMA_VERSION as i64);
        if max > SCHEMA_VERSION as i64 {
            return Err(anyhow!(
                "db schema version {max} is newer than this binary's {SCHEMA_VERSION} — upgrade the server"
            ));
        }
    }

    Ok(())
}

/// Lifecycle state of one analysis row — mirrors the coarse job
/// lifecycle exposed on `GET /analyze/:id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnalysisStatus {
    Pending,
    Ready,
    Errored,
}

impl AnalysisStatus {
    fn parse(s: &str) -> Self {
        match s {
            "ready" => Self::Ready,
            "errored" => Self::Errored,
            _ => Self::Pending,
        }
    }
}

impl std::fmt::Display for AnalysisStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => f.write_str("pending"),
            Self::Ready => f.write_str("ready"),
            Self::Errored => f.write_str("errored"),
        }
    }
}

/// User row (returned by `find_user`). `provider` is the OAuth
/// provider name (`github`/`gitlab`/`google`); `provider_user_id` is
/// the provider's stable id for the user (e.g. GitHub's numeric id).
/// `email`/`display_name`/`avatar_url` refresh on re-login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRow {
    pub id: String,
    pub provider: String,
    pub provider_user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub created_at: String,
}

/// One row of `pr_analyses` — the landing-page list, cache lookup
/// index, and restart-recovery source in one table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisRow {
    /// Row id — we reuse the axum `Job` uuid so the job and its row
    /// share identity across restarts.
    pub id: String,
    /// Null until auth lands (slice 2). Once set, this row is scoped
    /// to that user's landing page list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// `owner/name` when sourced from a remote (GitHub/GitLab), or
    /// the local canonical path's final component. Free-form; not
    /// cache-key material.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<i64>,
    /// Content-addressed hash of the head snapshot — derived from
    /// `Artifact::snapshot_sha(Side::Head)`. The cache-key primary
    /// axis. Same value for identical head trees regardless of path.
    pub head_sha: String,
    /// Blake3 fingerprint of `(intent, notes)`.
    pub intent_fp: String,
    /// Provider+model+prompt-version for the analysis LLM stack —
    /// pins the regime used so a model change doesn't silently serve
    /// stale artifacts.
    pub llm_sig: String,
    /// Relative path of the artifact JSON inside the cache dir. Null
    /// for not-yet-ready rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_key: Option<String>,
    pub status: AnalysisStatus,
    /// Error message when `status = errored`. Human-readable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_iso() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    fn row(head_sha: &str, intent_fp: &str, status: AnalysisStatus) -> AnalysisRow {
        AnalysisRow {
            id: uuid::Uuid::new_v4().to_string(),
            user_id: None,
            repo: Some("glide-mq".into()),
            pr_number: Some(181),
            head_sha: head_sha.into(),
            intent_fp: intent_fp.into(),
            llm_sig: "glm:glm-4.7@v0.2.0".into(),
            artifact_key: Some("abc123".into()),
            status,
            message: None,
            created_at: now_iso(),
            updated_at: now_iso(),
        }
    }

    #[tokio::test]
    async fn in_memory_migrations_and_upsert_roundtrip() {
        let db = DbStore::open_in_memory().await.expect("open");
        let r = row("head-1", "fp-1", AnalysisStatus::Pending);
        db.upsert_analysis(&r).await.expect("upsert");

        let listed = db.list_recent(None, 10).await.expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].head_sha, "head-1");
        assert_eq!(listed[0].status, AnalysisStatus::Pending);
    }

    #[tokio::test]
    async fn upsert_is_idempotent_on_cache_triple() {
        let db = DbStore::open_in_memory().await.expect("open");
        let mut r = row("head-1", "fp-1", AnalysisStatus::Pending);
        db.upsert_analysis(&r).await.expect("first");
        // Second call with new status + message but same cache triple —
        // should update in-place, not insert a duplicate.
        r.status = AnalysisStatus::Ready;
        r.message = Some("done".into());
        r.updated_at = now_iso();
        db.upsert_analysis(&r).await.expect("second");

        let listed = db.list_recent(None, 10).await.expect("list");
        assert_eq!(listed.len(), 1, "should dedupe on (head_sha, intent_fp, llm_sig)");
        assert_eq!(listed[0].status, AnalysisStatus::Ready);
    }

    #[tokio::test]
    async fn find_artifact_key_only_returns_ready_rows() {
        let db = DbStore::open_in_memory().await.expect("open");
        let pending = row("head-1", "fp-1", AnalysisStatus::Pending);
        db.upsert_analysis(&pending).await.expect("pending");
        let got = db
            .find_artifact_key("head-1", "fp-1", "glm:glm-4.7@v0.2.0")
            .await
            .expect("query");
        assert!(got.is_none(), "pending rows shouldn't surface as cache hits");

        // Upgrade the same row to ready — now the lookup should hit.
        let mut ready = pending.clone();
        ready.status = AnalysisStatus::Ready;
        ready.artifact_key = Some("abc123".into());
        ready.updated_at = now_iso();
        db.upsert_analysis(&ready).await.expect("ready");
        let got = db
            .find_artifact_key("head-1", "fp-1", "glm:glm-4.7@v0.2.0")
            .await
            .expect("query");
        assert_eq!(got, Some("abc123".into()));
    }

    #[tokio::test]
    async fn list_recent_orders_newest_first() {
        let db = DbStore::open_in_memory().await.expect("open");
        let mut a = row("head-a", "fp-1", AnalysisStatus::Ready);
        a.updated_at = "2026-01-01T00:00:00Z".into();
        db.upsert_analysis(&a).await.unwrap();
        let mut b = row("head-b", "fp-1", AnalysisStatus::Ready);
        b.updated_at = "2026-06-01T00:00:00Z".into();
        db.upsert_analysis(&b).await.unwrap();
        let mut c = row("head-c", "fp-1", AnalysisStatus::Ready);
        c.updated_at = "2026-03-01T00:00:00Z".into();
        db.upsert_analysis(&c).await.unwrap();

        let listed = db.list_recent(None, 10).await.unwrap();
        assert_eq!(listed.len(), 3);
        assert_eq!(listed[0].head_sha, "head-b");
        assert_eq!(listed[1].head_sha, "head-c");
        assert_eq!(listed[2].head_sha, "head-a");
    }
}
