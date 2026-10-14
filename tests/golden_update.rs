mod common;
use common::{git, git_commit_all, splice_cmd, template_repo, tree_snapshot};
use splice::lock::Lock;
use std::path::Path;
use tempfile::TempDir;

const USER_README: &str = "# my own readme\n\nhand written\n";

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn new_at(tpl: &TempDir, at: &str) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args(["new", s(tpl.path()), s(repo.path()), "--ref", at])
        .args(["--piece", "app:web", "--data", "project=demo", "--yes"])
        .assert()
        .success();
    git_commit_all(repo.path(), "new");
    repo
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn write(repo: &TempDir, rel: &str, text: &str) {
    std::fs::write(repo.path().join(rel), text).unwrap();
}

fn replace_in(repo: &TempDir, rel: &str, from: &str, to: &str) {
    let t = read(repo, rel);
    assert!(t.contains(from), "{rel} lacks {from:?}:\n{t}");
    write(repo, rel, &t.replace(from, to));
}

/// The "User edit" column of the fixture table (all but the `apps/web/tsconfig.json` row, which
/// would refuse the whole update; see `golden_owned_edit_refused_then_forced`).
fn apply_user_edits(repo: &TempDir) {
    replace_in(
        repo,
        "package.json",
        "\"private\": true,",
        "\"private\": true,\n  \"dependencies\": {\n    \"left-pad\": \"^1.3.0\"\n  },",
    );
    replace_in(
        repo,
        "config/settings.toml",
        "[build]",
        "# keep this comment\n[build]",
    );
    let t = read(repo, "config/settings.toml");
    write(
        repo,
        "config/settings.toml",
        &format!("{t}\n[local]\nx = 1\n"),
    );
    replace_in(
        repo,
        ".github/workflows/ci.yml",
        "ubuntu-latest",
        "ubuntu-22.04",
    );
    replace_in(repo, "notes.txt", "line five", "line 5 from the user");
    write(repo, "README.md", USER_README);
    git_commit_all(repo.path(), "user edits");
}

fn lock_version(repo: &TempDir) -> String {
    Lock::read(repo.path())
        .unwrap()
        .unwrap()
        .template
        .version
        .to_string()
}

/// Replace values that differ between runs (shas, temp paths) with stable placeholders.
fn redact(text: &str, repo: &Path, tpl: &Path) -> String {
    let out = text.replace(s(repo), "<repo>").replace(s(tpl), "<tpl>");
    // 40-hex commit ids
    let mut res = String::new();
    let b: Vec<char> = out.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let run = b[i..]
            .iter()
            .take_while(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            .count();
        let prev_hex = i > 0 && b[i - 1].is_ascii_alphanumeric();
        if run == 40 && !prev_hex {
            res.push_str("<sha>");
            i += 40;
        } else {
            res.push(b[i]);
            i += 1;
        }
    }
    // the lock's own hash covers the temp path and commit id
    res.lines()
        .map(|l| {
            if l.starts_with("splice = \"") {
                "splice = \"<version>\""
            } else {
                l
            }
        })
        .map(|l| match l.strip_prefix(".splice/lock.toml ") {
            Some(_) => ".splice/lock.toml <hash>".to_string(),
            None => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[test]
fn golden_v1_to_v2_with_user_edits() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    apply_user_edits(&repo);
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let text = redact(&String::from_utf8(out).unwrap(), repo.path(), tpl.path());
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    insta::assert_json_snapshot!(value);
    insta::assert_snapshot!(redact(&tree_snapshot(repo.path()), repo.path(), tpl.path()));
    assert!(repo.path().join(".splice/migrated.txt").exists());
    assert!(
        !repo.path().join("tool.config.mts").exists()
            && repo.path().join("tool.config.ts").exists()
    );
    assert_eq!(read(&repo, "README.md"), USER_README);
    assert_eq!(lock_version(&repo), "2.0.0");
}

#[test]
fn golden_owned_edit_refused_then_forced() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    replace_in(&repo, "apps/web/tsconfig.json", "false", "\"edited\"");
    git_commit_all(repo.path(), "edit");
    let before = tree_snapshot(repo.path());
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["changes"], serde_json::json!([]));
    assert_eq!(tree_snapshot(repo.path()), before);
    assert_eq!(lock_version(&repo), "1.0.0");

    splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes", "--force"])
        .assert()
        .success();
    assert!(read(&repo, "apps/web/tsconfig.json").contains("\"strict\": true"));
    assert_eq!(lock_version(&repo), "2.0.0");
}

#[test]
fn rerun_after_restore_succeeds() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    apply_user_edits(&repo);
    let lock_before = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();

    let out = splice_cmd()
        .current_dir(repo.path())
        .env("SPLICE_FIXTURE_FAIL", "1")
        .args(["update", "--yes", "--json"])
        .assert()
        .code(3)
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    // the message names the directory and the restore command
    let restore = splice::post::restore_cmd(repo.path());
    assert!(
        stderr.contains(&restore) || stdout.contains(&restore.replace('\\', "\\\\")),
        "no restore command in:\n{stderr}\n{stdout}"
    );
    // files were written, the lock still describes v1
    assert!(repo.path().join("tool.config.ts").exists());
    assert_eq!(
        std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
        lock_before
    );
    assert_eq!(lock_version(&repo), "1.0.0");

    git(repo.path(), &["checkout", "--", "."]);
    git(repo.path(), &["clean", "-fd"]);
    assert!(!repo.path().join("tool.config.ts").exists());

    splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes"])
        .assert()
        .code(1);
    assert_eq!(lock_version(&repo), "2.0.0");
    assert!(repo.path().join(".splice/migrated.txt").exists());
}

#[test]
fn restore_from_a_monorepo_subdir_stays_inside_it() {
    let tpl = template_repo(&["v1", "v2"]);
    let parent = tempfile::tempdir().unwrap();
    git(parent.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(parent.path().join("other.txt"), "o\n").unwrap();
    let sub = parent.path().join("sub");
    splice_cmd()
        .args(["new", s(tpl.path()), s(&sub), "--ref", "v1.0.0"])
        .args(["--data", "project=demo", "--yes"])
        .assert()
        .success();
    git_commit_all(parent.path(), "all");
    // an untracked file at the monorepo root that the restore must not touch
    std::fs::write(parent.path().join("keep.txt"), "keep\n").unwrap();

    // run from elsewhere with a relative-free -C so the message must carry the directory
    let out = splice_cmd()
        .args(["update", "-C", s(&sub), "--yes"])
        .env("SPLICE_FIXTURE_FAIL", "1")
        .assert()
        .code(3)
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let line = stderr
        .split('`')
        .find(|p| p.starts_with("cd ") && p.contains("git clean -fd"))
        .unwrap_or_else(|| panic!("no restore command in:\n{stderr}"))
        .to_string();
    assert!(sub.join("tool.config.ts").exists());

    #[cfg(unix)]
    let status = {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", &line]);
        c
    };
    #[cfg(windows)]
    let status = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", &line]);
        c
    };
    let mut status = status;
    // run from an unrelated cwd: only the `cd` in the message may place us in the project
    let elsewhere = tempfile::tempdir().unwrap();
    let o = status
        .current_dir(elsewhere.path())
        .env("GIT_CONFIG_GLOBAL", elsewhere.path().join("none"))
        .env("GIT_CONFIG_SYSTEM", elsewhere.path().join("none"))
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{line}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!sub.join("tool.config.ts").exists());
    assert!(sub.join("tool.config.mts").exists());
    assert_eq!(
        std::fs::read_to_string(parent.path().join("keep.txt")).unwrap(),
        "keep\n"
    );
    assert_eq!(lock_version_at(&sub), "1.0.0");

    // rerun without the failure env (no user edits here, so no conflict): ok, lock at 2.0.0
    splice_cmd()
        .args(["update", "-C", s(&sub), "--yes"])
        .assert()
        .success();
    assert_eq!(lock_version_at(&sub), "2.0.0");
}

fn lock_version_at(dir: &Path) -> String {
    Lock::read(dir)
        .unwrap()
        .unwrap()
        .template
        .version
        .to_string()
}

fn failed_step_stderr(repo: &TempDir, extra: &[&str]) -> String {
    let out = splice_cmd()
        .current_dir(repo.path())
        .env("SPLICE_FIXTURE_FAIL", "1")
        .args(["update", "--yes"])
        .args(extra)
        .assert()
        .code(3)
        .get_output()
        .clone();
    String::from_utf8_lossy(&out.stderr).to_string()
}

#[test]
fn failed_step_with_allow_dirty_does_not_advise_git_clean() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    write(&repo, "scratch.txt", "uncommitted user work\n");
    let stderr = failed_step_stderr(&repo, &["--allow-dirty"]);
    assert!(!stderr.contains("git clean"), "{stderr}");
    assert!(
        stderr.contains("uncommitted changes before the update"),
        "{stderr}"
    );
    assert!(stderr.contains("git status"), "{stderr}");
}

#[test]
fn failed_step_with_allow_dirty_on_a_clean_tree_keeps_the_restore_command() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    let stderr = failed_step_stderr(&repo, &["--allow-dirty"]);
    assert!(
        stderr.contains(&splice::post::restore_cmd(repo.path())),
        "{stderr}"
    );
}

#[test]
fn failed_step_outside_a_work_tree_says_there_is_no_git_undo() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();
    let stderr = failed_step_stderr(&repo, &["--allow-dirty"]);
    assert!(!stderr.contains("git clean"), "{stderr}");
    assert!(stderr.contains("no git undo"), "{stderr}");
}

#[test]
fn update_refuses_to_merge_over_unresolved_markers() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_at(&tpl, "v1.0.0");
    apply_user_edits(&repo);
    splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes"])
        .assert()
        .code(1);
    assert!(read(&repo, ".github/workflows/ci.yml").contains("<<<<<<<"));
    // a newer template release touching the same file
    let ci = tpl
        .path()
        .join("pieces/base/files/.github/workflows/ci.yml.jinja");
    let text = std::fs::read_to_string(&ci).unwrap();
    std::fs::write(&ci, format!("{text}# more\n")).unwrap();
    git_commit_all(tpl.path(), "v2.1");
    git(tpl.path(), &["tag", "v2.1.0"]);

    let before = tree_snapshot(repo.path());
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes", "--allow-dirty", "--json"])
        .assert()
        .code(1)
        .get_output()
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let msgs = v["messages"].to_string();
    assert!(
        msgs.contains("resolve conflict markers in .github/workflows/ci.yml first (splice check)"),
        "{msgs}"
    );
    assert_eq!(tree_snapshot(repo.path()), before);
    assert_eq!(lock_version(&repo), "2.0.0");
}
