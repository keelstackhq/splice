//! `splice channel add` (channels spec §3, §5, §6).

mod common;

use common::*;
use splice::lock::ChannelLock;
use std::path::Path;
use tempfile::TempDir;

fn src(ch: &TempDir) -> String {
    format!("{}//channel", ch.path().display())
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out)
        .unwrap_or_else(|e| panic!("stdout is not one JSON object: {e}: {out:?}"))
}

fn stdout_json(a: assert_cmd::assert::Assert) -> serde_json::Value {
    json(&a.get_output().stdout.clone())
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn rig(repo: &Path, name: &str) -> ChannelLock {
    ChannelLock::read(repo, name)
        .unwrap()
        .expect("channel lock")
}

fn status(repo: &Path) -> String {
    git_stdout(repo, &["status", "--porcelain", "--untracked-files=all"])
}

fn main_repo() -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    (tpl, repo)
}

const FULL: [&str; 7] = [
    "--ref",
    "v1.0.0",
    "--piece",
    "claude",
    "--piece",
    "skills:claude",
    "--data",
];

fn full_args(s: &str) -> Vec<&str> {
    let mut v = vec!["channel", "add", s];
    v.extend(FULL);
    v.push("dir=claude");
    v
}

#[test]
fn add_into_new_rendered_repo() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let lock_before = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();
    let v = stdout_json(chan(repo.path(), &full_args(&s)).success());
    assert_eq!(v["command"], "channel add");
    assert_eq!(v["ok"], true);
    let paths: Vec<String> = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap().to_string())
        .collect();
    assert!(paths.contains(&"AGENTS.md#rig".to_string()), "{paths:?}");
    assert!(paths.contains(&"CLAUDE.md#rig".to_string()), "{paths:?}");
    let r = repo.path();
    for f in [
        ".claude/agents/explorer.md",
        ".claude/agents/reviewer.md",
        ".claude/skills/caveman/SKILL.md",
        ".claude/skills/verify/SKILL.md",
        "docs/agents/rules.md",
        "docs/agents/notes.md",
    ] {
        assert!(r.join(f).is_file(), "{f}");
    }
    let agents = read(r, "AGENTS.md");
    assert!(
        agents.starts_with("<!-- splice:rig:begin — managed by `splice update --channel rig`"),
        "{agents}"
    );
    assert!(
        agents.contains("- Subagents live in `.claude/agents`."),
        "{agents}"
    );
    assert!(agents.contains("<!-- splice:rig:end -->"), "{agents}");
    assert!(read(r, "CLAUDE.md").contains("See `AGENTS.md` for the rules."));

    let cl = rig(r, "rig");
    assert_eq!(cl.meta.name, "rig");
    assert_eq!(cl.meta.track, None);
    assert_eq!(cl.lock.template.version.to_string(), "1.0.0");
    assert!(cl.lock.files.contains_key("docs/agents/rules.md"));
    assert!(!cl.lock.files.contains_key("docs/agents/notes.md")); // seed
    assert_eq!(
        cl.sections.keys().collect::<Vec<_>>(),
        ["AGENTS.md", "CLAUDE.md"]
    );
    assert!(cl.sections.values().all(|s| s.created));
    assert_eq!(
        std::fs::read(r.join(".splice/lock.toml")).unwrap(),
        lock_before
    );
}

#[test]
fn add_into_bare_repo() {
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s]).success());
    assert_eq!(v["ok"], true);
    assert!(!repo.path().join(".splice/lock.toml").exists());
    let cl = rig(repo.path(), "rig");
    assert_eq!(cl.lock.pieces.len(), 1);
    assert_eq!(cl.lock.pieces[0].id, "core");
    assert!(read(repo.path(), "AGENTS.md").contains("- Keep files small."));
    assert!(!read(repo.path(), "AGENTS.md").contains("Subagents"));
}

#[test]
fn name_defaults_and_as() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s, "--as", "Other"]).code(2));
    assert!(v["messages"].to_string().contains("--as NAME"), "{v}");
    assert_eq!(status(repo.path()), "");

    chan(repo.path(), &["channel", "add", &s]).success();
    assert!(repo.path().join(".splice/channels/rig.toml").is_file());

    let fresh = bare_repo();
    chan(fresh.path(), &["channel", "add", &s, "--as", "rig2"]).success();
    assert!(!fresh.path().join(".splice/channels/rig.toml").exists());
    assert_eq!(rig(fresh.path(), "rig2").meta.name, "rig2");
    let agents = read(fresh.path(), "AGENTS.md");
    assert!(agents.contains("<!-- splice:rig2:begin"), "{agents}");
    assert!(agents.contains("<!-- splice:rig2:end -->"), "{agents}");
}

#[test]
fn name_taken() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    chan_commit(repo.path(), &["channel", "add", &s]);
    let before = read(repo.path(), ".splice/channels/rig.toml");
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("use splice update --channel rig"),
        "{v}"
    );
    assert_eq!(read(repo.path(), ".splice/channels/rig.toml"), before);
    assert_eq!(status(repo.path()), "");
}

#[test]
fn dirty_tree_refused() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    std::fs::write(repo.path().join("scratch.txt"), "x\n").unwrap();
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s]).code(2));
    assert!(v["messages"].to_string().contains("--allow-dirty"), "{v}");
    assert!(!repo.path().join(".splice/channels").exists());
    chan(repo.path(), &["channel", "add", &s, "--allow-dirty"]).success();
    assert!(repo.path().join(".splice/channels/rig.toml").is_file());
}

#[test]
fn claim_conflict_writes_nothing() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s, "--piece", "collide"]).code(2));
    assert!(
        v["messages"].to_string().contains(
            "path .editorconfig is owned by main; exclude it with --exclude .editorconfig"
        ),
        "{v}"
    );
    assert_eq!(status(repo.path()), "");

    let editorconfig = read(repo.path(), ".editorconfig");
    chan(
        repo.path(),
        &[
            "channel",
            "add",
            &s,
            "--piece",
            "collide",
            "--exclude",
            ".editorconfig",
        ],
    )
    .success();
    assert_eq!(read(repo.path(), ".editorconfig"), editorconfig);
    let cl = rig(repo.path(), "rig");
    assert!(!cl.lock.files.contains_key(".editorconfig"));
    assert_eq!(cl.meta.exclude, [".editorconfig"]);
}

#[test]
fn host_owned_by_main_refused() {
    let tpl = template_repo_tagged(&[("v1-claude", "v1.0.0")]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s, "--piece", "claude"]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("path CLAUDE.md is owned by main"),
        "{v}"
    );
    assert_eq!(status(repo.path()), "");
}

#[test]
fn filters_stored_and_applied() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    let glob = ".claude/skills/caveman/**";
    chan(
        repo.path(),
        &[
            "channel",
            "add",
            &s,
            "--piece",
            "skills:claude",
            "--data",
            "dir=claude",
            "--exclude",
            glob,
        ],
    )
    .success();
    let r = repo.path();
    assert!(!r.join(".claude/skills/caveman").exists());
    assert!(r.join(".claude/skills/verify/SKILL.md").is_file());
    let cl = rig(r, "rig");
    assert!(
        !cl.lock
            .files
            .contains_key(".claude/skills/caveman/SKILL.md")
    );
    assert_eq!(cl.meta.exclude, [glob]);
    assert!(cl.meta.only.is_empty());

    let other = bare_repo();
    let v = stdout_json(chan(other.path(), &["channel", "add", &s, "--only", "nope/**"]).success());
    assert!(
        v["messages"]
            .to_string()
            .contains("filter `nope/**` matches nothing"),
        "{v}"
    );
    assert_eq!(rig(other.path(), "rig").meta.only, ["nope/**"]);
}

#[test]
fn identical_owned_file_adopts() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    let rules = ch
        .path()
        .join("channel/pieces/core/files/docs/agents/rules.md");
    std::fs::create_dir_all(repo.path().join("docs/agents")).unwrap();
    std::fs::copy(rules, repo.path().join("docs/agents/rules.md")).unwrap();
    commit_all(repo.path());
    chan(repo.path(), &["channel", "add", &s]).success();
    assert!(
        rig(repo.path(), "rig")
            .lock
            .files
            .contains_key("docs/agents/rules.md")
    );
}

#[test]
fn differing_owned_file_is_drift() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    std::fs::create_dir_all(repo.path().join("docs/agents")).unwrap();
    std::fs::write(repo.path().join("docs/agents/rules.md"), "# Mine\n").unwrap();
    commit_all(repo.path());
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s]).code(1));
    assert_eq!(v["ok"], false);
    assert!(
        v["messages"].to_string().contains("docs/agents/rules.md"),
        "{v}"
    );
    assert_eq!(status(repo.path()), "");
    assert_eq!(read(repo.path(), "docs/agents/rules.md"), "# Mine\n");

    chan(repo.path(), &["channel", "add", &s, "--force"]).success();
    assert_eq!(
        read(repo.path(), "docs/agents/rules.md"),
        "# Rules\n\nTest first.\n"
    );
}

#[test]
fn host_without_markers_gets_block_appended() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    std::fs::write(repo.path().join("AGENTS.md"), "# Mine\n").unwrap();
    commit_all(repo.path());
    chan(repo.path(), &["channel", "add", &s]).success();
    let agents = read(repo.path(), "AGENTS.md");
    assert!(
        agents.starts_with("# Mine\n\n<!-- splice:rig:begin"),
        "{agents}"
    );
    assert!(!rig(repo.path(), "rig").sections["AGENTS.md"].created);
}

#[test]
fn track_pins_head() {
    let ch = channel_repo(&["v1"]);
    let head = track_commit(ch.path(), true);
    let s = src(&ch);
    let repo = bare_repo();
    chan(repo.path(), &["channel", "add", &s, "--track", "main"]).success();
    let cl = rig(repo.path(), "rig");
    assert_eq!(cl.meta.track.as_deref(), Some("main"));
    assert_eq!(
        cl.lock.template.version.to_string(),
        format!("0.0.0+g{}", &head[..7])
    );
    assert_eq!(cl.lock.template.commit, head);
    assert!(read(repo.path(), "docs/agents/rules.md").contains("change "));
}

#[test]
fn track_unknown_branch() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    let v = stdout_json(chan(repo.path(), &["channel", "add", &s, "--track", "nope"]).code(3));
    assert!(v["messages"].to_string().contains("nope"), "{v}");
    assert_eq!(status(repo.path()), "");
}

#[test]
fn post_runs_for_add() {
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let repo = bare_repo();
    let out = chan(repo.path(), &["channel", "add", &s, "--piece", "hooks"]).success();
    json(&out.get_output().stdout.clone());
    assert_eq!(read(repo.path(), ".rig-post.log"), "rig-post\n");

    let other = bare_repo();
    chan(
        other.path(),
        &["channel", "add", &s, "--piece", "hooks", "--no-post"],
    )
    .success();
    assert!(!other.path().join(".rig-post.log").exists());
    assert!(other.path().join(".splice/channels/rig.toml").is_file());
}

#[test]
fn second_channel_disjoint_paths() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let chb = channel_repo_tagged("channel-b", &[("v1", "v1.0.0")]);
    let lock_before = std::fs::read(repo.path().join(".splice/lock.toml")).unwrap();
    chan_commit(repo.path(), &["channel", "add", &src(&ch)]);
    chan(repo.path(), &["channel", "add", &src(&chb)]).success();
    assert_eq!(rig(repo.path(), "rig").meta.name, "rig");
    let b = rig(repo.path(), "rigb");
    assert!(b.lock.files.contains_key("docs/agents/rigb.md"));
    assert_eq!(
        std::fs::read(repo.path().join(".splice/lock.toml")).unwrap(),
        lock_before
    );
}

/// An inline channel template (name `t`) committed and tagged `v1.0.0` in its own repo.
fn inline_channel(piece_toml: &str, files: FileSpec) -> TempDir {
    let t = inline_template(&[("core", piece_toml, files)]);
    git(
        t.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(t.path(), "v1", "v1.0.0");
    t
}

#[test]
fn shared_conflict_writes_markers_and_lock() {
    // G11: like `update`: markers written, lock written (records the target), post skipped, exit 1.
    let t = inline_channel(
        "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nshared = [\"notes.txt\"]\n[merge]\n\"notes.txt\" = \"text\"\n[[post]]\nrun = \"echo p > .post\"\nwhen = [\"add\"]\n",
        &[("notes.txt", "theirs\n")],
    );
    let repo = bare_repo();
    std::fs::write(repo.path().join("notes.txt"), "mine\n").unwrap();
    commit_all(repo.path());
    let v = stdout_json(chan(repo.path(), &["channel", "add", t.path().to_str().unwrap()]).code(1));
    assert_eq!(v["command"], "channel add");
    let notes = read(repo.path(), "notes.txt");
    assert!(
        notes.contains("<<<<<<<") && notes.contains("mine") && notes.contains("theirs"),
        "{notes}"
    );
    assert!(rig(repo.path(), "t").lock.files.contains_key("notes.txt"));
    assert!(!repo.path().join(".post").exists());
}

#[test]
fn failed_post_writes_no_channel_lock() {
    let t = inline_channel(
        "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\"a.txt\"]\n[[post]]\nrun = \"exit 7\"\nwhen = [\"add\"]\n",
        &[("a.txt", "a\n")],
    );
    let repo = bare_repo();
    let v = stdout_json(chan(repo.path(), &["channel", "add", t.path().to_str().unwrap()]).code(3));
    let msg = v["messages"].to_string();
    assert!(
        msg.contains("no .splice/channels/t.toml was written"),
        "{msg}"
    );
    assert!(!msg.contains("still records"), "{msg}");
    assert!(repo.path().join("a.txt").is_file());
    assert!(!repo.path().join(".splice/channels/t.toml").exists());
}

fn changes(v: &serde_json::Value) -> Vec<(String, String)> {
    v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["path"].as_str().unwrap().to_string(),
                c["action"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn second_harness_instance() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let s = src(&ch);
    let r = repo.path();
    let main_lock = std::fs::read(r.join(".splice/lock.toml")).unwrap();
    chan_commit(r, &full_args(&s));
    let v = stdout_json(
        chan(
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
        )
        .success(),
    );
    assert_eq!(v["command"], "add");
    for f in ["caveman", "verify"] {
        assert_eq!(
            read(r, &format!(".agents/skills/{f}/SKILL.md")),
            read(r, &format!(".claude/skills/{f}/SKILL.md"))
        );
    }
    let cl = rig(r, "rig");
    let skills: Vec<_> = cl
        .lock
        .pieces
        .iter()
        .filter(|p| p.id == "skills")
        .map(|p| (p.name.as_str(), p.vars["dir"].as_str()))
        .collect();
    assert_eq!(skills, [("claude", "claude"), ("agents", "agents")]);
    assert!(cl.lock.files.contains_key(".agents/skills/verify/SKILL.md"));
    assert_eq!(cl.sections.len(), 2);
    assert_eq!(
        std::fs::read(r.join(".splice/lock.toml")).unwrap(),
        main_lock
    );
}

#[test]
fn add_channel_unknown() {
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    chan_commit(repo.path(), &["channel", "add", &src(&ch)]);
    let v = stdout_json(chan(repo.path(), &["add", "claude", "--channel", "zz"]).code(2));
    let msg = v["messages"].to_string();
    assert!(
        msg.contains("unknown channel `zz`") && msg.contains("rig"),
        "{msg}"
    );
}

#[test]
fn add_channel_respects_filter() {
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(
        r,
        &[
            "channel",
            "add",
            &src(&ch),
            "--piece",
            "skills:claude",
            "--data",
            "dir=claude",
            "--exclude",
            ".agents/skills/caveman/**",
        ],
    );
    chan(
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
    )
    .success();
    assert!(!r.join(".agents/skills/caveman").exists());
    assert!(r.join(".agents/skills/verify/SKILL.md").is_file());
    assert!(r.join(".claude/skills/caveman/SKILL.md").is_file());
    let cl = rig(r, "rig");
    assert!(
        !cl.lock
            .files
            .contains_key(".agents/skills/caveman/SKILL.md")
    );
    assert_eq!(cl.meta.exclude, [".agents/skills/caveman/**"]);
}

#[test]
fn add_channel_claim_conflict() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    let before = read(r, ".splice/channels/rig.toml");
    let v = stdout_json(chan(r, &["add", "collide", "--channel", "rig"]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("path .editorconfig is owned by main"),
        "{v}"
    );
    assert_eq!(read(r, ".splice/channels/rig.toml"), before);
    assert_eq!(status(r), "");
}

#[test]
fn add_channel_updates_section_with_new_piece() {
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    assert!(!read(r, "AGENTS.md").contains("Subagents"));
    let v = stdout_json(chan(r, &["add", "claude", "--channel", "rig"]).success());
    let c = changes(&v);
    assert!(
        c.contains(&("AGENTS.md#rig".into(), "update".into())),
        "{c:?}"
    );
    assert!(
        c.contains(&("CLAUDE.md#rig".into(), "create".into())),
        "{c:?}"
    );
    assert!(read(r, "AGENTS.md").contains("- Subagents live in `.claude/agents`."));
    assert!(r.join(".claude/agents/explorer.md").is_file());
    assert_eq!(rig(r, "rig").sections.len(), 2);
}

#[test]
fn add_channel_drift_writes_nothing() {
    // G11: `add --channel` follows `add`: drift or conflict → nothing written, exit 1.
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    std::fs::create_dir_all(r.join(".claude/agents")).unwrap();
    std::fs::write(r.join(".claude/agents/explorer.md"), "# mine\n").unwrap();
    commit_all(r);
    let before = read(r, ".splice/channels/rig.toml");
    let v = stdout_json(chan(r, &["add", "claude", "--channel", "rig"]).code(1));
    assert_eq!(v["ok"], false);
    assert_eq!(read(r, ".splice/channels/rig.toml"), before);
    assert_eq!(status(r), "");
    assert!(!r.join("CLAUDE.md").exists());
}

#[test]
fn seed_at_main_owned_path_is_dropped() {
    // G10: a channel seed at a path the main lock owns is never written nor locked.
    let t = inline_channel(
        "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nseed = [\".editorconfig\"]\n",
        &[(".editorconfig", "root = false\n")],
    );
    let (_tpl, repo) = main_repo();
    let r = repo.path();
    let before = std::fs::read(r.join(".editorconfig")).unwrap();
    let v = stdout_json(chan(r, &["channel", "add", t.path().to_str().unwrap()]).success());
    assert!(
        !changes(&v).iter().any(|(p, _)| p == ".editorconfig"),
        "{v}"
    );
    assert_eq!(std::fs::read(r.join(".editorconfig")).unwrap(), before);
    assert!(!rig(r, "t").lock.files.contains_key(".editorconfig"));
}

#[test]
fn claim_conflict_with_existing_channel() {
    let t = inline_channel(
        "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\"docs/agents/rules.md\"]\n",
        &[("docs/agents/rules.md", "# other\n")],
    );
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    let v = stdout_json(chan(r, &["channel", "add", t.path().to_str().unwrap()]).code(2));
    assert!(
        v["messages"].to_string().contains(
            "path docs/agents/rules.md is owned by channel rig; exclude it with --exclude docs/agents/rules.md"
        ),
        "{v}"
    );
    assert!(!r.join(".splice/channels/t.toml").exists());
    assert_eq!(status(r), "");
}

#[test]
fn add_channel_names_the_other_holder() {
    // rig sorts before t, yet t already holds the path: the message names channel t.
    let t = inline_channel(
        "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\".claude/agents/explorer.md\"]\n",
        &[(".claude/agents/explorer.md", "# t\n")],
    );
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    chan_commit(r, &["channel", "add", t.path().to_str().unwrap()]);
    let v = stdout_json(chan(r, &["add", "claude", "--channel", "rig"]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("path .claude/agents/explorer.md is owned by channel t"),
        "{v}"
    );
    assert_eq!(status(r), "");
}
