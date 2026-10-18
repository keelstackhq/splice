//! `splice update --channel` (channels spec §8; main spec §8.3 on a channel lock).

mod common;

use common::*;
use splice::lock::ChannelLock;
use std::path::Path;
use tempfile::TempDir;

fn src(ch: &TempDir) -> String {
    format!("{}//channel", ch.path().display())
}

fn stdout_json(a: assert_cmd::assert::Assert) -> serde_json::Value {
    let out = a.get_output().stdout.clone();
    serde_json::from_slice(&out)
        .unwrap_or_else(|e| panic!("stdout is not one JSON object: {e}: {out:?}"))
}

fn read(repo: &Path, rel: &str) -> String {
    std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn bytes(repo: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(repo.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn rig(repo: &Path) -> ChannelLock {
    ChannelLock::read(repo, "rig")
        .unwrap()
        .expect("channel lock")
}

fn status(repo: &Path) -> String {
    git_stdout(repo, &["status", "--porcelain", "--untracked-files=all"])
}

const RIG_TOML: &str = ".splice/channels/rig.toml";
const MAIN_LOCK: &str = ".splice/lock.toml";

fn main_repo() -> (TempDir, TempDir) {
    let tpl = template_repo(&["v1"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    (tpl, repo)
}

/// `channel add` of `ch` at v1.0.0 with claude, skills:claude and hooks, committed.
fn add_full(repo: &Path, ch: &TempDir) {
    chan_commit(
        repo,
        &[
            "channel",
            "add",
            &src(ch),
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
}

/// Main repo at v1 plus channel rig (v1, full pieces), channel source with v1 and v2.
fn setup() -> (TempDir, TempDir, TempDir) {
    let (tpl, repo) = main_repo();
    let ch = channel_repo(&["v1", "v2"]);
    add_full(repo.path(), &ch);
    (tpl, repo, ch)
}

fn update_rig(repo: &Path, extra: &[&str]) -> assert_cmd::assert::Assert {
    let mut args = vec!["update", "--channel", "rig"];
    args.extend(extra);
    chan(repo, &args)
}

#[test]
fn update_channel_v1_to_v2() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    let main_before = bytes(r, MAIN_LOCK);
    let notes_before = bytes(r, "docs/agents/notes.md");
    let v = stdout_json(update_rig(r, &[]).success());
    assert_eq!(v["command"], "update");
    assert_eq!(v["ok"], true);
    assert_eq!(
        read(r, ".claude/agents/reviewer.md"),
        "# reviewer\n\nReview read-only, v2.\n"
    );
    assert!(!r.join(".claude/agents/explorer.md").exists());
    assert!(read(r, ".claude/skills/verify/SKILL.md").contains("Run the checks, v2."));
    let agents = read(r, "AGENTS.md");
    assert!(
        agents.contains("- Keep files small and focused."),
        "{agents}"
    );
    let paths: Vec<String> = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap().to_string())
        .collect();
    assert!(paths.contains(&"AGENTS.md#rig".to_string()), "{paths:?}");
    assert_eq!(bytes(r, "docs/agents/notes.md"), notes_before);
    let cl = rig(r);
    assert_eq!(cl.lock.template.version.to_string(), "2.0.0");
    assert!(!cl.lock.files.contains_key(".claude/agents/explorer.md"));
    assert_eq!(bytes(r, MAIN_LOCK), main_before);
    let log = read(r, ".rig-post.log");
    assert_eq!(log.lines().count(), 2, "{log}");
}

#[test]
fn section_drift_refused_then_forced() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    let agents = read(r, "AGENTS.md");
    let edited = format!(
        "# Outside\n\n{}",
        agents.replace("- Test first.", "- Test first, always.")
    );
    std::fs::write(r.join("AGENTS.md"), &edited).unwrap();
    commit_all(r);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_rig(r, &[]).code(1));
    let msg = v["messages"].to_string();
    assert!(msg.contains("AGENTS.md#rig"), "{msg}");
    assert!(msg.contains("--force"), "{msg}");
    assert_eq!(status(r), "");
    assert_eq!(bytes(r, RIG_TOML), rig_before);

    update_rig(r, &["--force"]).success();
    let after = read(r, "AGENTS.md");
    assert!(after.starts_with("# Outside\n\n"), "{after}");
    assert!(!after.contains("Test first, always."), "{after}");
    assert!(after.contains("- Keep files small and focused."), "{after}");
    assert_eq!(rig(r).lock.template.version.to_string(), "2.0.0");
}

#[test]
fn crlf_host_stays_crlf() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    let crlf = read(r, "AGENTS.md").replace('\n', "\r\n");
    std::fs::write(r.join("AGENTS.md"), crlf).unwrap();
    commit_all(r);
    update_rig(r, &[]).success();
    let after = read(r, "AGENTS.md");
    assert!(
        after.contains("- Keep files small and focused.\r\n"),
        "{after:?}"
    );
    for line in after.split_inclusive('\n') {
        assert!(line.ends_with("\r\n"), "{line:?} in {after:?}");
    }
}

#[test]
fn deleted_markers_are_drift() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    std::fs::write(r.join("AGENTS.md"), "# Mine\n").unwrap();
    commit_all(r);
    let v = stdout_json(update_rig(r, &[]).code(1));
    assert!(v["messages"].to_string().contains("AGENTS.md#rig"), "{v}");
    assert_eq!(status(r), "");
    update_rig(r, &["--force"]).success();
    let after = read(r, "AGENTS.md");
    assert!(after.starts_with("# Mine\n"), "{after}");
    assert!(after.contains("<!-- splice:rig:begin"), "{after}");
    assert!(after.contains("- Keep files small and focused."), "{after}");
}

#[test]
fn broken_markers_exit_1_even_forced() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    let agents = read(r, "AGENTS.md");
    let begin = agents.lines().next().unwrap().to_string();
    std::fs::write(r.join("AGENTS.md"), format!("{begin}\n{agents}")).unwrap();
    commit_all(r);
    let rig_before = bytes(r, RIG_TOML);
    for extra in [&[][..], &["--force"][..]] {
        let v = stdout_json(update_rig(r, extra).code(1));
        assert!(
            v["messages"].to_string().contains("lines"),
            "{extra:?}: {v}"
        );
        assert_eq!(status(r), "", "{extra:?}");
        assert_eq!(bytes(r, RIG_TOML), rig_before);
    }
}

const CAVEMAN: &str = ".claude/skills/caveman/SKILL.md";
const CAVEMAN_GLOB: &str = ".claude/skills/caveman/**";

#[test]
fn filter_change_on_update() {
    // At v1 only: the filter change alone drives the update.
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    add_full(r, &ch);
    update_rig(r, &["--exclude", CAVEMAN_GLOB]).success();
    assert!(!r.join(CAVEMAN).exists());
    let cl = rig(r);
    assert_eq!(cl.meta.exclude, [CAVEMAN_GLOB]);
    assert!(cl.meta.only.is_empty());
    assert!(!cl.lock.files.contains_key(CAVEMAN));
    commit_all(r);

    update_rig(r, &["--only", "**"]).success();
    let cl = rig(r);
    assert_eq!(cl.meta.only, ["**"]);
    assert!(cl.meta.exclude.is_empty());
    assert!(r.join(CAVEMAN).is_file());
    assert!(cl.lock.files.contains_key(CAVEMAN));
}

#[test]
fn filter_change_keeps_an_edited_file() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    add_full(r, &ch);
    std::fs::write(r.join(CAVEMAN), "# mine\n").unwrap();
    commit_all(r);
    let v = stdout_json(update_rig(r, &["--exclude", CAVEMAN_GLOB]).success());
    assert_eq!(read(r, CAVEMAN), "# mine\n");
    assert!(v["messages"].to_string().contains(CAVEMAN), "{v}");
    assert!(!rig(r).lock.files.contains_key(CAVEMAN));
}

#[test]
fn owned_drift_in_channel_refused() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    std::fs::write(r.join(".claude/agents/reviewer.md"), "# mine\n").unwrap();
    commit_all(r);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_rig(r, &[]).code(1));
    assert!(
        v["messages"]
            .to_string()
            .contains(".claude/agents/reviewer.md"),
        "{v}"
    );
    assert_eq!(status(r), "");
    assert_eq!(bytes(r, RIG_TOML), rig_before);
}

#[test]
fn dirty_tree_refused() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    std::fs::write(r.join("scratch.txt"), "x\n").unwrap();
    let before = status(r);
    let v = stdout_json(update_rig(r, &[]).code(2));
    assert!(v["messages"].to_string().contains("--allow-dirty"), "{v}");
    assert_eq!(status(r), before);
    assert_eq!(rig(r).lock.template.version.to_string(), "1.0.0");
}

#[test]
fn up_to_date_channel() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    add_full(r, &ch);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_rig(r, &[]).success());
    assert!(v["messages"].to_string().contains("up to date"), "{v}");
    assert_eq!(bytes(r, RIG_TOML), rig_before);
    assert_eq!(status(r), "");
}

#[test]
fn plain_update_untouched_by_channels() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let r = repo.path();
    add_full(r, &ch);
    let rig_before = bytes(r, RIG_TOML);
    chan(r, &["update"]).success();
    assert_eq!(
        splice::lock::Lock::read(r)
            .unwrap()
            .unwrap()
            .template
            .version
            .to_string(),
        "2.0.0"
    );
    assert_eq!(bytes(r, RIG_TOML), rig_before);
    assert!(read(r, ".claude/agents/reviewer.md").contains("Review read-only.\n"));
}

#[test]
fn channel_only_repo_plain_update() {
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    let v = stdout_json(chan(r, &["update"]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("no .splice/lock.toml; use --channel NAME or --all"),
        "{v}"
    );
}

#[test]
fn claim_conflict_on_update() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo_tagged("channel", &[("v1", "v1.0.0"), ("v2-collide", "v2.0.0")]);
    let r = repo.path();
    add_full(r, &ch);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_rig(r, &[]).code(2));
    assert!(
        v["messages"].to_string().contains(
            "path .editorconfig is owned by main; exclude it with --exclude .editorconfig"
        ),
        "{v}"
    );
    assert_eq!(status(r), "");
    assert_eq!(bytes(r, RIG_TOML), rig_before);
}

#[test]
fn main_update_claim_conflict_with_channel() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2-x", "v2.0.0")]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo_tagged("channel", &[("v2-x", "v1.0.0")]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch)]);
    assert!(rig(r).lock.files.contains_key("x.txt"));
    let main_before = bytes(r, MAIN_LOCK);
    let x_before = bytes(r, "x.txt");
    let v = stdout_json(chan(r, &["update", "--to", "2.0.0"]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("path x.txt is owned by channel rig"),
        "{v}"
    );
    assert_eq!(status(r), "");
    assert_eq!(bytes(r, MAIN_LOCK), main_before);
    assert_eq!(bytes(r, "x.txt"), x_before);
}

#[test]
fn unknown_channel() {
    let (_tpl, repo, _ch) = setup();
    let v = stdout_json(chan(repo.path(), &["update", "--channel", "zz"]).code(2));
    assert!(
        v["messages"].to_string().contains("unknown channel `zz`"),
        "{v}"
    );
}

#[test]
fn channel_to_and_data() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    let v = stdout_json(update_rig(r, &["--data", "dir=agents"]).code(2));
    assert!(
        v["messages"]
            .to_string()
            .contains("--data does not apply to `update --channel`"),
        "{v}"
    );
    assert_eq!(status(r), "");
    update_rig(r, &["--to", "2.0.0"]).success();
    assert_eq!(rig(r).lock.template.version.to_string(), "2.0.0");
}

#[test]
fn channel_downgrade_names_the_channel_lock() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    chan_commit(r, &["update", "--channel", "rig"]);
    let v = stdout_json(update_rig(r, &["--to", "1.0.0"]).code(2));
    let msg = v["messages"].to_string();
    assert!(msg.contains("splice does not downgrade"), "{msg}");
    assert!(msg.contains(RIG_TOML), "{msg}");
}

#[test]
fn failed_post_on_update_keeps_old_channel_lock() {
    let t = inline_template(&[(
        "core",
        "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\"a.txt\"]\n[[post]]\nrun = \"exit 7\"\nwhen = [\"update\"]\n",
        &[("a.txt", "a\n")],
    )]);
    git(
        t.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(t.path(), "v1", "v1.0.0");
    let repo = bare_repo();
    let r = repo.path();
    chan_commit(r, &["channel", "add", t.path().to_str().unwrap()]);
    std::fs::write(t.path().join("pieces/core/files/a.txt"), "a2\n").unwrap();
    commit_tag(t.path(), "v2", "v2.0.0");
    let before = bytes(r, ".splice/channels/t.toml");
    let v = stdout_json(chan(r, &["update", "--channel", "t"]).code(3));
    let msg = v["messages"].to_string();
    assert!(
        msg.contains("the lock still records 1.0.0 (.splice/channels/t.toml)"),
        "{msg}"
    );
    assert_eq!(read(r, "a.txt"), "a2\n");
    assert_eq!(bytes(r, ".splice/channels/t.toml"), before);
}

/// Bare repo with channel rig tracking `main` of a fresh channel source (v1), committed.
fn tracked() -> (TempDir, TempDir) {
    let repo = bare_repo();
    let ch = channel_repo(&["v1"]);
    chan_commit(
        repo.path(),
        &["channel", "add", &src(&ch), "--track", "main"],
    );
    (repo, ch)
}

#[test]
fn tracked_channel_moves_to_head() {
    let (repo, ch) = tracked();
    let r = repo.path();
    let head = track_commit(ch.path(), true);
    update_rig(r, &[]).success();
    let cl = rig(r);
    assert_eq!(cl.meta.track.as_deref(), Some("main"));
    assert_eq!(cl.lock.template.commit, head);
    assert_eq!(
        cl.lock.template.version.to_string(),
        format!("0.0.0+g{}", &head[..7])
    );
    assert!(read(r, "docs/agents/rules.md").contains("change "));
}

#[test]
fn tracked_outside_change_is_up_to_date() {
    let (repo, ch) = tracked();
    let r = repo.path();
    let before = bytes(r, RIG_TOML);
    track_commit(ch.path(), false);
    let v = stdout_json(update_rig(r, &[]).success());
    assert!(v["messages"].to_string().contains("up to date"), "{v}");
    assert_eq!(bytes(r, RIG_TOML), before);
    assert_eq!(status(r), "");
}

#[test]
fn tracked_rejects_to() {
    let (repo, _ch) = tracked();
    let v = stdout_json(update_rig(repo.path(), &["--to", "2.0.0"]).code(2));
    let msg = v["messages"].to_string();
    assert!(msg.contains("channel rig tracks main"), "{msg}");
    assert!(msg.contains("re-add it with --ref"), "{msg}");
}

#[test]
fn tracked_head_sorting_lower_still_updates() {
    let (repo, ch) = tracked();
    let r = repo.path();
    let locked = rig(r).lock.template.commit;
    let mut head = track_commit(ch.path(), true);
    for _ in 0..200 {
        if head[..7] < locked[..7] {
            break;
        }
        head = track_commit(ch.path(), true);
    }
    assert!(head[..7] < locked[..7], "no lower sha7 found");
    let v = stdout_json(update_rig(r, &[]).success());
    assert!(!v["messages"].to_string().contains("downgrade"), "{v}");
    assert_eq!(rig(r).lock.template.commit, head);
}

#[test]
fn tracked_runs_no_migrations() {
    // A migration whose version sorts between the locked and the new `0.0.0+g…` (semver orders
    // build metadata as text) would be selected by a version compare; tracked heads skip it.
    let (repo, ch) = tracked();
    let r = repo.path();
    let locked = rig(r).lock.template.commit;
    let mig = format!("0.0.0+g{}z", &locked[..7]);
    let dir = ch.path().join("channel/migrations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join(format!("{mig}.toml")),
        "[[step]]\nrun = \"touch ran\"\n",
    )
    .unwrap();
    let mut head = track_commit(ch.path(), true);
    for _ in 0..200 {
        if head[..7] > locked[..7] {
            break;
        }
        head = track_commit(ch.path(), true);
    }
    let v = |s: &str| semver::Version::parse(s).unwrap();
    let (from, to) = (
        v(&format!("0.0.0+g{}", &locked[..7])),
        v(&format!("0.0.0+g{}", &head[..7])),
    );
    assert!(from < v(&mig) && v(&mig) <= to, "{from} {mig} {to}");
    update_rig(r, &[]).success();
    assert!(!r.join("ran").exists());
    assert_eq!(rig(r).lock.template.commit, head);
}

#[test]
fn tagged_channel_same_subtree_is_up_to_date() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    add_full(r, &ch);
    std::fs::write(ch.path().join("README.md"), "# changed outside\n").unwrap();
    commit_tag(ch.path(), "readme only", "v3.0.0");
    let before = bytes(r, RIG_TOML);
    let v = stdout_json(update_rig(r, &[]).success());
    assert!(v["messages"].to_string().contains("up to date"), "{v}");
    assert_eq!(bytes(r, RIG_TOML), before);
    assert_eq!(status(r), "");
}
