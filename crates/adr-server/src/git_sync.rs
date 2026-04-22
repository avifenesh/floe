//! Shell-out to `git` for PR-URL-driven analyses.
//!
//! Given a GitHub repo URL + a commit sha, materialise a working
//! tree at `<root>/<owner>-<repo>/<sha>` that `adr-parse` can walk.
//! The strategy is intentionally dumb: per-sha shallow clones, one
//! `git init` + `fetch --depth=1 <sha>` + `checkout FETCH_HEAD`.
//!
//! Pros: no worktree gymnastics, no shared-bare-repo state, works on
//! Windows without hitting filesystem locks. Cons: we re-download
//! objects for base and head even though they likely share history —
//! acceptable because each side is ≤ 1 commit deep (typical PR < 50MB).
//!
//! Rerunning the same `(owner, repo, sha)` is a no-op: if the dest
//! exists we skip. Callers should use the cache dir's `repos/` root
//! so the clones survive restarts.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use tokio::process::Command;

/// Where to materialise checkouts. Lives under the server's cache
/// dir so we reuse the same housekeeping surface as artifact JSON.
pub fn repos_root(cache_dir: &Path) -> PathBuf {
    cache_dir.join("repos")
}

/// Destination path for a specific `(owner, repo, sha)` checkout.
/// Caller canonicalises after clone completes — we return the raw
/// path because the dir may not exist yet.
pub fn checkout_path(cache_dir: &Path, owner: &str, repo: &str, sha: &str) -> PathBuf {
    repos_root(cache_dir)
        .join(format!("{owner}-{repo}"))
        .join(sha)
}

/// Clone a single commit into `dest`. Idempotent: if `dest/.git`
/// exists we assume a prior run succeeded and return immediately.
/// `access_token` is embedded in the HTTPS URL when present — needed
/// for private repos; public repos work with `None`.
pub async fn clone_sha(
    owner: &str,
    repo: &str,
    sha: &str,
    access_token: Option<&str>,
    dest: &Path,
) -> Result<()> {
    if dest.join(".git").exists() {
        tracing::debug!(dest = %dest.display(), "checkout already present, reusing");
        return Ok(());
    }
    tokio::fs::create_dir_all(dest)
        .await
        .with_context(|| format!("create_dir_all {}", dest.display()))?;

    let remote = match access_token {
        Some(tok) => format!("https://x-access-token:{tok}@github.com/{owner}/{repo}.git"),
        None => format!("https://github.com/{owner}/{repo}.git"),
    };

    git(dest, &["init", "--quiet"]).await?;
    git(dest, &["remote", "add", "origin", &remote]).await?;
    // `--depth=1` keeps the clone tiny. `origin <sha>` works for any
    // commit GitHub's server exposes, including PR heads that aren't
    // on a branch (as long as `uploadpack.allowReachableSHA1InWant`
    // is on — which it is for GitHub).
    git(dest, &["fetch", "--depth=1", "origin", sha]).await?;
    git(dest, &["checkout", "--quiet", "FETCH_HEAD"]).await?;
    // Scrub the remote URL so the token doesn't sit in
    // `.git/config` after the clone. Best-effort — if it fails the
    // caller already has their checkout.
    let _ = git(
        dest,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("https://github.com/{owner}/{repo}.git"),
        ],
    )
    .await;
    Ok(())
}

async fn git(cwd: &Path, args: &[&str]) -> Result<()> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await
        .with_context(|| format!("spawn git {}", args.join(" ")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow!(
            "git {} failed ({}): {}",
            args.join(" "),
            out.status,
            stderr.trim()
        ));
    }
    Ok(())
}
