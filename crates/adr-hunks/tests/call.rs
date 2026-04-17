use std::path::PathBuf;

use adr_hunks::{extract_all, extract_call_hunk};
use adr_parse::Ingest;

fn fixture(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join(rel)
}

fn ingest_pair(slug: &str) -> (adr_core::Graph, adr_core::Graph) {
    let base = Ingest::new("base")
        .ingest_dir(&fixture(&format!("fixtures/{slug}/base")))
        .expect("base");
    let head = Ingest::new("head")
        .ingest_dir(&fixture(&format!("fixtures/{slug}/head")))
        .expect("head");
    (base, head)
}

#[test]
fn call_hunk_pr0001() {
    let (base, head) = ingest_pair("pr-0001-add-retry");
    let hunk = extract_call_hunk(&base, &head).expect("call hunk");
    insta::assert_json_snapshot!(hunk, {
        ".id" => "[id]",
        ".provenance.hash" => "[hash]"
    });
}

#[test]
fn call_hunk_none_when_identical() {
    let g = Ingest::new("g")
        .ingest_dir(&fixture("fixtures/pr-0001-add-retry/base"))
        .expect("ingest");
    let g2 = Ingest::new("g")
        .ingest_dir(&fixture("fixtures/pr-0001-add-retry/base"))
        .expect("ingest");
    assert!(extract_call_hunk(&g, &g2).is_none());
}

#[test]
fn all_hunks_pr0002_state() {
    let (base, head) = ingest_pair("pr-0002-state-widen");
    let hunks = extract_all(&base, &head);
    insta::assert_json_snapshot!(hunks, {
        "[].id" => "[id]",
        "[].provenance.hash" => "[hash]"
    });
}

#[test]
fn all_hunks_pr0003_api() {
    let (base, head) = ingest_pair("pr-0003-api-widen");
    let hunks = extract_all(&base, &head);
    insta::assert_json_snapshot!(hunks, {
        "[].id" => "[id]",
        "[].provenance.hash" => "[hash]"
    });
}
