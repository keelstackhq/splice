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

/// The `## Channels` section of the README names only flags that exist.
#[test]
fn readme_channel_flags_exist() {
    let readme = std::fs::read_to_string("README.md").unwrap();
    let start = readme
        .find("\n## Channels\n")
        .expect("README has a ## Channels section");
    let rest = &readme[start + 1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |i| i + 3);
    let section = &rest[..end];
    let mut help = String::new();
    for args in [
        vec!["channel", "add", "--help"],
        vec!["channel", "remove", "--help"],
        vec!["update", "--help"],
        vec!["check", "--help"],
        vec!["add", "--help"],
        vec!["diff", "--help"],
    ] {
        let out = splice_cmd()
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        help.push_str(&String::from_utf8_lossy(&out));
    }
    let flag = regex::Regex::new(r"--[a-z][a-z-]*").unwrap();
    let mut seen = 0;
    for m in flag.find_iter(section) {
        seen += 1;
        assert!(
            help.contains(m.as_str()),
            "README Channels names unknown flag {}",
            m.as_str()
        );
    }
    assert!(
        seen >= 8,
        "the Channels section should show the channel flags"
    );
}
