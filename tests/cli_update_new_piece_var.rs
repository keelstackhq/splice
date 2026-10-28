//! A "missing value" error on `update` suggests `--data` only where `update` accepts it for that
//! var: new template vars of a plain `update`. Piece vars, `--channel` and `--all` name the
//! default / interactive route instead.

mod common;

use common::{chan_commit, channel_repo, commit_all, commit_tag, git, inline_template, splice_cmd};
use tempfile::TempDir;

const PIECE: &str = "id = \"base\"\ndescription = \"d\"\nmultiple = false\n";
const NEW_VAR: &str = "[vars]\nextra = { prompt = \"e\" }\n";

/// A repo created from v1 of a one-piece template; v2 (tagged) has `edit` applied to the template.
fn v2_repo(edit: impl FnOnce(&std::path::Path)) -> (TempDir, TempDir) {
    let tpl = inline_template(&[("base", PIECE, &[("a.txt", "a\n")])]);
    git(
        tpl.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(tpl.path(), "v1", "v1.0.0");
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--yes", "--json"])
        .assert()
        .success();
    edit(tpl.path());
    commit_tag(tpl.path(), "v2", "v2.0.0");
    (tpl, repo)
}

fn output(repo: &TempDir, args: &[&str]) -> String {
    commit_all(repo.path());
    let out = splice_cmd()
        .current_dir(repo.path())
        .args(args)
        .args(["--yes", "--json"])
        .assert()
        .code(2)
        .get_output()
        .clone();
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn no_data_hint(all: &str) {
    assert!(all.contains("missing value for var `extra`"), "{all}");
    assert!(!all.contains("--data"), "{all}");
    assert!(all.contains("default") && all.contains("terminal"), "{all}");
}

fn add_template_var(tpl: &std::path::Path) {
    let toml = tpl.join("splice.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(toml, format!("{text}{NEW_VAR}")).unwrap();
}

#[test]
fn new_piece_var_hint_does_not_suggest_data() {
    let (_tpl, repo) = v2_repo(|t| {
        std::fs::write(
            t.join("pieces/base/piece.toml"),
            format!("{PIECE}{NEW_VAR}"),
        )
        .unwrap();
    });
    no_data_hint(&output(&repo, &["update"]));
}

#[test]
fn new_template_var_hint_on_plain_update_keeps_data() {
    let (_tpl, repo) = v2_repo(add_template_var);
    let all = output(&repo, &["update"]);
    assert!(all.contains("pass --data extra=…"), "{all}");
}

#[test]
fn new_template_var_hint_on_all_does_not_suggest_data() {
    let (_tpl, repo) = v2_repo(add_template_var);
    no_data_hint(&output(&repo, &["update", "--all"]));
}

#[test]
fn new_channel_template_var_hint_does_not_suggest_data() {
    let tpl = common::template_repo(&["v1"]);
    let repo = common::new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let src = format!("{}//channel", ch.path().display());
    chan_commit(
        repo.path(),
        &[
            "channel",
            "add",
            &src,
            "--ref",
            "v1.0.0",
            "--piece",
            "claude",
            "--piece",
            "skills:claude",
            "--piece",
            "hooks",
            "--data",
            "dir=claude",
        ],
    );
    add_template_var(&ch.path().join("channel"));
    commit_tag(ch.path(), "v3", "v3.0.0");
    no_data_hint(&output(&repo, &["update", "--channel", "rig"]));
}
