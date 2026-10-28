//! Template commands (`[[post]]`, migration `[[step]]`) run without token-like environment
//! variables (spec §8.1, §10 item 22), so template code cannot read the update bot's token.

mod common;

use common::{commit_all, git, inline_template, splice_cmd, stage_all};
use tempfile::TempDir;

const DATE: &str = "2026-01-01T00:00:00+00:00";
const SECRETS: [&str; 4] = ["s3cret-gh", "s3cret-ghs", "s3cret-npm", "s3cret-url"];

const PIECE: &str = r#"id = "base"
description = "Base piece"
multiple = false

[files]
owned = ["a.txt"]

[[post]]
run = "env | sort"
when = ["new"]
"#;

/// Give a command the secrets plus one harmless variable.
fn with_env(cmd: &mut assert_cmd::Command) -> &mut assert_cmd::Command {
    cmd.env("GH_TOKEN", "s3cret-gh")
        .env("GITHUB_TOKEN", "s3cret-ghs")
        .env("NPM_TOKEN", "s3cret-npm")
        .env("ACTIONS_ID_TOKEN_REQUEST_URL", "s3cret-url")
        .env("SPLICE_KEEP_ME", "visible")
}

fn tagged_template() -> TempDir {
    let t = inline_template(&[("base", PIECE, &[("a.txt", "one\n")])]);
    git(t.path(), DATE, &["init", "-q", "-b", "main"]);
    stage_all(t.path(), DATE);
    git(t.path(), DATE, &["commit", "-q", "-m", "v1"]);
    git(t.path(), DATE, &["tag", "v1.0.0"]);
    t
}

fn assert_clean(stderr: &str) {
    assert!(stderr.contains("SPLICE_KEEP_ME=visible"), "{stderr}");
    for s in SECRETS {
        assert!(!stderr.contains(s), "leaked {s}:\n{stderr}");
    }
    assert!(!stderr.contains("GH_TOKEN"), "{stderr}");
}

#[test]
fn post_runs_without_tokens() {
    let tpl = tagged_template();
    let dest = tempfile::tempdir().unwrap();
    let mut cmd = splice_cmd();
    let out = with_env(&mut cmd)
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            dest.path().to_str().unwrap(),
        ])
        .args(["--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    assert_clean(&String::from_utf8_lossy(&out.stderr));
}

#[test]
fn migration_step_runs_without_tokens() {
    let tpl = tagged_template();
    let dest = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            dest.path().to_str().unwrap(),
        ])
        .args(["--yes", "--json", "--no-post"])
        .assert()
        .success();
    commit_all(dest.path());

    std::fs::write(tpl.path().join("pieces/base/files/a.txt"), "two\n").unwrap();
    std::fs::create_dir_all(tpl.path().join("migrations")).unwrap();
    std::fs::write(
        tpl.path().join("migrations/2.0.0.toml"),
        "[[step]]\nrun = \"env | sort\"\n",
    )
    .unwrap();
    stage_all(tpl.path(), DATE);
    git(tpl.path(), DATE, &["commit", "-q", "-m", "v2"]);
    git(tpl.path(), DATE, &["tag", "v2.0.0"]);

    let mut cmd = splice_cmd();
    let out = with_env(&mut cmd)
        .current_dir(dest.path())
        .args(["update", "--yes", "--json"])
        .assert()
        .success()
        .get_output()
        .clone();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_clean(&stderr);
    assert!(stderr.contains("SPLICE_TO_VERSION=2.0.0"), "{stderr}");
}
