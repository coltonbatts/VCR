use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn run_vcr(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vcr"))
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("vcr command should run")
}

#[test]
fn library_add_verify_list_ascii_asset() {
    let dir = tempdir().expect("tempdir should create");
    let source = dir.path().join("tiny_ascii.txt");
    fs::write(&source, "HELLO\nVCR\n").expect("source should write");

    let add = run_vcr(
        dir.path(),
        &[
            "library",
            "add",
            "tiny_ascii.txt",
            "--id",
            "tiny-ascii",
            "--type",
            "ascii",
        ],
    );
    assert!(add.status.success(), "library add should succeed");

    let verify = run_vcr(dir.path(), &["library", "verify"]);
    assert!(verify.status.success(), "library verify should succeed");

    let list = run_vcr(dir.path(), &["library", "list", "--type", "ascii"]);
    assert!(list.status.success(), "library list should succeed");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("tiny-ascii"));

    assert!(
        dir.path().join("library/library.json").exists(),
        "registry should exist"
    );
    assert!(
        dir.path()
            .join("library/items/tiny-ascii/source.txt")
            .exists(),
        "copied asset should exist"
    );
}

#[test]
fn add_command_suggests_id_and_registers_in_library() {
    let dir = tempdir().expect("tempdir should create");
    let source = dir.path().join("My Cool Title.txt");
    fs::write(&source, "HELLO\nVCR\n").expect("source should write");

    let add = run_vcr(dir.path(), &["add", "My Cool Title.txt"]);
    assert!(
        add.status.success(),
        "add should succeed. stdout={} stderr={}",
        String::from_utf8_lossy(&add.stdout),
        String::from_utf8_lossy(&add.stderr)
    );
    let add_stdout = String::from_utf8_lossy(&add.stdout);
    assert!(add_stdout.contains("Using suggested id 'my-cool-title'"));
    assert!(add_stdout.contains("Added library item 'my-cool-title'"));

    let assets = run_vcr(dir.path(), &["assets"]);
    assert!(assets.status.success(), "assets list should succeed");
    let assets_stdout = String::from_utf8_lossy(&assets.stdout);
    assert!(assets_stdout.contains("library:my-cool-title"));
}

#[test]
fn add_command_can_target_pack_and_assets_info_can_resolve_it() {
    let dir = tempdir().expect("tempdir should create");
    let source = dir.path().join("line.txt");
    fs::write(&source, "LOWER\nTHIRD\n").expect("source should write");

    let add = run_vcr(dir.path(), &["add", "line.txt", "--pack", "social-kit"]);
    assert!(
        add.status.success(),
        "pack add should succeed. stdout={} stderr={}",
        String::from_utf8_lossy(&add.stdout),
        String::from_utf8_lossy(&add.stderr)
    );
    let add_stdout = String::from_utf8_lossy(&add.stdout);
    assert!(add_stdout.contains("Using suggested id 'line'"));
    assert!(add_stdout.contains("Added asset 'pack:social-kit/line'"));

    assert!(
        dir.path().join("packs/social-kit/pack.json").exists(),
        "pack manifest should exist"
    );
    assert!(
        dir.path()
            .join("packs/social-kit/items/line/source.txt")
            .exists(),
        "pack asset should be copied"
    );

    let search = run_vcr(dir.path(), &["assets", "search", "social-kit"]);
    assert!(search.status.success(), "assets search should succeed");
    let search_stdout = String::from_utf8_lossy(&search.stdout);
    assert!(search_stdout.contains("pack:social-kit/line"));

    let info = run_vcr(dir.path(), &["assets", "info", "pack:social-kit/line"]);
    assert!(info.status.success(), "assets info should succeed");
    let info_stdout = String::from_utf8_lossy(&info.stdout);
    assert!(info_stdout.contains("reference: pack:social-kit/line"));
    assert!(info_stdout.contains("origin: pack:social-kit"));
}
