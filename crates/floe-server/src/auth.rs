//! OAuth authentication + signed-cookie sessions.
//!
//! Slice 2 scope: GitHub OAuth authorization-code flow. Sign-in path:
//!
//! 1. `GET /auth/github` — generate a CSRF-protected auth URL, stash
//!    the `state` token in a short-lived signed cookie, 302 to GitHub.
//! 2. `GET /auth/github/callback?code=...&state=...` — verify `state`
//!    matches the cookie, exchange the code for an access token, hit
//!    `api.github.com/user` to get the profile, upsert into `users`,
//!    replace the state cookie with a session cookie keyed on
//!    `user_id`. 302 back to the landing page.
//! 3. `GET /me` — reads the session cookie, returns the user row or 401.
//! 4. `POST /auth/logout` — clears the session cookie.
//!
//! All cookies are signed with `FLOE_SESSION_SECRET` (HMAC) so a
//! tampered cookie value is rejected on decode. Sessions are
//! stateless — no server-side session table in v1. If we need
//! revocation later we add a `sessions` row keyed on a random token
//! and flip lookups through it.
//!
//! No OAuth provider other than GitHub is wired in v1. Slice 3 adds
//! GitLab + Google behind the same `AuthProvider` trait-shaped API.

use std::sync::Arc;

use anyhow::anyhow;
use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Redirect, Response},
    Json,
};
use axum_extra::extract::cookie::{Cookie, Key, SameSite, SignedCookieJar};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use time::Duration as TimeDuration;

use crate::db::DbStore;

/// Cookie names. Kept short to minimise per-request bytes. `adr_s`
/// = active session (long-lived), `floe_os = OAuth state (short-lived,
/// cleared immediately after callback exchange).
pub const SESSION_COOKIE: &str = "floe_s";
pub const OAUTH_STATE_COOKIE: &str = "floe_os";

/// Session cookie TTL. 30 days — long enough to feel persistent,
/// short enough that a stolen cookie isn't a lifetime grant.
const SESSION_TTL_DAYS: i64 = 30;

/// OAuth-state cookie TTL. 10 minutes — user has to complete the
/// GitHub round-trip in that window or they'll land on the callback
/// with no state cookie and get rejected (safe, just requires a
/// re-click on "Sign in").
const OAUTH_STATE_TTL_MINUTES: i64 = 10;

/// App-wide auth config derived from env vars. `None` for a field
/// means that provider is disabled.
#[derive(Clone)]
pub struct AuthConfig {
    pub session_key: Key,
    pub github: Option<GithubConfig>,
    /// Where the FE landing page lives — used as the post-sign-in
    /// redirect target. Defaults to "/"; override with
    /// `FLOE_FRONTEND_URL` for non-local dev (e.g.
    /// `http://127.0.0.1:5173`).
    pub frontend_url: String,
}

#[derive(Clone)]
pub struct GithubConfig {
    pub client_id: ClientId,
    pub client_secret: ClientSecret,
    pub redirect_url: RedirectUrl,
}

impl AuthConfig {
    /// Build from env. Returns `Ok(None)` when `FLOE_SESSION_SECRET`
    /// is unset — auth is fully optional; missing secret disables
    /// every provider.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(secret) = std::env::var("FLOE_SESSION_SECRET").ok().filter(|s| !s.is_empty()) else {
            tracing::info!("FLOE_SESSION_SECRET unset — auth disabled");
            return Ok(None);
        };
        let key_bytes = hex::decode(&secret)
            .map_err(|e| anyhow!("FLOE_SESSION_SECRET must be hex (openssl rand -hex 32): {e}"))?;
        if key_bytes.len() < 32 {
            return Err(anyhow!(
                "FLOE_SESSION_SECRET too short ({} bytes) — need at least 32 bytes of entropy (64 hex chars)",
                key_bytes.len()
            ));
        }
        // `Key::from` requires 64 raw bytes (512-bit), but we accept
        // the common `openssl rand -hex 32` output (32 bytes). Expand
        // deterministically via blake3's XOF: same secret → same key
        // across restarts, no HKDF dep.
        let mut expanded = [0u8; 64];
        let mut reader = blake3::Hasher::new()
            .update(b"floe-session-key-v1|")
            .update(&key_bytes)
            .finalize_xof();
        reader.fill(&mut expanded);
        let session_key = Key::from(&expanded);

        let frontend_url = std::env::var("FLOE_FRONTEND_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:5173".into());

        // GitHub is optional — the server runs happily with no
        // provider configured; the FE just never sees a Sign-in
        // button.
        let github = match (
            std::env::var("GITHUB_OAUTH_CLIENT_ID").ok().filter(|s| !s.is_empty()),
            std::env::var("GITHUB_OAUTH_CLIENT_SECRET").ok().filter(|s| !s.is_empty()),
        ) {
            (Some(id), Some(secret)) => {
                let redirect = std::env::var("GITHUB_OAUTH_REDIRECT_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:8787/auth/github/callback".into());
                Some(GithubConfig {
                    client_id: ClientId::new(id),
                    client_secret: ClientSecret::new(secret),
                    redirect_url: RedirectUrl::new(redirect)
                        .map_err(|e| anyhow!("GITHUB_OAUTH_REDIRECT_URL invalid: {e}"))?,
                })
            }
            _ => {
                tracing::info!("GITHUB_OAUTH_CLIENT_ID / GITHUB_OAUTH_CLIENT_SECRET unset — github provider disabled");
                None
            }
        };

        Ok(Some(Self {
            session_key,
            github,
            frontend_url,
        }))
    }
}

/// What a request knows about the signed-in user. Extracted from
/// the session cookie on every authenticated route. `None` when the
/// cookie is missing or tampered.
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: String,
}

impl Session {
    /// Read the session cookie from a `SignedCookieJar`. Returns
    /// `None` when no cookie is present or the signature fails.
    pub fn from_jar(jar: &SignedCookieJar) -> Option<Self> {
        jar.get(SESSION_COOKIE)
            .map(|c| Self { user_id: c.value().to_string() })
    }
}

/// Build a configured `BasicClient` for GitHub. Kept inline in the
/// per-request handler rather than cached on AppState — `BasicClient`
/// is cheap to construct.
fn github_client(cfg: &GithubConfig) -> BasicClient {
    BasicClient::new(
        cfg.client_id.clone(),
        Some(cfg.client_secret.clone()),
        AuthUrl::new("https://github.com/login/oauth/authorize".into())
            .expect("github auth url"),
        Some(
            TokenUrl::new("https://github.com/login/oauth/access_token".into())
                .expect("github token url"),
        ),
    )
    .set_redirect_uri(cfg.redirect_url.clone())
}

// ─────────────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────────────

/// GET `/auth/github` — kick off the OAuth dance.
pub async fn start_github(
    State(auth): State<Arc<AuthConfig>>,
    jar: SignedCookieJar,
) -> Result<(SignedCookieJar, Redirect), (StatusCode, String)> {
    let Some(gh) = auth.github.as_ref() else {
        return Err((StatusCode::NOT_FOUND, "github provider not configured".into()));
    };
    let client = github_client(gh);
    let (authorize_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        // `read:user` + `user:email` for profile/email.
        // `public_repo` lets the server read PR bodies + diff metadata
        // on public repos — enough for the "paste a GitHub PR URL,
        // get analysis + auto-populated intent" flow. Private repo
        // access needs the full `repo` scope; deferred until the
        // product promises to handle private code.
        .add_scope(Scope::new("read:user".into()))
        .add_scope(Scope::new("user:email".into()))
        .add_scope(Scope::new("public_repo".into()))
        .url();

    // Stash the CSRF state in a signed short-lived cookie so the
    // callback can verify it's talking to the same browser session
    // that started the flow.
    let state_cookie = build_cookie(
        OAUTH_STATE_COOKIE,
        csrf_token.secret().clone(),
        TimeDuration::minutes(OAUTH_STATE_TTL_MINUTES),
    );
    let jar = jar.add(state_cookie);
    Ok((jar, Redirect::temporary(authorize_url.as_str())))
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    pub code: String,
    pub state: String,
    /// GitHub sends `error`, `error_description`, and `error_uri` when
    /// the user cancelled or the app was revoked. We surface as a
    /// 400 with the description.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub error_description: Option<String>,
}

/// GET `/auth/github/callback?code=X&state=Y`.
pub async fn github_callback(
    State(auth): State<Arc<AuthConfig>>,
    State(db): State<DbStore>,
    Query(q): Query<CallbackQuery>,
    jar: SignedCookieJar,
) -> Result<(SignedCookieJar, Redirect), (StatusCode, String)> {
    if let Some(err) = q.error.as_deref() {
        let detail = q.error_description.as_deref().unwrap_or("");
        return Err((
            StatusCode::BAD_REQUEST,
            format!("github rejected the auth: {err} — {detail}"),
        ));
    }
    let Some(gh) = auth.github.as_ref() else {
        return Err((StatusCode::NOT_FOUND, "github provider not configured".into()));
    };
    // Verify CSRF state — cookie must match the query param. Missing
    // cookie means either the user took longer than the TTL or
    // something weird's going on; either way reject.
    let expected_state = jar
        .get(OAUTH_STATE_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "missing or expired oauth state cookie — retry the sign-in flow".into(),
            )
        })?;
    if expected_state != q.state {
        return Err((
            StatusCode::BAD_REQUEST,
            "oauth state mismatch — suspected csrf, aborting".into(),
        ));
    }

    let client = github_client(gh);
    let token = client
        .exchange_code(AuthorizationCode::new(q.code))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("github token exchange failed: {e}"),
            )
        })?;

    let profile = fetch_github_profile(token.access_token().secret())
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("github profile fetch failed: {e:#}"),
            )
        })?;

    let user_id = db
        .upsert_user(
            "github",
            &profile.id.to_string(),
            profile.email.as_deref(),
            Some(profile.display_name()),
            profile.avatar_url.as_deref(),
            // Save the OAuth access token so the server can call
            // GitHub's API on the user's behalf (fetch PR bodies,
            // list repos). Refreshed on every sign-in.
            Some(token.access_token().secret()),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("upsert user: {e:#}")))?;

    // Swap: drop the OAuth-state cookie, set the session cookie.
    let jar = jar
        .remove(Cookie::from(OAUTH_STATE_COOKIE))
        .add(build_cookie(
            SESSION_COOKIE,
            user_id,
            TimeDuration::days(SESSION_TTL_DAYS),
        ));
    Ok((jar, Redirect::temporary(&auth.frontend_url)))
}

/// GET `/me` — returns the currently signed-in user, or 401 if no
/// session cookie. Used by the FE to decide whether to show the
/// "Sign in" button or the user badge.
pub async fn me(
    State(db): State<DbStore>,
    jar: SignedCookieJar,
) -> Result<Json<crate::db::UserRow>, (StatusCode, String)> {
    let Some(session) = Session::from_jar(&jar) else {
        return Err((StatusCode::UNAUTHORIZED, "not signed in".into()));
    };
    let row = db
        .find_user(&session.user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("find_user: {e:#}")))?
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "session user not found".into()))?;
    Ok(Json(row))
}

#[derive(Debug, Deserialize)]
pub struct DevLoginRequest {
    pub handle: String,
}

/// POST `/auth/dev/login` — dev-only fake sign-in. Upserts a user under
/// the `dev` provider keyed by the supplied handle and drops a session
/// cookie. **Only mounted when `FLOE_ALLOW_DEV_LOGIN=1`.** Lets us test
/// signed-in flows without the full GitHub round-trip; never ship this
/// route enabled in production.
pub async fn dev_login(
    State(db): State<DbStore>,
    jar: SignedCookieJar,
    Json(req): Json<DevLoginRequest>,
) -> Result<(SignedCookieJar, Json<crate::db::UserRow>), (StatusCode, String)> {
    let handle = req.handle.trim();
    if handle.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "handle required".into()));
    }
    let user_id = db
        .upsert_user(
            "dev",
            handle,
            None,
            Some(handle),
            None,
            // No GitHub token for dev users; URL-driven analyse will
            // 403 for them but local-paths still work fine.
            None,
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("upsert: {e:#}")))?;
    let row = db
        .find_user(&user_id)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("find_user: {e:#}")))?
        .ok_or((StatusCode::INTERNAL_SERVER_ERROR, "user just upserted not found".into()))?;
    let jar = jar.add(build_cookie(
        SESSION_COOKIE,
        user_id,
        TimeDuration::days(SESSION_TTL_DAYS),
    ));
    Ok((jar, Json(row)))
}

/// POST `/auth/logout` — clears the session cookie. Idempotent; safe
/// to call when no session exists.
pub async fn logout(jar: SignedCookieJar) -> (SignedCookieJar, Response) {
    let jar = jar.remove(Cookie::from(SESSION_COOKIE));
    (jar, ([(header::CONTENT_TYPE, "application/json")], "{\"ok\":true}").into_response())
}

// ─────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────

/// Construct a cookie with our default security attributes. Dev
/// friendly (no `Secure` because we're on http://127.0.0.1); when
/// Session cookie: `SameSite=None; Secure; HttpOnly`. Browsers treat
/// `localhost` and `127.0.0.1` as secure contexts, so `Secure` is
/// accepted even over HTTP in dev. `None` (+ Secure) is required for
/// credentialed cross-origin XHR from the vite dev server on :5173
/// to the backend on :8787; otherwise the browser silently strips
/// the cookie and `/me` keeps returning 401 after sign-in.
fn build_cookie(name: &str, value: String, max_age: TimeDuration) -> Cookie<'static> {
    Cookie::build((name.to_string(), value))
        .http_only(true)
        .same_site(SameSite::None)
        .secure(true)
        .path("/")
        .max_age(max_age)
        .build()
}

/// GitHub user profile we extract from `GET api.github.com/user`.
/// Kept minimal — only the fields we put on the `users` row.
#[derive(Debug, Deserialize)]
struct GithubProfile {
    id: i64,
    login: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

impl GithubProfile {
    fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.login)
    }
}

/// Basic PR metadata we pull server-side once the user pastes a
/// GitHub PR URL. Populated via `api.github.com/repos/:owner/:name/pulls/:n`
/// using the authenticated user's stored OAuth access token.
/// `body` becomes the raw-text intent; `head.sha` + `base.sha` feed
/// the worktree clone path; everything else decorates the UI.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubPr {
    pub number: i64,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    pub state: String,
    pub head: GithubPrRef,
    pub base: GithubPrRef,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubPrRef {
    #[serde(rename = "ref")]
    pub ref_name: String,
    pub sha: String,
}

/// Fetch PR metadata using a caller-supplied token. The server
/// resolves tokens from the logged-in user's row (see
/// [`crate::db::DbStore::find_access_token`]); this function is
/// deliberately naive about auth so tests can inject tokens without
/// DB.
pub async fn fetch_github_pr(
    access_token: &str,
    owner: &str,
    repo: &str,
    number: u64,
) -> anyhow::Result<GithubPr> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}");
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .bearer_auth(access_token)
        .header("User-Agent", "floe-server")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("github PR fetch {status}: {body}"));
    }
    let pr: GithubPr = resp.json().await?;
    Ok(pr)
}

/// Parse a GitHub PR URL into `(owner, repo, number)`. Accepts the
/// canonical `https://github.com/owner/repo/pull/123` form plus the
/// `www.` variant. Rejects anything else — callers get a typed error
/// they can surface to the UI.
pub fn parse_github_pr_url(url: &str) -> anyhow::Result<(String, String, u64)> {
    let u = url::Url::parse(url.trim()).map_err(|e| anyhow!("invalid URL: {e}"))?;
    let host = u.host_str().unwrap_or("");
    if !matches!(host, "github.com" | "www.github.com") {
        return Err(anyhow!("not a github.com URL: {host}"));
    }
    let segments: Vec<&str> = u.path_segments().map(|s| s.collect()).unwrap_or_default();
    // Expected: /owner/repo/pull/123 (optionally /files, /commits, etc. trailing — ignored)
    if segments.len() < 4 || segments[2] != "pull" {
        return Err(anyhow!(
            "not a PR URL: expected /owner/repo/pull/<n>, got {:?}",
            segments
        ));
    }
    let owner = segments[0].to_string();
    let repo = segments[1].to_string();
    let number: u64 = segments[3]
        .parse()
        .map_err(|e| anyhow!("bad PR number `{}`: {e}", segments[3]))?;
    Ok((owner, repo, number))
}

async fn fetch_github_profile(access_token: &str) -> anyhow::Result<GithubProfile> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/user")
        .bearer_auth(access_token)
        .header("User-Agent", "floe-server")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("github /user responded {status}: {body}"));
    }
    let profile: GithubProfile = resp.json().await?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_pr_url() {
        let (o, r, n) = parse_github_pr_url("https://github.com/avifenesh/glide-mq/pull/181").unwrap();
        assert_eq!(o, "avifenesh");
        assert_eq!(r, "glide-mq");
        assert_eq!(n, 181);
    }

    #[test]
    fn parses_pr_url_with_trailing_subpath() {
        let (o, r, n) = parse_github_pr_url(
            "https://github.com/avifenesh/glide-mq/pull/181/files",
        )
        .unwrap();
        assert_eq!(o, "avifenesh");
        assert_eq!(r, "glide-mq");
        assert_eq!(n, 181);
    }

    #[test]
    fn rejects_non_github_host() {
        assert!(parse_github_pr_url("https://gitlab.com/x/y/pull/1").is_err());
    }

    #[test]
    fn rejects_non_pr_path() {
        assert!(parse_github_pr_url("https://github.com/x/y/issues/1").is_err());
    }

    #[test]
    fn build_cookie_is_none_samesite_secure_http_only() {
        let c = build_cookie("k", "v".into(), TimeDuration::minutes(5));
        assert_eq!(c.name(), "k");
        assert_eq!(c.value(), "v");
        // SameSite=None + Secure is required for credentialed
        // cross-origin XHR (vite dev 5173 → backend 8787). Browsers
        // accept Secure over HTTP on localhost/127.0.0.1.
        assert_eq!(c.same_site(), Some(SameSite::None));
        assert_eq!(c.secure(), Some(true));
        assert_eq!(c.http_only(), Some(true));
        assert_eq!(c.path(), Some("/"));
    }
}
