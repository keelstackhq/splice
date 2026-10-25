//! `splice check` against the fixture template (spec §8.7).

mod common;

use common::{commit_all, splice_cmd, template_repo};
use tempfile::TempDir;

/// `splice new` of the app piece from `tpl` (at `--ref` when given) into a committed repo.
fn new_repo(tpl: &TempDir, git_ref: Option<&str>) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    let mut cmd = splice_cmd();
    cmd.args([
        "new",
        tpl.path().to_str().unwrap(),
        repo.path().to_str().unwrap(),
    ])
    .args(["--piece", "app:web", "--data", "project=demo", "--yes"]);
    if let Some(r) = git_ref {
        cmd.args(["--ref", r]);
    }
    cmd.assert().success();
    commit_all(repo.path());
    repo
}

fn check(repo: &TempDir, args: &[&str]) -> (i32, serde_json::Value) {
    let out = splice_cmd()
        .current_dir(repo.path())
        .arg("check")
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).expect("stdout is exactly one JSON object");
    (out.status.code().unwrap(), v)
}

fn messages(v: &serde_json::Value) -> String {
    v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn clean_repo_passes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "check");
    assert_eq!(v["changes"], serde_json::json!([]));
    // Online too: the lock is already at the latest stable tag.
    assert_eq!(check(&repo, &[]).0, 0);
}

#[test]
fn owned_drift_fails() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    std::fs::write(repo.path().join(".editorconfig"), "root = false\n").unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".editorconfig","action":"update","class":"owned"}])
    );
    // A deleted owned file is drift too.
    std::fs::remove_file(repo.path().join(".editorconfig")).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(v["changes"][0]["path"], ".editorconfig");
}

#[test]
fn markers_fail_and_stray_separator_does_not() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let ci = repo.path().join(".github/workflows/ci.yml");
    let text = std::fs::read_to_string(&ci).unwrap();
    std::fs::write(&ci, text.replace("ubuntu-latest", "macos-latest")).unwrap();
    commit_all(repo.path());
    splice_cmd()
        .current_dir(repo.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .code(1);
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".github/workflows/ci.yml","action":"conflict","class":"shared"}])
    );

    // Resolved; a lone `=======` in a seed (not in the lock) and in a shared file is plain text.
    std::fs::write(&ci, "name: ci\non: [push]\n").unwrap();
    for rel in ["README.md", "notes.txt"] {
        let p = repo.path().join(rel);
        let mut t = std::fs::read_to_string(&p).unwrap();
        t.push_str("=======\n");
        std::fs::write(p, t).unwrap();
    }
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!((code, v["changes"].clone()), (0, serde_json::json!([])));

    // CRLF line endings: markers are still found.
    std::fs::write(
        &ci,
        "name: ci\r\n<<<<<<< ours\r\na\r\n||||||| base\r\nb\r\n=======\r\nc\r\n>>>>>>> template\r\n",
    )
    .unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([{"path":".github/workflows/ci.yml","action":"conflict","class":"shared"}])
    );
    assert!(messages(&v).contains("line 2"), "{v}");
}

#[test]
fn untagged_lock_only_sees_tags_that_descend_from_it() {
    let tpl = template_repo(&["v1"]);
    let x = common::untagged_commit(tpl.path(), "x");
    let repo = new_repo(&tpl, Some(&x));
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 0, "{v}");
    assert!(!messages(&v).contains("update available"), "{v}");

    common::untagged_commit(tpl.path(), "next");
    common::git(tpl.path(), "2026-02-02T00:00:00+00:00", &["tag", "v2.0.0"]);
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 1);
    assert!(
        messages(&v).contains(&format!("update available: 0.0.0+g{} → 2.0.0", &x[..7])),
        "{v}"
    );
}

#[test]
fn update_available() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 1);
    assert_eq!(v["ok"], false);
    assert!(
        messages(&v).contains("update available: 1.0.0 → 2.0.0"),
        "{v}"
    );
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 0);
    assert!(!messages(&v).contains("update available"));
}

#[test]
fn fetch_failure_is_message_not_error() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let path = tpl.path().to_path_buf();
    drop(tpl); // removes the template directory
    assert!(!path.exists());
    let (code, v) = check(&repo, &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["ok"], true);
    assert!(messages(&v).contains("could not look for updates"), "{v}");
}

#[test]
fn check_writes_nothing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, Some("v1.0.0"));
    let lock = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();
    check(&repo, &[]);
    assert_eq!(
        std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
        lock
    );
    assert_eq!(
        common::git_stdout(repo.path(), &["status", "--porcelain"]),
        ""
    );
}

#[test]
fn no_lock_is_usage_error() {
    let dir = tempfile::tempdir().unwrap();
    splice_cmd()
        .current_dir(dir.path())
        .args(["check", "--json"])
        .assert()
        .code(2);
}

#[test]
fn unsafe_lock_path_is_refused_not_read() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    let lock = repo.path().join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock).unwrap();
    let text = text.replacen(
        "[files]\n",
        "[files]\n\"../outside.txt\" = { class = \"owned\", sha256 = \"00\" }\n",
        1,
    );
    assert!(text.contains("../outside.txt"));
    std::fs::write(&lock, text).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 3, "{v}");
    assert!(messages(&v).contains("../outside.txt"), "{v}");
}

#[test]
fn markers_report_the_lock_class() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, None);
    let p = repo.path().join(".editorconfig");
    let mut t = std::fs::read_to_string(&p).unwrap();
    t.push_str("<<<<<<< ours\na\n||||||| base\nb\n=======\nc\n>>>>>>> template\n");
    std::fs::write(&p, t).unwrap();
    let (code, v) = check(&repo, &["--offline"]);
    assert_eq!(code, 1);
    assert_eq!(
        v["changes"],
        serde_json::json!([
            {"path":".editorconfig","action":"update","class":"owned"},
            {"path":".editorconfig","action":"conflict","class":"owned"}
        ])
    );
}

// ---- channels and sections (Task 15) ----

mod channels {
    use super::{check, messages};
    use crate::common::*;
    use tempfile::TempDir;

    fn src(ch: &TempDir) -> String {
        format!("{}//channel", ch.path().display())
    }

    fn paths(v: &serde_json::Value) -> Vec<String> {
        v["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["path"].as_str().unwrap().to_string())
            .collect()
    }

    /// Main repo (app piece) plus channel rig at v1 with claude + core, committed.
    fn main_and_rig(versions: &[&str]) -> (TempDir, TempDir, TempDir) {
        let tpl = template_repo(&["v1"]);
        let repo = new_repo_at(&tpl, "v1.0.0");
        let ch = channel_repo(versions);
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
        (repo, ch, tpl)
    }

    #[test]
    fn check_aggregates_sources() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1"]);
        assert_eq!(check(&repo, &["--offline"]).0, 0);
        std::fs::write(repo.path().join(".claude/agents/reviewer.md"), "edited\n").unwrap();
        std::fs::write(repo.path().join(".editorconfig"), "root = false\n").unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 1);
        assert_eq!(
            v["changes"],
            serde_json::json!([
                {"path":".claude/agents/reviewer.md","action":"update","class":"owned"},
                {"path":".editorconfig","action":"update","class":"owned"}
            ])
        );
        let m = messages(&v);
        assert!(m.contains("channel rig"), "{m}");
    }

    #[test]
    fn check_section_drift() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1"]);
        let p = repo.path().join("AGENTS.md");
        let text = std::fs::read_to_string(&p).unwrap();
        let edited = text.replace("Test first", "EDITED");
        assert_ne!(text, edited);
        std::fs::write(&p, edited).unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 1);
        assert_eq!(
            v["changes"],
            serde_json::json!([{"path":"AGENTS.md#rig","action":"update","class":"owned"}])
        );
    }

    /// `text` with the blank lines next to the rig markers removed (the pre-0.2 layout).
    fn without_marker_padding(text: &str) -> String {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut out = String::new();
        for (i, l) in lines.iter().enumerate() {
            let blank = l.trim().is_empty();
            let after_begin = i > 0 && lines[i - 1].contains("splice:rig:begin");
            let before_end = lines
                .get(i + 1)
                .is_some_and(|n| n.contains("splice:rig:end"));
            if !(blank && (after_begin || before_end)) {
                out.push_str(l);
            }
        }
        out
    }

    #[test]
    fn check_section_is_clean_with_formatter_old_and_crlf_layouts() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1"]);
        let p = repo.path().join("AGENTS.md");
        let written = std::fs::read_to_string(&p).unwrap();
        let old = without_marker_padding(&written);
        assert_ne!(old, written, "splice writes blank lines around the body");
        // What oxfmt/prettier leave: one blank line after begin and before end; extra blank
        // lines next to the markers are tolerated too.
        let formatted = old
            .replace("edit the source -->\n", "edit the source -->\n\n")
            .replace("\n<!-- splice:rig:end -->", "\n\n<!-- splice:rig:end -->");
        let roomy = old
            .replace("edit the source -->\n", "edit the source -->\n\n\n")
            .replace("\n<!-- splice:rig:end -->", "\n\n\n<!-- splice:rig:end -->");
        for (name, text) in [
            ("old layout, old lock", old.clone()),
            ("formatter layout", formatted.clone()),
            ("extra blank lines", roomy),
            ("crlf old", old.replace('\n', "\r\n")),
            ("crlf formatter", formatted.replace('\n', "\r\n")),
        ] {
            std::fs::write(&p, &text).unwrap();
            let (code, v) = check(&repo, &["--offline"]);
            assert_eq!(code, 0, "{name}: {v}");
        }
        // a real edit inside the formatter-shaped body is still drift
        std::fs::write(&p, formatted.replace("Test first", "EDITED")).unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 1);
        assert_eq!(
            v["changes"],
            serde_json::json!([{"path":"AGENTS.md#rig","action":"update","class":"owned"}])
        );
    }

    #[test]
    fn check_section_markers_broken_and_host_gone() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1"]);
        let p = repo.path().join("AGENTS.md");
        let text = std::fs::read_to_string(&p).unwrap();
        let no_end: String = text
            .lines()
            .filter(|l| !l.contains("splice:rig:end"))
            .map(|l| format!("{l}\n"))
            .collect();
        std::fs::write(&p, no_end).unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 1);
        assert_eq!(paths(&v), ["AGENTS.md#rig"]);
        let m = messages(&v);
        assert!(m.contains("unbalanced splice:rig marker"), "{m}");
        // A deleted host is drift too.
        std::fs::remove_file(&p).unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 1);
        assert_eq!(paths(&v), ["AGENTS.md#rig"]);
    }

    #[test]
    fn check_tracked_head_moved() {
        let repo = bare_repo();
        let ch = channel_repo(&["v1"]);
        chan_commit(
            repo.path(),
            &["channel", "add", &src(&ch), "--track", "main"],
        );
        let locked = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
        assert_eq!(check(&repo, &[]).0, 0);
        let head = track_commit(ch.path(), true);
        let (code, v) = check(&repo, &[]);
        assert_eq!(code, 1);
        assert_eq!(v["changes"], serde_json::json!([]));
        let lock = std::fs::read_to_string(repo.path().join(".splice/channels/rig.toml")).unwrap();
        let old = lock
            .lines()
            .find_map(|l| l.strip_prefix("commit = \""))
            .unwrap()
            .trim_end_matches('"')
            .to_string();
        assert_ne!(old, locked);
        let m = messages(&v);
        assert!(
            m.contains(&format!(
                "channel rig: update available {} → {} (main)",
                &old[..7],
                &head[..7]
            )),
            "{m}"
        );
        assert_eq!(check(&repo, &["--offline"]).0, 0);
    }

    #[test]
    fn check_tracked_outside_change() {
        let repo = bare_repo();
        let ch = channel_repo(&["v1"]);
        chan_commit(
            repo.path(),
            &["channel", "add", &src(&ch), "--track", "main"],
        );
        track_commit(ch.path(), false);
        let (code, v) = check(&repo, &[]);
        assert_eq!(code, 0, "{v}");
        assert_eq!(messages(&v), "");
    }

    #[test]
    fn check_tag_update_available() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1", "v2"]);
        let (code, v) = check(&repo, &[]);
        assert_eq!(code, 1);
        let m = messages(&v);
        assert!(
            m.contains("channel rig: update available 1.0.0 → 2.0.0 (tag)"),
            "{m}"
        );
        assert!(
            !m.contains("\nupdate available") && !m.starts_with("update available"),
            "{m}"
        );
        assert_eq!(check(&repo, &["--offline"]).0, 0);
    }

    #[test]
    fn check_untagged_channel_ignores_older_tags() {
        let tpl = template_repo(&["v1"]);
        let repo = new_repo_at(&tpl, "v1.0.0");
        let ch = channel_repo(&["v1"]);
        let x = track_commit(ch.path(), true);
        chan_commit(
            repo.path(),
            &[
                "channel",
                "add",
                &src(&ch),
                "--ref",
                &x,
                "--piece",
                "claude",
            ],
        );
        let (code, v) = check(&repo, &[]);
        assert_eq!(code, 0, "{v}");
        assert!(!messages(&v).contains("update available"), "{v}");
    }

    #[test]
    fn check_channel_only_repo() {
        let repo = bare_repo();
        let ch = channel_repo(&["v1"]);
        chan_commit(repo.path(), &["channel", "add", &src(&ch)]);
        let (code, v) = check(&repo, &[]);
        assert_eq!(code, 0, "{v}");
        assert_eq!(v["ok"], true);
    }

    #[test]
    fn check_single_channel() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1"]);
        std::fs::write(repo.path().join(".editorconfig"), "root = false\n").unwrap();
        assert_eq!(check(&repo, &["--offline"]).0, 1);
        let (code, v) = check(&repo, &["--offline", "--channel", "rig"]);
        assert_eq!(code, 0, "{v}");
        std::fs::write(repo.path().join(".claude/agents/reviewer.md"), "x\n").unwrap();
        let (code, v) = check(&repo, &["--offline", "--channel", "rig"]);
        assert_eq!(code, 1);
        assert_eq!(paths(&v), [".claude/agents/reviewer.md"]);
    }

    #[test]
    fn check_unknown_channel_and_corrupt_lock() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1"]);
        let (code, v) = check(&repo, &["--channel", "nope"]);
        assert_eq!(code, 2);
        assert!(messages(&v).contains("unknown channel `nope`"), "{v}");
        std::fs::write(
            repo.path().join(".splice/channels/rig.toml"),
            "not toml [[\n",
        )
        .unwrap();
        let (code, _) = check(&repo, &["--offline"]);
        assert_eq!(code, 3);
    }

    #[test]
    fn check_is_read_only() {
        let (repo, _ch, _tpl) = main_and_rig(&["v1", "v2"]);
        std::fs::write(repo.path().join(".claude/agents/reviewer.md"), "x\n").unwrap();
        let before = tree_snapshot(repo.path());
        check(&repo, &[]);
        assert_eq!(tree_snapshot(repo.path()), before);
    }
}

mod channel_paths {
    use super::{check, messages};
    use crate::common::*;

    fn rig_repo() -> (tempfile::TempDir, tempfile::TempDir) {
        let repo = bare_repo();
        let ch = channel_repo(&["v1"]);
        chan_commit(
            repo.path(),
            &[
                "channel",
                "add",
                &format!("{}//channel", ch.path().display()),
                "--piece",
                "claude",
            ],
        );
        (repo, ch)
    }

    #[test]
    fn unsafe_channel_lock_path_is_refused_and_names_the_lock() {
        let (repo, _ch) = rig_repo();
        let lock = repo.path().join(".splice/channels/rig.toml");
        let text = std::fs::read_to_string(&lock).unwrap();
        std::fs::write(
            &lock,
            text.replace(".claude/agents/reviewer.md", "../evil.md"),
        )
        .unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 3, "{v}");
        let m = messages(&v);
        assert!(
            m.contains(".splice/channels/rig.toml") && m.contains("unsafe path"),
            "{m}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_section_host_is_refused() {
        let (repo, _ch) = rig_repo();
        std::fs::remove_file(repo.path().join("AGENTS.md")).unwrap();
        std::os::unix::fs::symlink("README.md", repo.path().join("AGENTS.md")).unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 3, "{v}");
        assert!(messages(&v).contains(".splice/channels/rig.toml"), "{v}");
    }

    #[test]
    fn non_utf8_section_host_is_drift_with_a_message() {
        let (repo, _ch) = rig_repo();
        std::fs::write(repo.path().join("AGENTS.md"), [0xff, 0xfe, 0x00, b'x']).unwrap();
        let (code, v) = check(&repo, &["--offline"]);
        assert_eq!(code, 1, "{v}");
        assert_eq!(v["changes"][0]["path"], "AGENTS.md#rig");
        assert!(messages(&v).contains("not valid UTF-8"), "{v}");
    }
}
