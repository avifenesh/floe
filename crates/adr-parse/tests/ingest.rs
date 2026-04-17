use std::path::PathBuf;

use adr_parse::Ingest;

fn fixture(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join(rel)
}

#[test]
fn ingest_pr0001_base() {
    let graph = Ingest::new("test")
        .ingest_dir(&fixture("fixtures/pr-0001-add-retry/base"))
        .expect("ingest");
    insta::assert_json_snapshot!(graph, {
        ".nodes[].provenance.hash" => "[hash]",
        ".edges[].provenance.hash" => "[hash]"
    });
}

#[test]
fn ingest_pr0001_head() {
    let graph = Ingest::new("test")
        .ingest_dir(&fixture("fixtures/pr-0001-add-retry/head"))
        .expect("ingest");
    insta::assert_json_snapshot!(graph, {
        ".nodes[].provenance.hash" => "[hash]",
        ".edges[].provenance.hash" => "[hash]"
    });
}
