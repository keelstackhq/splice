//! `splice add` end to end against the fixture template.

mod common;

use common::{splice_cmd, template_repo, tree_snapshot};
use splice::lock::Lock;
use tempfile::TempDir;

/// A repo made by `splice new` from the v1 fixture; the template dir is returned to keep it alive.
fn new_repo(extra: &[&str]) -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--data", "project=demo", "--yes", "--json"])
        .args(extra)
        .assert()
        .success();
    (tpl, repo)
}

fn add(repo: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd()
        .current_dir(repo.path())
        .arg("add")
        .args(args)
        .args(["--yes", "--json"])
        .assert()
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn lock_bytes(repo: &TempDir) -> Vec<u8> {
    std::fs::read(repo.path().join(".splice/lock.toml")).unwrap()
}

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).expect("stdout is exactly one JSON object")
}

#[test]
fn add_second_app_updates_pieces_dependent_files() {
    let (_tpl, repo) = new_repo(&[]); // base only
    add(&repo, &["app", "--name", "admin"]).success();
    assert!(repo.path().join("apps/admin/package.json").exists());
    assert!(repo.path().join("only-with-app.txt").exists()); // `"app" in pieces` now true
    assert_eq!(read(&repo, "apps.txt"), "pieces:\n- base\n- app\n\n");
    assert!(read(&repo, "apps/admin/package.json").contains("@acme/admin"));
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(
        lock.pieces
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["base", "admin"]
    );
    assert!(lock.files.contains_key("apps/admin/package.json"));
    assert!(lock.files.contains_key("only-with-app.txt"));
    // Seeds are created but never listed.
    assert!(repo.path().join("apps/admin/src/main.ts").exists());
    assert!(!lock.files.contains_key("apps/admin/src/main.ts"));
}

#[test]
fn add_reports_created_files_and_classes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    let out = add(&repo, &["app", "--name", "admin"])
        .success()
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert_eq!(r["ok"], true);
    assert_eq!(r["command"], "add");
    let changes = r["changes"].as_array().unwrap();
    let find = |p: &str| changes.iter().find(|c| c["path"] == p).unwrap().clone();
    assert_eq!(find("apps/admin/package.json")["action"], "create");
    assert_eq!(find("apps/admin/src/main.ts")["class"], "seed");
}

#[test]
fn duplicate_name_and_single_piece_twice_are_usage() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    for (first, second) in [
        (None, vec!["app", "--name", "web"]),
        (Some(vec!["lib"]), vec!["lib"]),
    ] {
        if let Some(first) = first {
            add(&repo, &first).success();
        }
        let tree = tree_snapshot(repo.path());
        let lock = lock_bytes(&repo);
        let out = add(&repo, &second).code(2).get_output().stdout.clone();
        assert_eq!(json(out)["ok"], false);
        assert_eq!(tree_snapshot(repo.path()), tree);
        assert_eq!(lock_bytes(&repo), lock);
    }
}

#[test]
fn usage_errors_change_nothing() {
    let (_tpl, repo) = new_repo(&[]); // base only: `lib` requires `app`
    let tree = tree_snapshot(repo.path());
    let lock = lock_bytes(&repo);
    for args in [
        vec!["lib"],                    // requires app
        vec!["nope"],                   // unknown piece
        vec!["lib", "--name", "x"],     // not multiple
        vec!["base"],                   // already present, not multiple
        vec!["app", "--data", "zzz=1"], // unknown var
    ] {
        add(&repo, &args).code(2);
        assert_eq!(tree_snapshot(repo.path()), tree, "{args:?}");
        assert_eq!(lock_bytes(&repo), lock, "{args:?}");
    }
}

#[test]
fn edited_file_reported_not_overwritten() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("apps.txt"), "mine\n").unwrap();
    let out = add(&repo, &["app", "--name", "admin"])
        .code(1)
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert_eq!(r["ok"], false);
    assert_eq!(read(&repo, "apps.txt"), "mine\n");
    let c = r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "apps.txt")
        .unwrap()
        .clone();
    assert_eq!(c["action"], "conflict");
    assert_eq!(c["class"], "owned");
    assert!(
        r["messages"][0]
            .as_str()
            .unwrap()
            .contains("edited locally")
    );
    // The rest was written, with the lock.
    assert!(repo.path().join("apps/admin/package.json").exists());
    assert_eq!(Lock::read(repo.path()).unwrap().unwrap().pieces.len(), 2);
}

#[test]
fn existing_path_with_other_content_is_a_conflict() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::create_dir_all(repo.path().join("apps/admin")).unwrap();
    std::fs::write(repo.path().join("apps/admin/package.json"), "mine\n").unwrap();
    let out = add(&repo, &["app", "--name", "admin"])
        .code(1)
        .get_output()
        .stdout
        .clone();
    let r = json(out);
    assert_eq!(read(&repo, "apps/admin/package.json"), "mine\n");
    let c = r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "apps/admin/package.json")
        .unwrap()
        .clone();
    assert_eq!(c["action"], "conflict");
}

#[test]
fn force_overwrites_conflicts() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("apps.txt"), "mine\n").unwrap();
    add(&repo, &["app", "--name", "admin", "--force"]).success();
    assert_eq!(read(&repo, "apps.txt"), "pieces:\n- base\n- app\n\n");
}

#[test]
fn existing_seed_is_kept() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::create_dir_all(repo.path().join("apps/admin/src")).unwrap();
    std::fs::write(repo.path().join("apps/admin/src/main.ts"), "mine\n").unwrap();
    let out = add(&repo, &["app", "--name", "admin"])
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(read(&repo, "apps/admin/src/main.ts"), "mine\n");
    let r = json(out);
    let c = r["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "apps/admin/src/main.ts")
        .unwrap()
        .clone();
    assert_eq!(c["action"], "skip");
}

#[test]
fn add_without_lock_is_usage() {
    let dir = tempfile::tempdir().unwrap();
    add(&dir, &["app"]).code(2);
}

#[test]
fn post_runs_for_add_unless_conflicts() {
    let tpl = template_repo(&["v1"]);
    let path = tpl.path().join("pieces/app/piece.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\n[[post]]\nrun = \"echo ran >> .add-post\"\nwhen = [\"add\"]\n");
    std::fs::write(&path, text).unwrap();
    common::amend_tip(tpl.path(), "v1.0.0");
    let fresh = || {
        let repo = tempfile::tempdir().unwrap();
        splice_cmd()
            .args([
                "new",
                tpl.path().to_str().unwrap(),
                repo.path().to_str().unwrap(),
            ])
            .args(["--data", "project=demo", "--yes", "--json"])
            .assert()
            .success();
        assert!(!repo.path().join(".add-post").exists()); // `when` has no `new`
        repo
    };
    // A conflict skips post.
    let repo = fresh();
    std::fs::write(repo.path().join("apps.txt"), "mine\n").unwrap();
    add(&repo, &["app", "--name", "a"]).code(1);
    assert!(!repo.path().join(".add-post").exists());
    // Without one, post runs.
    let repo = fresh();
    add(&repo, &["app", "--name", "a"]).success();
    assert_eq!(read(&repo, ".add-post"), "ran\n");
}
