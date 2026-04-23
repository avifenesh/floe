use std::path::PathBuf;

use floe_parse::Ingest;

fn fixture(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("../..").join(rel)
}

#[test]
fn cfg_pr0001_head() {
    let root = fixture("fixtures/pr-0001-add-retry/head");
    let graph = Ingest::new("head").ingest_dir(&root).expect("ingest");
    let cfg = floe_cfg::build_for_graph(&graph, &root).expect("cfg");
    insta::assert_json_snapshot!(cfg);
}

#[test]
fn cfg_pr0004_head() {
    let root = fixture("fixtures/pr-0004-combined/head");
    let graph = Ingest::new("head").ingest_dir(&root).expect("ingest");
    let cfg = floe_cfg::build_for_graph(&graph, &root).expect("cfg");
    insta::assert_json_snapshot!(cfg);
}
