use std::path::PathBuf;

use adr_hunks::extract_call_hunk;
use adr_parse::Ingest;

fn fixture(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join(rel)
}

#[test]
fn call_hunk_pr0001() {
    let base = Ingest::new("base")
        .ingest_dir(&fixture("fixtures/pr-0001-add-retry/base"))
        .expect("base");
    let head = Ingest::new("head")
        .ingest_dir(&fixture("fixtures/pr-0001-add-retry/head"))
        .expect("head");
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
