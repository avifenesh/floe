//! Built-in demo PRs exposed to anonymous testers.
//!
//! The landing page's "Try a sample" gallery reads from [`Samples`],
//! which the server builds at startup by walking a fixtures root
//! (defaults to `<workspace>/fixtures`). Each `pr-XXXX-<slug>/` dir
//! with a `base/` + `head/` subtree becomes one [`Sample`].
//!
//! Why server-resolved and not frontend-hardcoded:
//!
//! 1. **Paths are absolute on disk.** The frontend never sees them,
//!    so there's nothing to sanitize — the `/analyze/sample/:id`
//!    endpoint looks the sample up by id and feeds the existing
//!    `run_pipeline` with paths it resolved itself.
//! 2. **Per-host portability.** A self-hoster's `fixtures/` lives at
//!    their own path; the server figures it out once at startup
//!    (`ADR_SAMPLES_ROOT` overrides).
//! 3. **Graceful absence.** If the fixtures dir is missing (a
//!    bare-bones deploy without the repo), `Samples::load` yields an
//!    empty list; `/samples` returns `[]`; the landing gallery
//!    hides itself. No error page, no dead links.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One demo PR the landing page can offer. Title + description come
/// from `meta.json` when present (optional); otherwise we derive a
/// terse title from the directory name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Sample {
    /// Stable identifier — the directory name, e.g. `pr-0001-add-retry`.
    /// Survives cosmetic edits to title/description.
    pub id: String,
    /// Reviewer-facing title, one line.
    pub title: String,
    /// What this PR demonstrates, one sentence. The landing card
    /// shows this verbatim.
    pub description: String,
    /// Absolute path to the base-side snapshot.
    #[serde(skip)]
    pub base: PathBuf,
    /// Absolute path to the head-side snapshot.
    #[serde(skip)]
    pub head: PathBuf,
}

/// Public view served by `GET /samples` — no paths, id + titles only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SampleView {
    pub id: String,
    pub title: String,
    pub description: String,
}

impl From<&Sample> for SampleView {
    fn from(s: &Sample) -> Self {
        Self {
            id: s.id.clone(),
            title: s.title.clone(),
            description: s.description.clone(),
        }
    }
}

/// Optional `meta.json` sitting alongside `base/` + `head/` in a
/// sample directory. Any missing field falls back to a derived
/// default, so the file is always optional.
#[derive(Debug, Default, Deserialize)]
struct SampleMeta {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// The loaded sample set. Empty when the fixtures dir isn't present.
#[derive(Debug, Clone, Default)]
pub struct Samples {
    pub samples: Vec<Sample>,
}

impl Samples {
    /// Walk `root` and return any `pr-*/` subdirectory that carries a
    /// `base/` + `head/` pair. Sorted by id so the landing gallery
    /// is deterministic across restarts.
    pub fn load(root: &Path) -> Self {
        let Ok(entries) = std::fs::read_dir(root) else {
            tracing::info!(
                root = %root.display(),
                "samples root absent or unreadable — demo gallery will be empty",
            );
            return Self::default();
        };
        let mut out: Vec<Sample> = Vec::new();
        for entry in entries.flatten() {
            let ty = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if !ty.is_dir() {
                continue;
            }
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if !name.starts_with("pr-") {
                continue;
            }
            let dir = entry.path();
            let base = dir.join("base");
            let head = dir.join("head");
            if !base.is_dir() || !head.is_dir() {
                continue;
            }
            let meta = load_meta(&dir);
            let title = meta.title.unwrap_or_else(|| derive_title(&name));
            let description = meta
                .description
                .unwrap_or_else(|| format!("Sample PR from {name}."));
            out.push(Sample { id: name, title, description, base, head });
        }
        out.sort_by(|a, b| a.id.cmp(&b.id));
        tracing::info!(count = out.len(), "loaded demo samples");
        Self { samples: out }
    }

    /// Look up a sample by id. Returns `None` when no match.
    pub fn get(&self, id: &str) -> Option<&Sample> {
        self.samples.iter().find(|s| s.id == id)
    }

    /// Public view list for `GET /samples`.
    pub fn view(&self) -> Vec<SampleView> {
        self.samples.iter().map(SampleView::from).collect()
    }
}

fn load_meta(dir: &Path) -> SampleMeta {
    let path = dir.join("meta.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return SampleMeta::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// `pr-0001-add-retry` → `Add retry`. Lowercase hyphen-separated
/// slug with a leading `pr-NNNN-` prefix.
fn derive_title(slug: &str) -> String {
    let rest = slug.strip_prefix("pr-").unwrap_or(slug);
    // Skip the numeric chunk after `pr-`.
    let after_num = rest.split_once('-').map(|x| x.1).unwrap_or(rest);
    let words: Vec<String> = after_num.split('-').map(str::to_string).collect();
    let mut out = words.join(" ");
    if let Some(first) = out.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    if out.is_empty() {
        slug.to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn make_sample(root: &Path, name: &str, with_meta: Option<&str>) {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("base")).unwrap();
        fs::create_dir_all(dir.join("head")).unwrap();
        if let Some(json) = with_meta {
            fs::write(dir.join("meta.json"), json).unwrap();
        }
    }

    #[test]
    fn missing_root_yields_empty_set() {
        let s = Samples::load(Path::new("/definitely-not-a-real-path-12345"));
        assert!(s.samples.is_empty());
    }

    #[test]
    fn loads_multiple_samples_sorted_by_id() {
        let d = tmp();
        make_sample(d.path(), "pr-0002-state-widen", None);
        make_sample(d.path(), "pr-0001-add-retry", None);
        let s = Samples::load(d.path());
        assert_eq!(s.samples.len(), 2);
        assert_eq!(s.samples[0].id, "pr-0001-add-retry");
        assert_eq!(s.samples[1].id, "pr-0002-state-widen");
    }

    #[test]
    fn meta_json_overrides_derived_title_and_description() {
        let d = tmp();
        make_sample(
            d.path(),
            "pr-0001-add-retry",
            Some(r#"{"title":"Retry logic","description":"adds a retry with backoff"}"#),
        );
        let s = Samples::load(d.path());
        assert_eq!(s.samples[0].title, "Retry logic");
        assert_eq!(s.samples[0].description, "adds a retry with backoff");
    }

    #[test]
    fn derived_title_from_slug() {
        let d = tmp();
        make_sample(d.path(), "pr-0003-api-widen", None);
        let s = Samples::load(d.path());
        assert_eq!(s.samples[0].title, "Api widen");
    }

    #[test]
    fn skips_directory_without_base_and_head() {
        let d = tmp();
        fs::create_dir_all(d.path().join("pr-0001-broken")).unwrap();
        let s = Samples::load(d.path());
        assert!(s.samples.is_empty());
    }

    #[test]
    fn skips_entries_not_prefixed_with_pr_dash() {
        let d = tmp();
        make_sample(d.path(), "not-a-sample", None);
        make_sample(d.path(), "pr-0001-ok", None);
        let s = Samples::load(d.path());
        assert_eq!(s.samples.len(), 1);
        assert_eq!(s.samples[0].id, "pr-0001-ok");
    }

    #[test]
    fn get_returns_none_for_unknown_id() {
        let d = tmp();
        make_sample(d.path(), "pr-0001-ok", None);
        let s = Samples::load(d.path());
        assert!(s.get("pr-9999-missing").is_none());
        assert!(s.get("pr-0001-ok").is_some());
    }

    #[test]
    fn view_does_not_leak_paths() {
        let d = tmp();
        make_sample(d.path(), "pr-0001-ok", None);
        let s = Samples::load(d.path());
        let json = serde_json::to_string(&s.view()).unwrap();
        assert!(!json.contains(d.path().to_string_lossy().as_ref()));
    }

    #[test]
    fn malformed_meta_json_falls_back_to_derived() {
        let d = tmp();
        make_sample(
            d.path(),
            "pr-0001-add-retry",
            Some("{ not json at all"),
        );
        let s = Samples::load(d.path());
        assert_eq!(s.samples[0].title, "Add retry");
    }
}
