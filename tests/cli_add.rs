mod common;
use common::{git_commit_all, splice_cmd, template_repo, tree_snapshot};
use splice::lock::Lock;
use std::path::Path;
use tempfile::TempDir;

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

/// A repo created by `splice new` from fixture v1 (base only unless `extra` adds pieces).
fn new_repo(extra: &[&str]) -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path())])
        .args(extra)
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    common::git_commit_all_init(repo.path());
    common::git_commit_all(repo.path(), "new");
    (tpl, repo)
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn lock_bytes(repo: &TempDir) -> Vec<u8> {
    std::fs::read(repo.path().join(".splice/lock.toml")).unwrap()
}

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn add_second_app_updates_pieces_dependent_files() {
    let (_tpl, repo) = new_repo(&[]);
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--name", "admin", "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(v["command"], "add");
    assert_eq!(v["ok"], true);
    assert!(repo.path().join("apps/admin/package.json").exists());
    assert!(repo.path().join("only-with-app.txt").exists());
    assert_eq!(read(&repo, "apps.txt"), "base\napp\n");
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
    // seeds are created but never locked
    assert!(repo.path().join("apps/admin/src/main.ts").exists());
    assert!(!lock.files.contains_key("apps/admin/src/main.ts"));
    let changes = v["changes"].as_array().unwrap();
    let act = |p: &str| {
        changes
            .iter()
            .find(|c| c["path"] == p)
            .map(|c| c["action"].as_str().unwrap().to_string())
    };
    assert_eq!(act("apps.txt").as_deref(), Some("update"));
    assert_eq!(act("only-with-app.txt").as_deref(), Some("create"));
    assert_eq!(act("apps/admin/package.json").as_deref(), Some("create"));
    // the root package.json lists the pieces, so it changes too (shared, merged)
    assert_eq!(act("package.json").as_deref(), Some("update"));
    assert!(read(&repo, "package.json").contains("\"app\""));
    // unchanged files are not reported
    assert_eq!(act("config/settings.toml"), None);
}

#[test]
fn add_second_instance_of_multiple_piece_and_dependent_piece() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--name", "admin", "--yes"])
        .assert()
        .success();
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "lib", "--yes"])
        .assert()
        .success();
    assert_eq!(read(&repo, "apps.txt"), "base\napp\nlib\n");
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.pieces.len(), 4);
}

fn assert_usage_untouched(repo: &TempDir, args: &[&str]) {
    let before_tree = tree_snapshot(repo.path());
    let before_lock = lock_bytes(repo);
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(args)
        .args(["--yes", "--json"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(out)["ok"], false);
    assert_eq!(tree_snapshot(repo.path()), before_tree);
    assert_eq!(lock_bytes(repo), before_lock);
}

#[test]
fn duplicate_name_and_single_piece_twice_are_usage() {
    let (_tpl, repo) = new_repo(&[]);
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--name", "web", "--yes"])
        .assert()
        .success();
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "app", "--name", "web"]);
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "lib", "--yes"])
        .assert()
        .success();
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "lib"]);
}

#[test]
fn other_usage_errors_leave_everything_untouched() {
    let (_tpl, repo) = new_repo(&[]);
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "nope"]);
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "base"]);
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "lib"]); // requires app
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "app", "--name", "../x"]);
    assert_usage_untouched(
        &repo,
        &["add", "--allow-dirty", "app", "--name", "Bad Name"],
    ); // pattern
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "app", "--data", "bogus=1"]);
    assert_usage_untouched(
        &repo,
        &["add", "--allow-dirty", "app", "--data", "project=other"],
    ); // template var
}

#[test]
fn non_multiple_piece_cannot_be_named() {
    let (_tpl, repo) = new_repo(&["--piece", "app"]);
    assert_usage_untouched(&repo, &["add", "--allow-dirty", "lib", "--name", "x"]);
}

#[test]
fn edited_owned_file_refuses_the_add() {
    // spec 8.3 step 6 via 8.2: owned drift writes nothing and exits 1
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("apps.txt"), "mine\n").unwrap();
    let before_tree = tree_snapshot(repo.path());
    let before_lock = lock_bytes(&repo);
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(v["ok"], false);
    assert_eq!(v["changes"].as_array().unwrap().len(), 0, "{v}");
    let msgs = v["messages"].to_string();
    assert!(
        msgs.contains("apps.txt") && msgs.contains("nothing was written"),
        "{v}"
    );
    assert_eq!(tree_snapshot(repo.path()), before_tree);
    assert_eq!(lock_bytes(&repo), before_lock);
}

#[test]
fn force_replaces_edited_file() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("apps.txt"), "mine\n").unwrap();
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--force", "--yes"])
        .assert()
        .success();
    assert_eq!(read(&repo, "apps.txt"), "base\napp\n");
}

#[test]
fn edited_seed_is_never_touched() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    std::fs::write(repo.path().join("README.md"), "mine\n").unwrap();
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--name", "admin", "--yes"])
        .assert()
        .success();
    assert_eq!(read(&repo, "README.md"), "mine\n");
}

#[test]
fn add_without_lock_is_usage() {
    let d = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .current_dir(d.path())
        .args(["add", "app", "--yes"])
        .assert()
        .code(2)
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&out).contains("lock.toml"));
    assert!(!d.path().join(".splice").exists());
}

#[test]
fn dash_c_selects_the_repo() {
    let (_tpl, repo) = new_repo(&[]);
    splice_cmd()
        .args(["-C", s(repo.path()), "add", "app", "--yes"])
        .assert()
        .success();
    assert!(repo.path().join("apps/web/package.json").exists());
}

#[cfg(unix)]
#[test]
fn symlink_in_the_way_is_template_error_and_nothing_is_written() {
    let (_tpl, repo) = new_repo(&[]);
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), repo.path().join("apps")).unwrap();
    let before_lock = lock_bytes(&repo);
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--json"])
        .assert()
        .code(3);
    assert_eq!(lock_bytes(&repo), before_lock);
    assert!(!repo.path().join("only-with-app.txt").exists());
    assert_eq!(read(&repo, "apps.txt"), "base\n");
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

#[test]
fn unknown_lock_keys_surface_as_messages() {
    let (_tpl, repo) = new_repo(&[]);
    let p = repo.path().join(".splice/lock.toml");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, format!("future_key = 1\n{text}")).unwrap();
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert!(
        v["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str().unwrap().contains("future_key")),
        "{v}"
    );
}

#[test]
fn post_runs_for_the_added_piece_only_when_not_disabled() {
    // base declares a post command for new/update only, so `add` runs none
    let (_tpl, repo) = new_repo(&[]);
    std::fs::remove_file(repo.path().join(".post-ran")).ok();
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--no-post"])
        .assert()
        .success();
    assert!(!repo.path().join(".post-ran").exists());
}

/// v1 with a pieces-dependent shared file (`notes.txt`) and an `add` post on the app piece,
/// served from branch `main`.
fn custom_repo() -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let notes = tpl.path().join("pieces/base/files/notes.txt.jinja");
    let mut t = std::fs::read_to_string(&notes).unwrap();
    t.push_str("{% if \"app\" in pieces %}app note\n{% endif %}");
    std::fs::write(&notes, t).unwrap();
    let pt = tpl.path().join("pieces/app/piece.toml");
    let mut p = std::fs::read_to_string(&pt).unwrap();
    p.push_str("\n[[post]]\nrun = \"echo x> .post-add\"\nwhen = [\"add\"]\n");
    std::fs::write(&pt, p).unwrap();
    common::git_commit_all(tpl.path(), "custom");
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path()), "--ref", "main"])
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    common::git_commit_all_init(repo.path());
    common::git_commit_all(repo.path(), "new");
    (tpl, repo)
}

fn change_action(v: &serde_json::Value, path: &str) -> Option<String> {
    v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == path)
        .map(|c| c["action"].as_str().unwrap().to_string())
}

#[test]
fn force_never_overwrites_edited_shared_file() {
    let (_tpl, repo) = custom_repo();
    std::fs::write(repo.path().join("notes.txt"), "mine\n").unwrap();
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--force", "--yes", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    // the edit is never replaced: it comes back inside diff3 markers
    let notes = read(&repo, "notes.txt");
    assert!(notes.contains("<<<<<<< ours\nmine\n"), "{notes}");
    assert!(notes.contains(">>>>>>> template"), "{notes}");
    assert_eq!(
        change_action(&json(out), "notes.txt").as_deref(),
        Some("conflict")
    );
}

#[test]
fn deleted_shared_is_skip_deleted_owned_refuses_unless_forced() {
    let (_tpl, repo) = custom_repo();
    std::fs::remove_file(repo.path().join("notes.txt")).unwrap();
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        change_action(&json(out), "notes.txt").as_deref(),
        Some("skip")
    );
    assert!(!repo.path().join("notes.txt").exists());

    let (_tpl2, repo2) = custom_repo();
    std::fs::remove_file(repo2.path().join("apps.txt")).unwrap();
    let before = lock_bytes(&repo2);
    splice_cmd()
        .current_dir(repo2.path())
        .args(["add", "--allow-dirty", "app", "--yes"])
        .assert()
        .code(1);
    assert!(!repo2.path().join("apps.txt").exists());
    assert_eq!(lock_bytes(&repo2), before);
    splice_cmd()
        .current_dir(repo2.path())
        .args(["add", "--allow-dirty", "app", "--force", "--yes"])
        .assert()
        .success();
    assert_eq!(read(&repo2, "apps.txt"), "base\napp\n");
}

#[test]
fn post_is_skipped_on_conflict_and_runs_otherwise() {
    let (_tpl, repo) = custom_repo();
    std::fs::write(repo.path().join("notes.txt"), "mine\n").unwrap();
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    assert!(!repo.path().join(".post-add").exists());
    let msgs = json(out)["messages"].to_string();
    assert!(
        msgs.contains("post commands skipped") && msgs.contains("echo x> .post-add"),
        "{msgs}"
    );
    let (_tpl2, repo2) = custom_repo();
    splice_cmd()
        .current_dir(repo2.path())
        .args(["add", "--allow-dirty", "app", "--yes"])
        .assert()
        .success();
    assert!(repo2.path().join(".post-add").exists());
}

#[test]
fn edited_base_only_file_is_kept_and_left_out_of_the_lock() {
    let tpl = template_repo(&["v1"]);
    let f = tpl.path().join("pieces/base/files/gone.txt.jinja");
    std::fs::write(&f, "{% if \"app\" in pieces %}{% else %}hello\n{% endif %}").unwrap();
    common::git_commit_all(tpl.path(), "gone");
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path()), "--ref", "main"])
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    assert!(
        Lock::read(repo.path())
            .unwrap()
            .unwrap()
            .files
            .contains_key("gone.txt")
    );
    std::fs::write(repo.path().join("gone.txt"), "mine\n").unwrap();
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(change_action(&v, "gone.txt").as_deref(), Some("skip"));
    assert!(
        v["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m.as_str().unwrap().contains("gone.txt")
                && m.as_str().unwrap().contains("no longer managed")),
        "{v}"
    );
    assert_eq!(read(&repo, "gone.txt"), "mine\n");
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert!(!lock.files.contains_key("gone.txt"));
}

#[test]
fn conflicted_shared_file_lock_records_theirs() {
    let (_tpl, repo) = custom_repo();
    std::fs::write(repo.path().join("notes.txt"), "mine\n").unwrap();
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "--allow-dirty", "app", "--yes"])
        .assert()
        .code(1);
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    let theirs = splice::fsio::sha256_hex(
        b"Notes for demo\nline two\nline three\nline four\nline five\napp note\n",
    );
    let disk = splice::fsio::sha256_hex(
        std::fs::read(repo.path().join("notes.txt"))
            .unwrap()
            .as_slice(),
    );
    assert_eq!(lock.files["notes.txt"].sha256, theirs);
    assert_ne!(lock.files["notes.txt"].sha256, disk);
}

#[test]
fn add_merges_edited_shared_file() {
    let (_tpl, repo) = new_repo(&[]);
    let p = repo.path().join("package.json");
    let t = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        t.replace(
            "\"private\": true,",
            "\"private\": true,\n  \"license\": \"MIT\",",
        ),
    )
    .unwrap();
    git_commit_all(repo.path(), "edit");
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--name", "admin", "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert_eq!(change_action(&v, "package.json").as_deref(), Some("merge"));
    let pkg = read(&repo, "package.json");
    assert!(pkg.contains("\"license\": \"MIT\""), "{pkg}");
    assert!(pkg.contains("\"app\","), "{pkg}");
}

#[test]
fn dirty_tree_is_refused_unless_allow_dirty() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("x"), "x").unwrap();
    let before = tree_snapshot(repo.path());
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--yes"])
        .assert()
        .code(2);
    assert_eq!(tree_snapshot(repo.path()), before);
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--yes", "--allow-dirty"])
        .assert()
        .success();
    assert!(repo.path().join("apps/web/package.json").exists());
}

#[test]
fn no_work_tree_is_refused_unless_allow_dirty() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--yes"])
        .assert()
        .code(2);
    assert!(!repo.path().join("apps/web").exists());
    splice_cmd()
        .current_dir(repo.path())
        .args(["add", "app", "--yes", "--allow-dirty"])
        .assert()
        .success();
}
