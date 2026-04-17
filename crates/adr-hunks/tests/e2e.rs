//! End-to-end: ingest base + head for every fixture, run every extractor, and
//! snapshot the full hunk list. These snapshots are the closest thing we have
//! to "does the whole pipeline still behave?" regression checks.

use std::path::PathBuf;

use adr_hunks::extract_all;
use adr_parse::Ingest;

fn fixture(slug: &str) -> (adr_core::Graph, adr_core::Graph) {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..").join("fixtures").join(slug);
    let base = Ingest::new("base")
        .ingest_dir(&root.join("base"))
        .expect("base");
    let head = Ingest::new("head")
        .ingest_dir(&root.join("head"))
        .expect("head");
    (base, head)
}

macro_rules! e2e {
    ($name:ident, $slug:literal) => {
        #[test]
        fn $name() {
            let (base, head) = fixture($slug);
            let hunks = extract_all(&base, &head);
            insta::assert_json_snapshot!(hunks, {
                "[].id" => "[id]",
                "[].provenance.hash" => "[hash]"
            });
        }
    };
}

e2e!(pr0001_add_retry, "pr-0001-add-retry");
e2e!(pr0004_combined, "pr-0004-combined");
e2e!(pr0005_noop, "pr-0005-noop");
e2e!(pr0006_cross_file, "pr-0006-cross-file");
