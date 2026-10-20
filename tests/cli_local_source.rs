//! Relative local template sources: the lock stores an absolute path, so every command works
//! from inside the repo, from elsewhere and with `-C` (spec §4, §10 item 18).

mod common;

use common::{channel_repo, commit_all, new_repo_at, splice_cmd, template_repo};
use std::path::Path;
use tempfile::TempDir;

fn copy_to(src: &TempDir, dst: &Path) {
    let ok = std::process::Command::new("cp")
        .arg("-r")
        .arg(src.path())
        .arg(dst)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

fn run(cwd: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd().current_dir(cwd).args(args).assert()
}

/// `check` succeeds and really reached the template (a fetch failure is only a message).
fn check_ok(cwd: &Path, args: &[&str]) {
    let out = run(cwd, args).success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(!text.contains("could not look for updates"), "{text}");
}

fn lock_source(file: &Path) -> String {
    let text = std::fs::read_to_string(file).unwrap();
    let doc: toml::Table = text.parse().unwrap();
    doc["template"]["source"].as_str().unwrap().to_string()
}

#[test]
fn relative_source_from_new_works_from_inside_and_outside_the_repo() {
    let parent = tempfile::tempdir().unwrap();
    copy_to(&template_repo(&["v1"]), &parent.path().join("tpl"));
    run(
        parent.path(),
        &[
            "new",
            "./tpl",
            "app",
            "--ref",
            "v1.0.0",
            "--piece",
            "app:web",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ],
    )
    .success();
    let app = parent.path().join("app");
    commit_all(&app);
    let stored = lock_source(&app.join(".splice/lock.toml"));
    assert!(Path::new(&stored).is_absolute(), "{stored}");
    check_ok(&app, &["check", "--json"]);
    check_ok(parent.path(), &["-C", "app", "check", "--json"]);
    check_ok(&app.join("apps"), &["-C", "..", "check", "--json"]);
}

#[test]
fn relative_source_from_channel_add_works_from_inside_the_repo() {
    let tpl = template_repo(&["v1"]);
    let app = new_repo_at(&tpl, "v1.0.0");
    let parent = tempfile::tempdir().unwrap();
    copy_to(&channel_repo(&["v1"]), &parent.path().join("chan"));
    // A source relative to the repo's parent, added from there with -C.
    let repo_name = app.path().to_str().unwrap().to_string();
    run(
        parent.path(),
        &[
            "-C",
            &repo_name,
            "channel",
            "add",
            "./chan//channel",
            "--ref",
            "v1.0.0",
            "--piece",
            "claude",
            "--piece",
            "skills:claude",
            "--data",
            "dir=claude",
            "--yes",
            "--json",
        ],
    )
    .success();
    let lock = std::fs::read_dir(app.path().join(".splice/channels"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let stored = lock_source(&lock);
    assert!(Path::new(&stored).is_absolute(), "{stored}");
    commit_all(app.path());
    check_ok(app.path(), &["check", "--json"]);
}

/// A relative source written into a lock by hand (or by another splice) resolves against the
/// repo root, never the process cwd.
#[test]
fn relative_lock_source_resolves_from_the_repo_root() {
    let parent = tempfile::tempdir().unwrap();
    copy_to(&template_repo(&["v1"]), &parent.path().join("tpl"));
    run(
        parent.path(),
        &[
            "new",
            "./tpl",
            "app",
            "--ref",
            "v1.0.0",
            "--piece",
            "app:web",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ],
    )
    .success();
    let app = parent.path().join("app");
    let lock = app.join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock).unwrap();
    let abs = lock_source(&lock);
    std::fs::write(&lock, text.replace(&abs, "../tpl")).unwrap();
    commit_all(&app);
    check_ok(&app, &["check", "--json"]);
    check_ok(parent.path(), &["-C", "app", "check", "--json"]);
    check_ok(&app.join("apps"), &["-C", "..", "check", "--json"]);
}
