mod common;

use common::splice_cmd;

#[test]
fn bad_flag_with_json_is_one_json_object_exit_2() {
    let out = splice_cmd()
        .args(["new", "--json", "--bogus"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "new");
}

/// Run, expect exit 2, and assert the clap text names every flag in `flags`.
fn usage_names(args: &[&str], flags: &[&str], phrase: &str) {
    let out = splice_cmd()
        .args(args)
        .assert()
        .code(2)
        .get_output()
        .clone();
    let err = String::from_utf8_lossy(&out.stderr).to_string();
    for f in flags {
        assert!(err.contains(f), "{args:?}: stderr lacks {f}: {err}");
    }
    assert!(
        err.contains(phrase),
        "{args:?}: stderr lacks {phrase:?}: {err}"
    );
}

#[test]
fn ref_and_track_conflict() {
    let out = splice_cmd()
        .args([
            "--json", "channel", "add", "x", "--ref", "v1", "--track", "main",
        ])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let msg = v["messages"].to_string();
    for t in ["--ref", "--track", "cannot be used with"] {
        assert!(msg.contains(t), "{t}: {msg}");
    }
    usage_names(
        &["channel", "add", "x", "--ref", "v1", "--track", "main"],
        &["--ref", "--track"],
        "cannot be used with",
    );
}

#[test]
fn channel_and_all_conflict() {
    usage_names(
        &["update", "--channel", "a", "--all"],
        &["--channel", "--all"],
        "cannot be used with",
    );
}

#[test]
fn filter_flags_need_channel() {
    for f in ["--only", "--exclude"] {
        usage_names(
            &["update", f, "a/**"],
            &[f, "--channel"],
            "required arguments were not provided",
        );
    }
}

#[test]
fn all_rejects_to() {
    usage_names(
        &["update", "--all", "--to", "2.0.0"],
        &["--all", "--to"],
        "cannot be used with",
    );
}

#[test]
fn all_rejects_data_only_exclude() {
    for f in ["--data", "--only", "--exclude"] {
        usage_names(
            &["update", "--all", f, "a=b"],
            &["--all", f],
            "cannot be used with",
        );
    }
}

#[test]
fn channel_flags_on_other_commands_are_not_implemented_yet() {
    for args in [
        vec!["check", "--channel", "rig"],
        vec!["diff", "--channel", "rig"],
    ] {
        let out = splice_cmd()
            .arg("--json")
            .args(&args)
            .assert()
            .code(2)
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out)
            .unwrap_or_else(|e| panic!("{args:?}: stdout is not one JSON object: {e}"));
        assert_eq!(v["ok"], false, "{args:?}");
        assert!(
            v["messages"].to_string().contains("not implemented yet"),
            "{args:?}: {v}"
        );
    }
}

#[test]
fn update_to_alone_is_valid() {
    // B3: `--to` needs no `--channel`; the failure is the missing lock, not clap.
    let dir = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .arg("-C")
        .arg(dir.path())
        .args(["--json", "update", "--allow-dirty", "--to", "1.0.0"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let msg = v["messages"].to_string();
    assert!(msg.contains(".splice/lock.toml"), "{msg}");
    assert!(!msg.contains("--channel"), "{msg}");
}

#[test]
fn clap_error_json_names_channel_command() {
    for (args, name) in [
        (vec!["--json", "channel", "add"], "channel add"),
        (vec!["--json", "channel", "remove"], "channel remove"),
    ] {
        let out = splice_cmd()
            .args(args)
            .assert()
            .code(2)
            .get_output()
            .stdout
            .clone();
        let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["command"], name);
    }
}

#[test]
fn help_lists_channel() {
    let out = splice_cmd()
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&out).contains("channel"));
}

#[test]
fn channel_remove_is_a_stub_until_implemented() {
    let out = splice_cmd()
        .args(["--json", "channel", "remove", "rig"])
        .assert()
        .code(2)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert!(
        v["messages"].to_string().contains("not implemented yet"),
        "{v}"
    );
}
