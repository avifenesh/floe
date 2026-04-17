//! Guard: the committed `schema.json` at the repo root must match the schema
//! derived from the live `Artifact` type. Run `cargo run -p adr-cli -- schema >
//! schema.json` to refresh after an intentional bump.

use std::path::PathBuf;

#[test]
fn schema_in_repo_matches_artifact_type() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let committed_path = manifest.join("../..").join("schema.json");
    let committed = std::fs::read_to_string(&committed_path)
        .unwrap_or_else(|_| panic!("read {}", committed_path.display()));
    let live = serde_json::to_string_pretty(&schemars::schema_for!(adr_core::Artifact))
        .expect("serialize schema");
    // Normalise trailing newline, Windows CRLF.
    let norm = |s: &str| s.replace("\r\n", "\n").trim_end().to_string();
    assert_eq!(
        norm(&live),
        norm(&committed),
        "schema.json is stale — run: cargo run -p adr-cli -- schema > schema.json"
    );
}
