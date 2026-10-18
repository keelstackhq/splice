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

// ---- update --all (channels spec §8, main spec §8.3) ----

fn update_all(repo: &Path) -> assert_cmd::assert::Assert {
    chan(repo, &["update", "--all"])
}

fn update_all_env(repo: &Path, key: &str, val: &str) -> assert_cmd::assert::Assert {
    splice_cmd()
        .arg("-C")
        .arg(repo)
        .args(["--yes", "--json", "update", "--all"])
        .env(key, val)
        .assert()
}

fn main_version(repo: &Path) -> String {
    splice::lock::Lock::read(repo)
        .unwrap()
        .expect("main lock")
        .template
        .version
        .to_string()
}

fn chan_version(repo: &Path, name: &str) -> String {
    ChannelLock::read(repo, name)
        .unwrap()
        .expect("channel lock")
        .lock
        .template
        .version
        .to_string()
}

const RIGB_TOML: &str = ".splice/channels/rigb.toml";

fn chb_v1_v2() -> TempDir {
    channel_repo_tagged("channel-b", &[("v1", "v1.0.0"), ("v2", "v2.0.0")])
}

/// Main repo at v1 (template `main_versions`), rig (full pieces) and rigb, both at v1 with v2
/// available, committed.
fn setup_two(main_versions: &[&str]) -> (TempDir, TempDir, TempDir, TempDir) {
    let tpl = template_repo(main_versions);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let chb = chb_v1_v2();
    add_full(repo.path(), &ch);
    chan_commit(
        repo.path(),
        &["channel", "add", &src(&chb), "--ref", "v1.0.0"],
    );
    (tpl, repo, ch, chb)
}

fn lines_eq(text: &str, line: &str) -> usize {
    text.lines().filter(|l| *l == line).count()
}

#[test]
fn all_updates_main_then_channels() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let r = repo.path();
    add_full(r, &ch);
    let log_before = read(r, ".rig-post.log").lines().count();
    let v = stdout_json(update_all(r).success());
    assert_eq!(v["command"], "update");
    assert_eq!(v["ok"], true);
    assert_eq!(main_version(r), "2.0.0");
    assert_eq!(chan_version(r, "rig"), "2.0.0");
    assert_eq!(read(r, ".rig-post.log").lines().count(), log_before + 1);
    assert!(r.join(".post-ran").exists());
    let msg = v["messages"].to_string();
    assert!(msg.contains("source=main"), "{msg}");
    assert!(msg.contains("source=channel:rig"), "{msg}");
    assert!(r.join("CHANGELOG.md").is_file());
    assert_eq!(
        read(r, ".claude/agents/reviewer.md"),
        "# reviewer\n\nReview read-only, v2.\n"
    );
    let paths: Vec<&str> = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
    assert!(paths.contains(&"AGENTS.md#rig"), "{paths:?}");
    assert!(paths.contains(&"CHANGELOG.md"), "{paths:?}");
}

#[test]
fn all_one_drifted_source_writes_nothing() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let r = repo.path();
    add_full(r, &ch);
    std::fs::write(r.join(".claude/agents/reviewer.md"), "# mine\n").unwrap();
    commit_all(r);
    let main_before = bytes(r, MAIN_LOCK);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_all(r).code(1));
    let msg = v["messages"].to_string();
    assert!(msg.contains("channel rig: "), "{msg}");
    assert!(msg.contains(".claude/agents/reviewer.md"), "{msg}");
    assert!(msg.contains("nothing was written"), "{msg}");
    assert_eq!(status(r), "");
    assert_eq!(bytes(r, MAIN_LOCK), main_before);
    assert_eq!(bytes(r, RIG_TOML), rig_before);
}

#[test]
fn all_claim_conflict_across_new_versions() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2-x", "v2.0.0")]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo_tagged("channel", &[("v1", "v1.0.0"), ("v2-x", "v2.0.0")]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch), "--ref", "v1.0.0"]);
    let main_before = bytes(r, MAIN_LOCK);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_all(r).code(2));
    let msg = v["messages"].to_string();
    assert!(msg.contains("path x.txt is owned by main"), "{msg}");
    assert_eq!(status(r), "");
    assert_eq!(bytes(r, MAIN_LOCK), main_before);
    assert_eq!(bytes(r, RIG_TOML), rig_before);
}

#[test]
fn all_post_dedup_and_before_locks() {
    let (_tpl, repo, _ch, _chb) = setup_two(&["v1"]);
    let r = repo.path();
    let log = read(r, ".rig-post.log");
    let (posts, old) = (lines_eq(&log, "rig-post"), lines_eq(&log, "old-lock"));
    stdout_json(update_all(r).success());
    let log = read(r, ".rig-post.log");
    assert_eq!(lines_eq(&log, "rig-post"), posts + 1, "{log}");
    assert_eq!(lines_eq(&log, "old-lock"), old + 1, "{log}");
    assert_eq!(chan_version(r, "rig"), "2.0.0");
    assert_eq!(chan_version(r, "rigb"), "2.0.0");
    assert_eq!(main_version(r), "1.0.0");
}

#[test]
fn all_failure_in_second_source_writes_no_lock() {
    // Main stays at v1 (its own v2 step would fail first under the same env).
    let (_tpl, repo, _ch, _chb) = setup_two(&["v1"]);
    let r = repo.path();
    let before: Vec<Vec<u8>> = [MAIN_LOCK, RIG_TOML, RIGB_TOML]
        .iter()
        .map(|p| bytes(r, p))
        .collect();
    let v = stdout_json(update_all_env(r, "SPLICE_FIXTURE_FAIL", "1").code(3));
    let msg = v["messages"].to_string();
    assert!(msg.contains("channel rigb: "), "{msg}");
    assert!(msg.contains("no lock was written"), "{msg}");
    assert!(msg.contains("git checkout -- . && git clean -fd"), "{msg}");
    for (p, b) in [MAIN_LOCK, RIG_TOML, RIGB_TOML].iter().zip(&before) {
        assert_eq!(&bytes(r, p), b, "{p}");
    }
    // rig's files were written before rigb failed.
    assert!(read(r, ".claude/agents/reviewer.md").contains("v2."));
    let out = std::process::Command::new("sh")
        .args(["-c", "git checkout -- . && git clean -fdq"])
        .current_dir(r)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(status(r), "");
    update_all(r).success();
    assert_eq!(chan_version(r, "rig"), "2.0.0");
    assert_eq!(chan_version(r, "rigb"), "2.0.0");
}

#[test]
fn all_failed_post_writes_no_lock() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let r = repo.path();
    add_full(r, &ch);
    std::fs::remove_file(r.join(".rig-post.log")).unwrap();
    std::fs::create_dir(r.join(".rig-post.log")).unwrap();
    std::fs::write(r.join(".rig-post.log/keep"), "").unwrap();
    commit_all(r);
    let main_before = bytes(r, MAIN_LOCK);
    let rig_before = bytes(r, RIG_TOML);
    let v = stdout_json(update_all(r).code(3));
    let msg = v["messages"].to_string();
    assert!(msg.contains("echo rig-post >> .rig-post.log"), "{msg}");
    assert!(msg.contains("no lock was written"), "{msg}");
    assert_eq!(bytes(r, MAIN_LOCK), main_before);
    assert_eq!(bytes(r, RIG_TOML), rig_before);
}

#[test]
fn all_seed_then_section_same_host() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let chb = chb_v1_v2();
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&chb), "--ref", "v1.0.0"]);
    assert!(!r.join("CHANGELOG.md").exists());
    update_all(r).success();
    let seed = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/template/v2/pieces/base/files/CHANGELOG.md"),
    )
    .unwrap();
    let block = "<!-- splice:rigb:begin — managed by `splice update --channel rigb`; edit the source -->\n\
                 - rigb 2.0.0 changed the notes.\n<!-- splice:rigb:end -->\n";
    assert_eq!(read(r, "CHANGELOG.md"), format!("{seed}\n{block}"));
    let cl = ChannelLock::read(r, "rigb").unwrap().unwrap();
    assert!(!cl.sections["CHANGELOG.md"].created, "{:?}", cl.sections);
    assert_eq!(main_version(r), "2.0.0");
}

#[test]
fn all_in_channel_only_repo() {
    let repo = bare_repo();
    let ch = channel_repo(&["v1", "v2"]);
    let r = repo.path();
    chan_commit(r, &["channel", "add", &src(&ch), "--ref", "v1.0.0"]);
    let v = stdout_json(update_all(r).success());
    assert_eq!(chan_version(r, "rig"), "2.0.0");
    assert!(!r.join(MAIN_LOCK).exists());
    assert!(
        v["messages"].to_string().contains("source=channel:rig"),
        "{v}"
    );
    assert!(!v["messages"].to_string().contains("source=main"), "{v}");
}

#[test]
fn all_up_to_date() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    add_full(r, &ch);
    let v = stdout_json(update_all(r).success());
    let msgs: Vec<&str> = v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("main: ") && m.contains("up to date")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter()
            .any(|m| m.starts_with("channel rig: ") && m.contains("up to date")),
        "{msgs:?}"
    );
    assert_eq!(status(r), "");
}

#[test]
fn all_dirty_tree_refused() {
    let (_tpl, repo, _ch) = setup();
    let r = repo.path();
    std::fs::write(r.join("scratch.txt"), "x\n").unwrap();
    let before = status(r);
    let v = stdout_json(update_all(r).code(2));
    assert!(v["messages"].to_string().contains("--allow-dirty"), "{v}");
    assert_eq!(status(r), before);
}

#[test]
fn all_lock_write_failure_names_groups_and_converges() {
    // Channel `t` sorts after `rig`; its post (with the env set) puts a directory where its lock
    // goes, so rig.toml is written and t.toml is not.
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo_at(&tpl, "v1.0.0");
    let ch = channel_repo(&["v1", "v2"]);
    let r = repo.path();
    add_full(r, &ch);
    let toml = "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\"a.txt\"]\n\
                [[post]]\nrun = \"test -z \\\"$SPLICE_FIXTURE_BREAK\\\" || { rm .splice/channels/t.toml && mkdir .splice/channels/t.toml && touch .splice/channels/t.toml/x; }\"\nwhen = [\"update\"]\n";
    let t = inline_template(&[("core", toml, &[("a.txt", "a\n")])]);
    git(
        t.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(t.path(), "v1", "v1.0.0");
    chan_commit(r, &["channel", "add", t.path().to_str().unwrap()]);
    std::fs::write(t.path().join("pieces/core/files/a.txt"), "a2\n").unwrap();
    commit_tag(t.path(), "v2", "v2.0.0");

    let v = stdout_json(update_all_env(r, "SPLICE_FIXTURE_BREAK", "1").code(3));
    let msg = v["messages"].to_string();
    assert!(
        msg.contains("locks written: .splice/lock.toml, .splice/channels/rig.toml"),
        "{msg}"
    );
    assert!(
        msg.contains("not confirmed written: .splice/channels/t.toml"),
        "{msg}"
    );
    assert!(msg.contains("locks not written: none"), "{msg}");
    assert_eq!(main_version(r), "2.0.0");
    assert_eq!(chan_version(r, "rig"), "2.0.0");

    // Restore the old t.toml and re-run: everything converges on the targets.
    std::fs::remove_dir_all(r.join(".splice/channels/t.toml")).unwrap();
    git(
        r,
        "2026-03-01T00:00:00+00:00",
        &["checkout", "--", ".splice/channels/t.toml"],
    );
    assert_eq!(chan_version(r, "t"), "1.0.0");
    commit_all(r);
    stdout_json(update_all(r).success());
    assert_eq!(main_version(r), "2.0.0");
    assert_eq!(chan_version(r, "rig"), "2.0.0");
    assert_eq!(chan_version(r, "t"), "2.0.0");
    assert_eq!(read(r, "a.txt"), "a2\n");
}

#[test]
fn subtree_equal_commit_with_filter_change_applies() {
    let (_tpl, repo) = main_repo();
    let ch = channel_repo(&["v1"]);
    let r = repo.path();
    add_full(r, &ch);
    std::fs::write(ch.path().join("README.md"), "# changed outside\n").unwrap();
    commit_tag(ch.path(), "readme only", "v3.0.0");
    let before = bytes(r, RIG_TOML);
    update_rig(r, &["--exclude", CAVEMAN_GLOB]).success();
    assert!(!r.join(CAVEMAN).exists());
    assert_ne!(bytes(r, RIG_TOML), before);
    let cl = rig(r);
    assert_eq!(cl.meta.exclude, [CAVEMAN_GLOB]);
    assert!(!cl.lock.files.contains_key(CAVEMAN));
}

#[test]
fn all_two_channels_one_host_keep_both_blocks() {
    // Channels `a` and `b` (one source, `--as`) both host a section in HOST.md; --all writes
    // both new bodies (the second plan sees the first one's host write).
    let toml = "id = \"core\"\ndescription = \"d\"\nmultiple = false\n[sections]\n\"HOST.md\" = \"s.md\"\n";
    let t = inline_template(&[("core", toml, &[("s.md", "- one\n")])]);
    git(
        t.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(t.path(), "v1", "v1.0.0");
    let repo = bare_repo();
    let r = repo.path();
    let spec = t.path().to_str().unwrap();
    chan_commit(r, &["channel", "add", spec, "--as", "a"]);
    chan_commit(r, &["channel", "add", spec, "--as", "b"]);
    std::fs::write(t.path().join("pieces/core/files/s.md"), "- two\n").unwrap();
    commit_tag(t.path(), "v2", "v2.0.0");
    update_all(r).success();
    let host = read(r, "HOST.md");
    assert!(!host.contains("- one"), "{host}");
    assert_eq!(host.matches("- two\n").count(), 2, "{host}");
}
