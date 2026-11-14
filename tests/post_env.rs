//! Template commands (`[[post]]`, migration `[[step]]`) run without secret-like environment
//! variables (spec §8.1, §10 item 22), so template code cannot read the update bot's token.

mod common;

use common::{commit_all, git, inline_template, splice_cmd, stage_all};
use tempfile::TempDir;

const DATE: &str = "2026-01-01T00:00:00+00:00";
const SECRETS: [&str; 9] = [
    "s3cret-gh",
    "s3cret-ghs",
    "s3cret-npm",
    "s3cret-url",
    "s3cret-sops",
    "s3cret-db",
    "s3cret-pat",
    "s3cret-client",
    "s3cret-auth",
];

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
    cmd.env("SOPS_AGE_KEY", "s3cret-sops")
        .env("DB_PASSWORD", "s3cret-db")
        .env("MY_PAT", "s3cret-pat")
        .env("CLIENT_SECRET", "s3cret-client")
        .env("GH_TOKEN", "s3cret-gh")
        .env("GITHUB_TOKEN", "s3cret-ghs")
        .env("NPM_TOKEN", "s3cret-npm")
        .env("ACTIONS_ID_TOKEN_REQUEST_URL", "s3cret-url")
        .env("SPLICE_KEEP_ME", "visible");
    // A name with `/` and `:` cannot be set on Windows.
    #[cfg(unix)]
    cmd.env("npm_config_//registry.npmjs.org/:_authToken", "s3cret-auth");
    cmd
}

fn tagged_template() -> TempDir {
    let t = inline_template(&[("base", PIECE, &[("a.txt", "one\n")])]);
    git(t.path(), DATE, &["init", "-q", "-b", "main"]);
    stage_all(t.path(), DATE);
    git(t.path(), DATE, &["commit", "-q", "-m", "v1"]);
    git(t.path(), DATE, &["tag", "v1.0.0"]);
    t
}

#[test]
fn post_runs_without_git_credential_helpers() {
    let marker = tempfile::tempdir().unwrap();
    let marker = marker.path().join("helper-ran");
    let gitconfig = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        gitconfig.path(),
        format!(
            "[credential]\n\thelper = \"!f() {{ echo ran >> '{}'; }}; f\"\n",
            marker.display()
        ),
    )
    .unwrap();
    let piece = PIECE.replace(
        "run = \"env | sort\"",
        "run = \"printf 'protocol=https\\\\nhost=example.com\\\\n\\\\n' | git credential fill >/dev/null 2>&1 || true\"",
    );
    let t = inline_template(&[("base", &piece, &[("a.txt", "one\n")])]);
    git(t.path(), DATE, &["init", "-q", "-b", "main"]);
    stage_all(t.path(), DATE);
    git(t.path(), DATE, &["commit", "-q", "-m", "v1"]);
    git(t.path(), DATE, &["tag", "v1.0.0"]);
    let dest = tempfile::tempdir().unwrap();
    splice_cmd()
        .env("GIT_CONFIG_GLOBAL", gitconfig.path())
        .args([
            "new",
            t.path().to_str().unwrap(),
            dest.path().to_str().unwrap(),
        ])
        .args(["--yes", "--json"])
        .assert()
        .success();
    assert!(
        !marker.exists(),
        "the post command reached the credential helper (spec §10 item 26)"
    );
}

#[test]
fn post_drops_inherited_git_config_injection() {
    let tpl = tagged_template();
    let dest = tempfile::tempdir().unwrap();
    let out = splice_cmd()
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraheader")
        .env("GIT_CONFIG_VALUE_0", "AUTHORIZATION: basic s3cret-hdr")
        .env("GIT_CONFIG_PARAMETERS", "'http.extraheader=s3cret-params'")
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
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("s3cret-hdr") && !stderr.contains("s3cret-params"),
        "{stderr}"
    );
    assert!(stderr.contains("GIT_CONFIG_COUNT=2"), "{stderr}");
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
