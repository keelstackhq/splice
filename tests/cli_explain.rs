//! `splice explain` against the fixture template.

mod common;

use common::{splice_cmd, template_repo};
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

fn json_of(cmd: &mut assert_cmd::Command) -> serde_json::Value {
    let out = cmd.output().unwrap();
    assert!(out.status.success(), "{out:?}");
    serde_json::from_slice(&out.stdout).expect("stdout is exactly one JSON object")
}

fn explain_json(repo: &TempDir, path: &str) -> serde_json::Value {
    let mut cmd = splice_cmd();
    cmd.current_dir(repo.path())
        .args(["explain", path, "--json"]);
    json_of(&mut cmd)
}

/// Human output of a successful `explain`.
fn stdout_of(repo: &TempDir, path: &str) -> String {
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(["explain", path])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap()
}

#[test]
fn explains_classes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web", "--data", "project=demo"]);
    let out = stdout_of(&repo, "apps/web/package.json");
    for want in ["shared", "app/web", "merge=json"] {
        assert!(out.contains(want), "missing `{want}` in {out:?}");
    }
    let v = explain_json(&repo, "apps/web/src/main.ts");
    assert_eq!(
        v["changes"][0],
        serde_json::json!({"path":"apps/web/src/main.ts","action":"skip","class":"seed"})
    );
    let msgs = v["messages"].as_array().unwrap();
    assert!(msgs.iter().any(|m| m == "glob=src/**"), "{msgs:?}");
    assert!(msgs.iter().any(|m| m == "piece=app/web"), "{msgs:?}");
    // No channel locks: explain stays exactly M2 (no `source=` message or column).
    assert_eq!(msgs.len(), 3, "{msgs:?}");
    assert!(!out.contains("source="), "{out}");
}

#[test]
fn normalises_dot_slash_and_backslashes() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    for p in ["./apps/web/tsconfig.json", "apps\\web\\tsconfig.json"] {
        let v = explain_json(&repo, p);
        assert_eq!(v["changes"][0]["path"], "apps/web/tsconfig.json", "{p}");
        assert_eq!(v["changes"][0]["class"], "owned");
    }
}

#[test]
fn unmanaged_path() {
    let (_tpl, repo) = new_repo(&[]);
    std::fs::write(repo.path().join("notes.md"), "mine\n").unwrap();
    let v = explain_json(&repo, "notes.md");
    assert_eq!(v["ok"], true);
    assert_eq!(v["changes"], serde_json::json!([]));
    assert_eq!(v["messages"], serde_json::json!(["unmanaged"]));
    assert!(stdout_of(&repo, "notes.md").contains("unmanaged"));
}

#[test]
fn path_outside_the_repo_is_a_usage_error() {
    let (_tpl, repo) = new_repo(&[]);
    for p in ["../x", "/etc/passwd"] {
        splice_cmd()
            .current_dir(repo.path())
            .args(["explain", p])
            .assert()
            .code(2);
    }
}

#[test]
fn works_offline_after_new() {
    let (tpl, repo) = new_repo(&["--piece", "app:web"]);
    drop(tpl); // removes the template repository
    let v = explain_json(&repo, "apps/web/tsconfig.json");
    assert_eq!(v["changes"][0]["class"], "owned");
}

#[test]
fn human_mode_prints_each_line_once() {
    let (_tpl, repo) = new_repo(&["--piece", "app:web"]);
    std::fs::write(repo.path().join("notes.md"), "mine\n").unwrap();
    for p in ["apps/web/package.json", "notes.md"] {
        let out = splice_cmd()
            .current_dir(repo.path())
            .args(["explain", p])
            .assert()
            .success();
        let stderr = String::from_utf8_lossy(&out.get_output().stderr).into_owned();
        assert_eq!(stderr, "", "{p}: the stdout line is not repeated on stderr");
    }
}

mod channels {
    use crate::common::*;
    use tempfile::TempDir;

    fn src(ch: &TempDir) -> String {
        format!("{}//channel", ch.path().display())
    }

    fn explain(repo: &std::path::Path, path: &str, json: bool) -> (i32, String) {
        let mut cmd = splice_cmd();
        cmd.current_dir(repo).args(["explain", path]);
        if json {
            cmd.arg("--json");
        }
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
        )
    }

    fn msgs(stdout: &str) -> Vec<String> {
        let v: serde_json::Value = serde_json::from_str(stdout).unwrap();
        v["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m.as_str().unwrap().to_string())
            .collect()
    }

    fn setup() -> (TempDir, TempDir, TempDir) {
        let tpl = template_repo(&["v1"]);
        let repo = new_repo_at(&tpl, "v1.0.0");
        let ch = channel_repo(&["v1"]);
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
    fn explain_channel_file() {
        let (_t, repo, _c) = setup();
        let (code, out) = explain(repo.path(), ".claude/agents/reviewer.md", true);
        assert_eq!(code, 0, "{out}");
        let m = msgs(&out);
        assert_eq!(m[0], "piece=claude/claude", "{m:?}");
        assert!(m[1].starts_with("glob="), "{m:?}");
        assert_eq!(m[2], "merge=none", "{m:?}");
        assert_eq!(m[3], "source=channel:rig", "{m:?}");
        assert_eq!(m.len(), 4, "{m:?}");
    }

    #[test]
    fn explain_section_host() {
        let (_t, repo, _c) = setup();
        let (code, out) = explain(repo.path(), "AGENTS.md", true);
        assert_eq!(code, 0, "{out}");
        assert_eq!(msgs(&out), ["unmanaged", "section=rig"]);
        let (_, human) = explain(repo.path(), "AGENTS.md", false);
        assert!(
            human.contains("unmanaged") && human.contains("section=rig"),
            "{human}"
        );
    }

    #[test]
    fn explain_main_file_has_source_main() {
        let (_t, repo, _c) = setup();
        let (_, out) = explain(repo.path(), ".editorconfig", true);
        let m = msgs(&out);
        assert!(m[0].starts_with("piece="), "{m:?}");
        assert_eq!(m.last().unwrap(), "source=main", "{m:?}");
        assert_eq!(m.len(), 4, "{m:?}");
    }

    #[test]
    fn explain_human_has_source_column() {
        let (_t, repo, _c) = setup();
        let (code, out) = explain(repo.path(), ".editorconfig", false);
        assert_eq!(code, 0);
        assert_eq!(out.lines().count(), 1, "{out}");
        assert!(out.contains("source=main"), "{out}");
        let (_, out) = explain(repo.path(), ".claude/agents/reviewer.md", false);
        assert!(out.contains("source=channel:rig"), "{out}");
    }

    #[test]
    fn explain_channel_only_repo_and_unmanaged() {
        let repo = bare_repo();
        let ch = channel_repo(&["v1"]);
        chan_commit(
            repo.path(),
            &["channel", "add", &src(&ch), "--piece", "claude"],
        );
        let (code, out) = explain(repo.path(), ".claude/agents/reviewer.md", true);
        assert_eq!(code, 0, "{out}");
        assert!(msgs(&out).contains(&"source=channel:rig".to_string()));
        let (code, out) = explain(repo.path(), "README.md", true);
        assert_eq!(code, 0);
        assert_eq!(msgs(&out), ["unmanaged"]);
    }
}
