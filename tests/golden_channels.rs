//! Golden `splice update --all` for a main template plus a channel, v1 → v2, with user edits
//! (channels spec §8, main spec §8). The main lock must stay what M2 writes.

mod common;

use common::{
    USER_README, apply_user_edits, chan, chan_commit, channel_repo, commit_all, edit, new_repo_at,
    redact, template_repo, tree_snapshot,
};
use std::path::Path;
use tempfile::TempDir;

/// Main fixture at v1 with the main template's user edits, channel `rig` (claude, skills:claude,
/// skills:agents, core, hooks) added at v1 and committed. Returns the repo and the channel source
/// repo (v1 only; `update --all` needs v2, so the channel repo gets both tags).
fn golden_repo() -> (TempDir, TempDir, TempDir) {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let src = format!("{}//channel", ch.path().display());
    let r = repo.path();
    chan_commit(
        r,
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
    chan_commit(
        r,
        &[
            "add",
            "skills",
            "--channel",
            "rig",
            "--name",
            "agents",
            "--data",
            "dir=agents",
        ],
    );
    apply_user_edits(&repo);
    (tpl, repo, ch)
}

fn read(r: &Path, rel: &str) -> String {
    std::fs::read_to_string(r.join(rel)).unwrap()
}

fn all_json(repo: &TempDir, extra: &[&str], code: i32) -> serde_json::Value {
    let mut args = vec!["update", "--all"];
    args.extend(extra);
    let out = chan(repo.path(), &args)
        .code(code)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn golden_main_and_channel_v1_to_v2() {
    let (_tpl, repo, _ch) = golden_repo();
    let r = repo.path();
    // Channel-side user edits: outside the AGENTS.md block, inside the CLAUDE.md block (the
    // fragment is unchanged in v2, so the edit is kept without --force), and the seed notes.md.
    edit(&repo, "AGENTS.md", |t| {
        format!("# My rules\n\n{t}\nMy footer.\n")
    });
    edit(&repo, "CLAUDE.md", |t| {
        t.replace("See `AGENTS.md` for the rules.", "See `AGENTS.md`, mine.")
    });
    edit(&repo, "docs/agents/notes.md", |t| {
        format!("{t}my seed note\n")
    });
    commit_all(r);
    let v = all_json(&repo, &[], 1); // the main ci.yml conflict
    insta::assert_json_snapshot!(v);
    insta::assert_snapshot!(redact(&tree_snapshot(r)));
    let agents = read(r, "AGENTS.md");
    assert!(agents.starts_with("# My rules\n") && agents.contains("My footer."));
    assert!(
        agents.contains("- Keep files small and focused."),
        "{agents}"
    );
    assert!(
        read(r, "CLAUDE.md").contains("mine."),
        "inside-block edit kept"
    );
    assert!(read(r, "docs/agents/notes.md").contains("my seed note"));
    assert!(!r.join(".claude/agents/explorer.md").exists());
    assert_eq!(read(r, "README.md"), USER_README);
    // `channel add` and `add --channel` each ran the post once; the conflicted --all skips it.
    let log = read(r, ".rig-post.log");
    assert_eq!(log.lines().count(), 2, "{log}");
}

#[test]
fn golden_forced_section_variant() {
    let (_tpl, repo, _ch) = golden_repo();
    let r = repo.path();
    edit(&repo, "CLAUDE.md", |t| {
        t.replace("See `AGENTS.md` for the rules.", "See `AGENTS.md`, mine.")
    });
    // A block whose fragment changed in v2, edited: section drift refuses without --force.
    edit(&repo, "AGENTS.md", |t| {
        t.replace("- Keep files small.", "- Keep files tiny.")
    });
    edit(&repo, ".claude/agents/reviewer.md", |t| {
        format!("{t}mine\n")
    });
    commit_all(r);
    let before = tree_snapshot(r);
    let refused = all_json(&repo, &[], 1);
    assert_eq!(refused["ok"], false);
    let msgs = refused["messages"].to_string();
    assert!(
        msgs.contains("`AGENTS.md#rig` is owned by channel rig but was edited locally"),
        "{msgs}"
    );
    assert_eq!(tree_snapshot(r), before, "drift writes nothing");

    let v = all_json(&repo, &["--force"], 1);
    insta::assert_json_snapshot!(v);
    insta::assert_snapshot!(redact(&tree_snapshot(r)));
    assert!(
        read(r, "CLAUDE.md").contains("mine."),
        "unchanged fragment: edit kept"
    );
    let agents = read(r, "AGENTS.md");
    assert!(agents.contains("- Keep files small and focused.") && !agents.contains("tiny"));
    assert!(
        read(r, ".claude/agents/reviewer.md").contains("v2")
            && !read(r, ".claude/agents/reviewer.md").contains("mine")
    );
}

#[test]
fn contract_guard_golden() {
    // The main lock written next to channels is the one golden_update snapshots.
    let (_tpl, repo, _ch) = golden_repo();
    let r = repo.path();
    all_json(&repo, &[], 1);
    let lock = redact(&read(r, ".splice/lock.toml"));
    assert!(
        !lock.contains("[channel]") && !lock.contains("[sections]"),
        "{lock}"
    );
    let reference = std::fs::read_to_string(
        "tests/snapshots/golden_update__golden_v1_to_v2_with_user_edits-2.snap",
    )
    .unwrap();
    let section = |text: &str| {
        let a = text
            .find("== .splice/lock.toml ==\n")
            .expect("lock section");
        let rest = &text[a..];
        let end = rest[3..].find("\n== ").map_or(rest.len(), |i| i + 4);
        rest[..end].trim_end().to_string()
    };
    assert_eq!(
        section(&format!("== .splice/lock.toml ==\n{lock}")),
        section(&reference)
    );
}

#[test]
fn all_conflict_summary_names_its_source() {
    // The ci.yml conflict is the main template's; the summary must not read as the channel's.
    let (_tpl, repo, _ch) = golden_repo();
    let v = all_json(&repo, &[], 1);
    let msgs: Vec<String> = v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().to_string())
        .collect();
    let summary: Vec<_> = msgs.iter().filter(|m| m.contains("conflicts in")).collect();
    assert!(
        summary
            .iter()
            .any(|m| m.starts_with("main: conflicts in .github/workflows/ci.yml")),
        "{msgs:?}"
    );
    assert!(summary.iter().all(|m| m.starts_with("main: ")), "{msgs:?}");
}
