//! Smoke test — start the real typescript-language-server, open a
//! two-file project with one cross-file call, assert the LSP call
//! hierarchy resolves across files (the exact capability that
//! tree-sitter alone can't deliver and the reason TS v2 exists).
//!
//! Skipped unless `typescript-language-server` is on PATH.
//!
//! Run with: `cargo test -p floe-lsp --test smoke -- --nocapture`

use std::fs;
use std::path::Path;

use floe_lsp::TsLspClient;

fn tsls_on_path() -> bool {
    let which = if cfg!(windows) { "where" } else { "which" };
    std::process::Command::new(which)
        .arg("typescript-language-server")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn cross_file_call_hierarchy() {
    if !tsls_on_path() {
        eprintln!(
            "skip: typescript-language-server not on PATH — \
             `npm i -g typescript-language-server typescript` to run this"
        );
        return;
    }

    let dir = tempfile::tempdir().expect("mktemp");
    let root = dir.path();
    fs::write(
        root.join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "es2022",
    "module": "nodenext",
    "moduleResolution": "nodenext",
    "strict": true,
    "skipLibCheck": true
  },
  "include": ["*.ts"]
}
"#,
    )
    .expect("write tsconfig");

    // callee.ts — a single exported function.
    let callee = root.join("callee.ts");
    fs::write(&callee, "export function callee(x: number): number {\n  return x + 1;\n}\n")
        .expect("write callee.ts");

    // caller.ts — imports and calls `callee`. The 3 `callee` tokens
    // are: import, call site, return-type passthrough.
    let caller = root.join("caller.ts");
    fs::write(
        &caller,
        "import { callee } from './callee';\nexport function caller(): number {\n  return callee(41);\n}\n",
    )
    .expect("write caller.ts");

    let mut client = TsLspClient::start(root).await.expect("start TsLspClient");
    let callee_src = fs::read_to_string(&callee).unwrap();
    client
        .open_file(&callee, &callee_src)
        .await
        .expect("open callee");
    let caller_src = fs::read_to_string(&caller).unwrap();
    client
        .open_file(&caller, &caller_src)
        .await
        .expect("open caller");

    // Position of `caller` in `caller.ts` — line 1 col 16 (after
    // "export function ").
    let items = client
        .prepare_call_hierarchy(&caller, 1, 16)
        .await
        .expect("prepare");
    assert!(
        !items.is_empty(),
        "prepare_call_hierarchy should return an item for `caller`; got empty"
    );

    let outgoing = client
        .outgoing_calls(&items[0])
        .await
        .expect("outgoing_calls");
    assert!(
        !outgoing.is_empty(),
        "caller should have ≥1 outgoing call (to `callee`); got empty"
    );

    // The outgoing target should point at the callee file.
    let target_uri = &outgoing[0].to.uri;
    assert!(
        target_uri.to_string().ends_with("callee.ts"),
        "expected outgoing target to be callee.ts, got {target_uri}"
    );

    client.shutdown().await.expect("shutdown");
}

#[allow(dead_code)]
fn _check_fixture_dir(_p: &Path) {}
