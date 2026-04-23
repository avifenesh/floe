//! Workspace topology scanner.
//!
//! Reads whatever monorepo manifest the repo uses (pnpm, npm/yarn
//! workspaces, or `tsconfig.json#references`) and turns it into a
//! [`PackageMap`] — a precompiled glob → package-name lookup. Call
//! [`PackageMap::resolve`] with a file path to get the owning
//! package name, or `None` if the file is outside every declared
//! workspace glob.
//!
//! The manifests we handle:
//!
//! - `pnpm-workspace.yaml` — `packages: [glob, ...]` at the root.
//! - `package.json#workspaces` — either a string array (`["packages/*"]`)
//!   or an object with a `packages` array (yarn v2 style).
//! - `tsconfig.json#references` — array of `{ path: "..." }`. The
//!   referenced directory's own `tsconfig` gets its declared name if
//!   its `package.json` has one.
//!
//! For each glob hit we recurse one level to find the nested
//! `package.json#name`. That's the package label the rest of the
//! product sees on `Node.package`.
//!
//! Single-repo projects (no manifest) get an empty `PackageMap`;
//! `resolve` returns `None` for every path and downstream callers
//! treat the repo as one unnamed package.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Deserialize;

/// Precompiled map from path → owning package name.
pub struct PackageMap {
    entries: Vec<PackageEntry>,
    /// Root the map was built against — `resolve` strips this prefix
    /// before matching so either absolute or relative paths work.
    root: PathBuf,
}

struct PackageEntry {
    /// Human-visible name (either `package.json#name` or the glob's
    /// directory stem when the package doesn't declare a name).
    name: String,
    /// Relative path to the package root directory, without a trailing slash.
    root_rel: String,
    /// Glob matcher that covers every file under `root_rel`.
    matcher: GlobSet,
}

impl PackageMap {
    /// Build a `PackageMap` by scanning workspace manifests under
    /// `root`. Never returns an error — absent / malformed manifests
    /// yield an empty map with a log warning. The scan is cheap
    /// (~ms) and deterministic.
    pub fn load(root: &Path) -> Self {
        match try_load(root) {
            Ok(m) => m,
            Err(e) => {
                tracing::debug!(error = %e, "no workspace topology detected — running single-package");
                PackageMap {
                    entries: Vec::new(),
                    root: root.to_path_buf(),
                }
            }
        }
    }

    /// Resolve a `Node.file` (stored as a forward-slash path relative
    /// to the workspace root) to its package name. Returns `None`
    /// when the path is outside every declared package glob.
    pub fn resolve(&self, rel_file: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        // Normalise to forward slashes for glob matching — source
        // paths on Windows may arrive with `\`.
        let norm = rel_file.replace('\\', "/");
        for e in &self.entries {
            if e.matcher.is_match(&norm) {
                return Some(e.name.clone());
            }
        }
        None
    }

    /// Expose the registered package list — used by tests and by the
    /// debug endpoint on `floe-server::router`.
    pub fn packages(&self) -> Vec<&str> {
        self.entries.iter().map(|e| e.name.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn try_load(root: &Path) -> Result<PackageMap> {
    let mut patterns: Vec<String> = Vec::new();
    // pnpm-workspace.yaml
    let pnpm = root.join("pnpm-workspace.yaml");
    if pnpm.is_file() {
        let text = std::fs::read_to_string(&pnpm).context("read pnpm-workspace.yaml")?;
        let parsed: PnpmManifest =
            serde_yaml::from_str(&text).context("parse pnpm-workspace.yaml")?;
        patterns.extend(parsed.packages.unwrap_or_default());
    }
    // package.json#workspaces
    let pkg = root.join("package.json");
    if pkg.is_file() {
        let text = std::fs::read_to_string(&pkg).context("read root package.json")?;
        let parsed: RootPackageJson =
            serde_json::from_str(&text).context("parse root package.json")?;
        match parsed.workspaces {
            Some(Workspaces::List(v)) => patterns.extend(v),
            Some(Workspaces::Object(o)) => patterns.extend(o.packages.unwrap_or_default()),
            None => {}
        }
    }
    // tsconfig.json#references — paths point at directories with their
    // own tsconfig. Treat each referenced path as a package root.
    let tsconfig = root.join("tsconfig.json");
    if tsconfig.is_file() {
        let text = std::fs::read_to_string(&tsconfig).unwrap_or_default();
        // tsconfigs are JSONC; strip // comments naïvely — good enough
        // for the shape we're interested in.
        let stripped = strip_jsonc_comments(&text);
        if let Ok(cfg) = serde_json::from_str::<TsConfigRoot>(&stripped) {
            for r in cfg.references.unwrap_or_default() {
                patterns.push(r.path);
            }
        }
    }

    if patterns.is_empty() {
        return Ok(PackageMap {
            entries: Vec::new(),
            root: root.to_path_buf(),
        });
    }

    let mut entries: Vec<PackageEntry> = Vec::new();
    for pat in patterns {
        let pat = pat.trim_end_matches('/').to_string();
        // Each pattern is a directory glob; expand it to concrete dirs
        // under root, then for each dir read its package.json#name.
        let globbed = expand_dir_glob(root, &pat);
        for dir in globbed {
            let rel_dir_os = dir.strip_prefix(root).unwrap_or(&dir).to_string_lossy();
            let rel_dir = rel_dir_os.replace('\\', "/");
            let pkg_json = dir.join("package.json");
            let name = if pkg_json.is_file() {
                std::fs::read_to_string(&pkg_json)
                    .ok()
                    .and_then(|s| serde_json::from_str::<PackageJsonName>(&s).ok())
                    .and_then(|p| p.name)
                    .unwrap_or_else(|| rel_dir.clone())
            } else {
                rel_dir.clone()
            };
            let glob_str = if rel_dir.is_empty() {
                "**/*".to_string()
            } else {
                format!("{rel_dir}/**/*")
            };
            let mut builder = GlobSetBuilder::new();
            builder.add(
                Glob::new(&glob_str).with_context(|| format!("bad glob {glob_str}"))?,
            );
            builder.add(Glob::new(&rel_dir).with_context(|| format!("bad dir glob {rel_dir}"))?);
            let matcher = builder.build().context("build globset")?;
            entries.push(PackageEntry {
                name,
                root_rel: rel_dir,
                matcher,
            });
        }
    }
    // Sort longest-prefix first so nested packages (e.g.
    // `packages/core/internal`) win over their parents
    // (`packages/core`) in `resolve`.
    entries.sort_by_key(|e| std::cmp::Reverse(e.root_rel.len()));

    Ok(PackageMap {
        entries,
        root: root.to_path_buf(),
    })
}

/// Expand a workspace glob like `packages/*` into a list of concrete
/// directories under `root` that exist on disk. Respects `.gitignore`
/// via `ignore::WalkBuilder`.
fn expand_dir_glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    // Two cases: patterns without wildcards (e.g. `apps/web`) → direct
    // path check; wildcard patterns → walk and match.
    let pat = pattern.trim_end_matches('/');
    if !pat.contains('*') && !pat.contains('?') {
        let candidate = root.join(pat);
        return if candidate.is_dir() {
            vec![candidate]
        } else {
            Vec::new()
        };
    }
    let glob = match Glob::new(pat).map(|g| g.compile_matcher()) {
        Ok(g) => g,
        Err(e) => {
            tracing::warn!(pattern = %pat, error = %e, "skipping unparseable workspace glob");
            return Vec::new();
        }
    };
    // Pattern depth gates the walk depth — `packages/*` is 2 segments
    // and must only match directories at segment depth 2, not their
    // children. `globset`'s `*` default doesn't cross `/`, but
    // globset-after-walker can still return parent directories if a
    // pattern has no wildcard in a given segment. Enforce an exact
    // segment-count match to keep matches at the intended level.
    let pat_depth = pat.split('/').filter(|s| !s.is_empty()).count();
    let mut out = Vec::new();
    for entry in ignore::WalkBuilder::new(root)
        .max_depth(Some(pat_depth))
        .standard_filters(true)
        .build()
        .flatten()
    {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let rel = match entry.path().strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.is_empty() {
            continue;
        }
        let depth = rel_str.split('/').filter(|s| !s.is_empty()).count();
        if depth != pat_depth {
            continue;
        }
        if glob.is_match(&rel_str) {
            out.push(entry.path().to_path_buf());
        }
    }
    out
}

/// Strip `// line comments` from a JSONC body so `serde_json` can
/// parse it. Doesn't touch string-interior content — checks `in_str`
/// state. Good enough for tsconfig.json (which doesn't use `/* */`
/// blocks in its `references` section).
fn strip_jsonc_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let mut in_str = false;
    let mut escape = false;
    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            out.push(c);
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            // skip to newline
            for n in chars.by_ref() {
                if n == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[derive(Debug, Deserialize)]
struct PnpmManifest {
    #[serde(default)]
    packages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct RootPackageJson {
    #[serde(default)]
    workspaces: Option<Workspaces>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Workspaces {
    List(Vec<String>),
    Object(WorkspacesObject),
}

#[derive(Debug, Deserialize)]
struct WorkspacesObject {
    #[serde(default)]
    packages: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct TsConfigRoot {
    #[serde(default)]
    references: Option<Vec<TsReference>>,
}

#[derive(Debug, Deserialize)]
struct TsReference {
    path: String,
}

#[derive(Debug, Deserialize)]
struct PackageJsonName {
    #[serde(default)]
    name: Option<String>,
}

/// Walk a graph and tag every node's `package` field using the map.
pub fn tag_graph(graph: &mut floe_core::Graph, map: &PackageMap) {
    if map.is_empty() {
        return;
    }
    for node in graph.nodes.iter_mut() {
        if node.package.is_none() {
            node.package = map.resolve(&node.file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tmpdir")
    }

    #[test]
    fn strips_jsonc_line_comments() {
        let src = r#"// comment
        { "a": 1, "b": "//not a comment" }"#;
        let out = strip_jsonc_comments(src);
        assert!(out.contains("\"a\": 1"));
        assert!(out.contains("\"//not a comment\""));
    }

    #[test]
    fn empty_map_when_no_manifest() {
        let dir = tmpdir();
        let map = PackageMap::load(dir.path());
        assert!(map.is_empty());
        assert_eq!(map.resolve("src/foo.ts"), None);
    }

    #[test]
    fn pnpm_workspace_with_two_packages() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("packages/core/src")).unwrap();
        fs::create_dir_all(dir.path().join("packages/api/src")).unwrap();
        fs::write(
            dir.path().join("packages/core/package.json"),
            r#"{ "name": "@adr/core" }"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("packages/api/package.json"),
            r#"{ "name": "@adr/api" }"#,
        )
        .unwrap();
        let map = PackageMap::load(dir.path());
        assert!(!map.is_empty());
        assert_eq!(
            map.resolve("packages/core/src/index.ts").as_deref(),
            Some("@adr/core")
        );
        assert_eq!(
            map.resolve("packages/api/src/index.ts").as_deref(),
            Some("@adr/api")
        );
        assert_eq!(map.resolve("scripts/release.ts"), None);
    }

    #[test]
    fn package_json_workspaces_array() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "root", "workspaces": ["apps/*"] }"#,
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("apps/web/src")).unwrap();
        fs::write(
            dir.path().join("apps/web/package.json"),
            r#"{ "name": "web" }"#,
        )
        .unwrap();
        let map = PackageMap::load(dir.path());
        assert_eq!(map.resolve("apps/web/src/main.ts").as_deref(), Some("web"));
    }

    #[test]
    fn falls_back_to_relpath_when_no_name_in_pkg() {
        let dir = tmpdir();
        fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - libs/*\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("libs/unnamed")).unwrap();
        fs::write(dir.path().join("libs/unnamed/package.json"), "{}").unwrap();
        let map = PackageMap::load(dir.path());
        assert_eq!(
            map.resolve("libs/unnamed/src/x.ts").as_deref(),
            Some("libs/unnamed")
        );
    }
}
