//! Coverage-delta pass — parse `lcov.info` (or vitest/jest
//! `coverage-summary.json`) on both sides after the test run, compute
//! per-file line-coverage delta (in permille), and attach `CoverageDrop`
//! claims to flows whose entities live in files that lost coverage.
//!
//! Path resolution: `FLOE_COVERAGE_FILE` → `coverage/lcov.info` →
//! `coverage/coverage-summary.json`. Missing files → skip silently.

use std::path::Path;

use floe_core::{Artifact, Claim, ClaimKind, CoverageDelta, CoverageFile, Strength};
use floe_core::provenance::Provenance;

pub async fn attach(artifact: &mut Artifact, base: &Path, head: &Path) {
    let Some(delta) = compute(base, head).await else {
        return;
    };
    for cf in &delta.files {
        let Some(drop) = cf.delta_permille else { continue };
        if drop >= 0 {
            continue;
        }
        let strength = if drop <= -100 {
            Strength::High
        } else if drop <= -20 {
            Strength::Medium
        } else {
            Strength::Low
        };
        let text = format!(
            "{} — coverage {}% → {}% (Δ {:+}%)",
            cf.file,
            cf.base_permille.unwrap_or(0) / 10,
            cf.head_permille.unwrap_or(0) / 10,
            drop / 10,
        );
        let file_touched = artifact.head.nodes.iter().any(|n| n.file == cf.file);
        if !file_touched {
            continue;
        }
        // Fan out the claim to every flow whose entities live in this file.
        for flow in artifact.flows.iter_mut() {
            let flow_touches = flow
                .entities
                .iter()
                .chain(flow.extra_entities.iter())
                .any(|e| {
                    artifact
                        .head
                        .nodes
                        .iter()
                        .any(|n| n.file == cf.file && node_name_matches(n, e))
                });
            if !flow_touches {
                continue;
            }
            let mut h = blake3::Hasher::new();
            h.update(b"coverage-drop|");
            h.update(flow.id.as_bytes());
            h.update(b"|");
            h.update(cf.file.as_bytes());
            let id = format!("claim-{}", h.finalize().to_hex());
            flow.evidence.push(Claim {
                id,
                kind: ClaimKind::CoverageDrop,
                text: text.clone(),
                strength,
                entities: Vec::new(),
                source_refs: Vec::new(),
                provenance: Provenance {
                    source: "floe-server::passes::coverage".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    pass_id: "coverage-delta".into(),
                    hash: String::new(),
                },
            });
        }
    }
    artifact.coverage_delta = Some(delta);
}

fn node_name_matches(n: &floe_core::Node, entity: &str) -> bool {
    match &n.kind {
        floe_core::NodeKind::Function { name, .. }
        | floe_core::NodeKind::Type { name }
        | floe_core::NodeKind::State { name, .. } => name == entity,
        _ => false,
    }
}

async fn compute(base: &Path, head: &Path) -> Option<CoverageDelta> {
    let (source, base_map) = read_side(base).await?;
    let (_, head_map) = read_side(head).await?;
    let mut files: std::collections::BTreeMap<String, CoverageFile> =
        std::collections::BTreeMap::new();
    for (p, v) in &base_map {
        files.entry(p.clone()).or_insert_with(|| cf(p)).base_permille = Some(*v);
    }
    for (p, v) in &head_map {
        files.entry(p.clone()).or_insert_with(|| cf(p)).head_permille = Some(*v);
    }
    for f in files.values_mut() {
        if let (Some(b), Some(h)) = (f.base_permille, f.head_permille) {
            f.delta_permille = Some(h - b);
        }
    }
    Some(CoverageDelta {
        source,
        files: files.into_values().collect(),
    })
}

fn cf(p: &str) -> CoverageFile {
    CoverageFile {
        file: p.to_string(),
        base_permille: None,
        head_permille: None,
        delta_permille: None,
    }
}

async fn read_side(root: &Path) -> Option<(String, std::collections::BTreeMap<String, i32>)> {
    if let Ok(custom) = std::env::var("FLOE_COVERAGE_FILE") {
        let p = root.join(&custom);
        if let Some(m) = load(&p).await {
            return Some((custom, m));
        }
    }
    for rel in ["coverage/lcov.info", "coverage/coverage-summary.json"] {
        let p = root.join(rel);
        if let Some(m) = load(&p).await {
            return Some((rel.to_string(), m));
        }
    }
    None
}

async fn load(path: &Path) -> Option<std::collections::BTreeMap<String, i32>> {
    let text = tokio::fs::read_to_string(path).await.ok()?;
    if path.extension().and_then(|s| s.to_str()) == Some("json") {
        parse_json_summary(&text)
    } else {
        Some(parse_lcov(&text))
    }
}

/// Bare-bones LCOV line-coverage parser. Output is permille (0–1000).
fn parse_lcov(text: &str) -> std::collections::BTreeMap<String, i32> {
    let mut out = std::collections::BTreeMap::new();
    let mut current: Option<String> = None;
    let mut lf: u32 = 0;
    let mut lh: u32 = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("SF:") {
            current = Some(rest.trim().replace('\\', "/"));
            lf = 0;
            lh = 0;
        } else if let Some(rest) = line.strip_prefix("LF:") {
            lf = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("LH:") {
            lh = rest.trim().parse().unwrap_or(0);
        } else if line.starts_with("end_of_record") {
            if let Some(f) = current.take() {
                if lf > 0 {
                    out.insert(f, (lh as i64 * 1000 / lf as i64) as i32);
                }
            }
        }
    }
    out
}

fn parse_json_summary(text: &str) -> Option<std::collections::BTreeMap<String, i32>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let obj = v.as_object()?;
    let mut out = std::collections::BTreeMap::new();
    for (key, val) in obj {
        if key == "total" {
            continue;
        }
        let pct = val
            .get("lines")
            .and_then(|l| l.get("pct"))
            .and_then(|p| p.as_f64())?;
        out.insert(key.replace('\\', "/"), (pct * 10.0).round() as i32);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lcov_records() {
        let text = "SF:src/a.ts\nLF:10\nLH:7\nend_of_record\nSF:src/b.ts\nLF:5\nLH:5\nend_of_record\n";
        let m = parse_lcov(text);
        assert_eq!(m["src/a.ts"], 700);
        assert_eq!(m["src/b.ts"], 1000);
    }

    #[test]
    fn parses_json_summary_file() {
        let text = r#"{"total":{"lines":{"pct":80}},"src/a.ts":{"lines":{"pct":50}}}"#;
        let m = parse_json_summary(text).unwrap();
        assert_eq!(m["src/a.ts"], 500);
        assert!(!m.contains_key("total"));
    }
}
