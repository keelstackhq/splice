//! `splice diff` end to end (spec §8.8): the update plan, printed, with nothing written.

mod common;

use common::{commit_all, splice_cmd, template_repo};
use tempfile::TempDir;

/// A repo at v1 of `tpl`, committed, with a shared edit that conflicts with v2 (`ci.yml`) and a
/// shared edit that merges cleanly (`notes.txt`), committed too.
fn edited_repo(tpl: &TempDir) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args([
            "--ref",
            "v1.0.0",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ])
        .assert()
        .success();
    let ci = repo.path().join(".github/workflows/ci.yml");
    let text = std::fs::read_to_string(&ci).unwrap();
    std::fs::write(&ci, text.replace("ubuntu-latest", "macos-latest")).unwrap();
    let notes = repo.path().join("notes.txt");
    let mut text = std::fs::read_to_string(&notes).unwrap();
    text.push_str("mine\n");
    std::fs::write(&notes, text).unwrap();
    commit_all(repo.path());
    repo
}

fn porcelain(repo: &TempDir) -> String {
    common::git_stdout(repo.path(), &["status", "--porcelain"])
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).expect("stdout is exactly one JSON object")
}

#[test]
fn diff_writes_nothing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = edited_repo(&tpl);
    // Owned drift too: `update` would refuse; `diff` still reports.
    std::fs::write(repo.path().join("tool.config.mts"), "mine\n").unwrap();
    commit_all(repo.path());
    let out = splice_cmd()
        .current_dir(repo.path())
        .arg("diff")
        .assert()
        .code(0)
        .get_output()
        .clone();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stdout.contains("--- a/.editorconfig"), "{stdout}");
    assert!(stdout.contains("+++ b/.editorconfig"), "{stdout}");
    assert!(stdout.contains("+indent_size = 4"), "{stdout}");
    // The conflict shows its markers; removals get one line each.
    assert!(stdout.contains("+<<<<<<< ours"), "{stdout}");
    assert!(stdout.contains("delete old.txt"), "{stdout}");
    assert!(
        stderr.contains("tool.config.mts") && stderr.contains("edited locally"),
        "{stderr}"
    );
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn diff_json_changes_match_update() {
    let tpl = template_repo(&["v1", "v2"]);
    let (a, b) = (edited_repo(&tpl), edited_repo(&tpl));
    let diff = splice_cmd()
        .current_dir(a.path())
        .args(["diff", "--json"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let diff = json(&diff);
    assert_eq!(diff["command"], "diff");
    assert_eq!(diff["ok"], true);
    assert_eq!(porcelain(&a), "");
    let update = splice_cmd()
        .current_dir(b.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .code(1) // the ci.yml conflict
        .get_output()
        .stdout
        .clone();
    let update = json(&update);
    assert!(!diff["changes"].as_array().unwrap().is_empty());
    assert_eq!(diff["changes"], update["changes"]);
}

#[test]
fn diff_up_to_date_and_downgrade() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = edited_repo(&tpl);
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["diff", "--to", "1.0.0", "--json"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(&out)["changes"], serde_json::json!([]));
    let tpl2 = template_repo(&["v1", "v2"]);
    let repo2 = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl2.path().to_str().unwrap(),
            repo2.path().to_str().unwrap(),
        ])
        .args(["--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    splice_cmd()
        .current_dir(repo2.path())
        .args(["diff", "--to", "1.0.0", "--json"])
        .assert()
        .code(2);
}

mod channels {
    use super::{json, porcelain};
    use crate::common::*;
    use tempfile::TempDir;

    fn src(ch: &TempDir) -> String {
        format!("{}//channel", ch.path().display())
    }

    /// Main repo plus channel rig at v1 (claude piece); the channel source also has v2.
    fn setup() -> (TempDir, TempDir, TempDir) {
        let tpl = template_repo(&["v1"]);
        let repo = new_repo_at(&tpl, "v1.0.0");
        let ch = channel_repo(&["v1", "v2"]);
        chan_commit(
            repo.path(),
            &[
                "channel",
                "add",
                &src(&ch),
                "--ref",
                "v1.0.0",
                "--piece",
                "claude",
            ],
        );
        (tpl, repo, ch)
    }

    #[test]
    fn diff_channel_writes_nothing() {
        let (_t, repo, _ch) = setup();
        let out = splice_cmd()
            .current_dir(repo.path())
            .args(["diff", "--channel", "rig"])
            .assert()
            .code(0)
            .get_output()
            .clone();
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(
            stdout.contains("--- a/.claude/agents/reviewer.md"),
            "{stdout}"
        );
        assert!(stdout.contains("+Review read-only, v2."), "{stdout}");
        assert!(stdout.contains("--- a/AGENTS.md"), "{stdout}");
        assert!(stdout.contains("+++ b/AGENTS.md"), "{stdout}");
        assert!(
            stdout.contains("+- Keep files small and focused."),
            "{stdout}"
        );
        assert!(
            stdout.contains("delete .claude/agents/explorer.md"),
            "{stdout}"
        );
        assert_eq!(porcelain(&repo), "");
    }

    #[test]
    fn diff_channel_json_matches_update() {
        let (_t, a, _c1) = setup();
        let (_t2, b, _c2) = setup();
        let diff = json(
            &splice_cmd()
                .current_dir(a.path())
                .args(["diff", "--channel", "rig", "--json"])
                .assert()
                .code(0)
                .get_output()
                .stdout,
        );
        assert_eq!(diff["command"], "diff");
        assert_eq!(diff["ok"], true);
        let update = json(
            &chan(b.path(), &["update", "--channel", "rig"])
                .success()
                .get_output()
                .stdout,
        );
        let changes = diff["changes"].as_array().unwrap();
        assert!(
            changes.iter().any(|c| c["path"] == "AGENTS.md#rig"),
            "{diff}"
        );
        assert_eq!(diff["changes"], update["changes"]);
    }

    #[test]
    fn diff_channel_reports_drift_and_unknown_channel() {
        let (_t, repo, _ch) = setup();
        std::fs::write(repo.path().join(".claude/agents/reviewer.md"), "mine\n").unwrap();
        let out = splice_cmd()
            .current_dir(repo.path())
            .args(["diff", "--channel", "rig"])
            .assert()
            .code(0)
            .get_output()
            .clone();
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(
            stderr.contains("reviewer.md") && stderr.contains("edited locally"),
            "{stderr}"
        );
        splice_cmd()
            .current_dir(repo.path())
            .args(["diff", "--channel", "nope"])
            .assert()
            .code(2);
    }
}

#[test]
fn diff_ref_matches_update_ref() {
    let tpl = template_repo(&["v1"]);
    let x = common::untagged_commit(tpl.path(), "x");
    let y = common::untagged_commit(tpl.path(), "y");
    let (a, b) = (edited_repo(&tpl), edited_repo(&tpl));
    let diff = splice_cmd()
        .current_dir(a.path())
        .args(["diff", "--ref", &x, "--json"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let diff = json(&diff);
    assert!(!diff["changes"].as_array().unwrap().is_empty());
    assert_eq!(porcelain(&a), "");
    let update = splice_cmd()
        .current_dir(b.path())
        .args(["update", "--ref", &x, "--yes", "--json"])
        .assert()
        .get_output()
        .stdout
        .clone();
    assert_eq!(diff["changes"], json(&update)["changes"]);

    // y is a descendant of x, so a lock at y makes x a non-descendant.
    let c = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            c.path().to_str().unwrap(),
        ])
        .args(["--ref", &y, "--data", "project=demo", "--yes", "--json"])
        .assert()
        .success();
    commit_all(c.path());
    splice_cmd()
        .current_dir(c.path())
        .args(["diff", "--ref", &x, "--json"])
        .assert()
        .code(2);
}
