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

fn new_app_from_relative_tpl(parent: &Path, tpl: &TempDir) -> std::path::PathBuf {
    copy_to(tpl, &parent.join("tpl"));
    run(
        parent,
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
    let app = parent.join("app");
    let lock = app.join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock).unwrap();
    let abs = lock_source(&lock);
    std::fs::write(&lock, text.replace(&abs, "../tpl")).unwrap();
    commit_all(&app);
    app
}

/// `update` resolves a relative lock source for use but writes back what was there.
#[test]
fn update_keeps_a_relative_lock_source() {
    let parent = tempfile::tempdir().unwrap();
    let app = new_app_from_relative_tpl(parent.path(), &template_repo(&["v1", "v2"]));
    run(&app, &["update", "--yes", "--json", "--to", "v2.0.0"]);
    let lock = app.join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock).unwrap();
    assert!(text.contains("version = \"2.0.0\""), "{text}");
    assert_eq!(lock_source(&lock), "../tpl");
}

/// A relative source in a channel lock resolves from the repo root for `check` and `channel
/// update`, and `update --all` keeps it relative.
#[test]
fn relative_channel_lock_source_checks_and_updates_and_stays_relative() {
    let parent = tempfile::tempdir().unwrap();
    let app = new_app_from_relative_tpl(parent.path(), &template_repo(&["v1"]));
    copy_to(&channel_repo(&["v1", "v2"]), &parent.path().join("chan"));
    let abs_chan = format!("{}//channel", parent.path().join("chan").display());
    run(
        &app,
        &[
            "channel",
            "add",
            &abs_chan,
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
    let lock = std::fs::read_dir(app.join(".splice/channels"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let text = std::fs::read_to_string(&lock).unwrap();
    let abs = lock_source(&lock);
    std::fs::write(&lock, text.replace(&abs, "../chan//channel")).unwrap();
    commit_all(&app);
    // The channel has a newer tag: `check` reaches it through the relative source (exit 1).
    for (cwd, args) in [
        (app.as_path(), vec!["check", "--json"]),
        (parent.path(), vec!["-C", "app", "check", "--json"]),
    ] {
        let out = run(cwd, &args).code(1).get_output().stdout.clone();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("update available 1.0.0"), "{text}");
    }
    run(&app, &["update", "--all", "--yes", "--json"]);
    let text = std::fs::read_to_string(&lock).unwrap();
    assert!(text.contains("version = \"2.0.0\""), "{text}");
    assert_eq!(lock_source(&lock), "../chan//channel");
}
