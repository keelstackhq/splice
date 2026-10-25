//! `splice update` end to end against the fixture template (spec §8.3).

mod common;

use common::{
    amend_tip, commit_all, commit_tag, splice_cmd, template_repo, template_repo_tagged,
    tree_snapshot,
};
use splice::lock::Lock;
use tempfile::TempDir;

/// `splice new` from `tpl` into a fresh repo, then commit everything so the tree is clean.
fn new_repo(tpl: &TempDir, extra: &[&str]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--data", "project=demo", "--yes", "--json"])
        .args(extra)
        .assert()
        .success();
    commit_all(repo.path());
    repo
}

fn update(repo: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd()
        .current_dir(repo.path())
        .arg("update")
        .args(args)
        .args(["--yes", "--json"])
        .assert()
}

fn json(out: &[u8]) -> serde_json::Value {
    serde_json::from_slice(out).expect("stdout is exactly one JSON object")
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

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

fn lock_bytes(repo: &TempDir) -> Vec<u8> {
    std::fs::read(repo.path().join(".splice/lock.toml")).unwrap()
}

fn lock(repo: &TempDir) -> Lock {
    Lock::read(repo.path()).unwrap().unwrap()
}

fn porcelain(repo: &TempDir) -> String {
    common::git_stdout(repo.path(), &["status", "--porcelain"])
}

#[test]
fn updates_v1_to_v2() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    std::fs::remove_file(repo.path().join(".post-ran")).unwrap();
    commit_all(repo.path());

    let out = update(&repo, &[]).success().get_output().stdout.clone();
    let v = json(&out);
    assert_eq!(v["command"], "update");
    assert_eq!(v["ok"], true);
    let l = lock(&repo);
    assert_eq!(l.template.version.to_string(), "2.0.0");
    assert!(!repo.path().join("tool.config.mts").exists());
    assert!(!repo.path().join("old.txt").exists());
    assert!(repo.path().join("tool.config.ts").exists());
    assert!(read(&repo, ".editorconfig").contains("indent_size = 4"));
    assert!(read(&repo, "apps/web/tsconfig.json").contains("noUncheckedIndexedAccess"));
    // Migration step and post command (when ∋ update) ran.
    assert!(repo.path().join(".splice/migrated.txt").exists());
    assert!(repo.path().join(".post-ran").exists());
}

#[test]
fn dirty_tree_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    std::fs::write(repo.path().join("x"), "dirty").unwrap();
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).code(2).get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("--allow-dirty"));
    assert_eq!(lock_bytes(&repo), before);

    update(&repo, &["--allow-dirty"]).success();
    assert_eq!(lock(&repo).template.version.to_string(), "2.0.0");
    assert_eq!(read(&repo, "x"), "dirty");
}

#[test]
fn outside_work_tree_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();
    update(&repo, &[]).code(2);
    update(&repo, &["--allow-dirty"]).success();
}

#[test]
fn up_to_date_is_noop() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &[]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    let v = json(&out);
    assert!(messages(&v).contains("up to date"));
    assert_eq!(v["changes"].as_array().unwrap().len(), 0);
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn moved_tag_proceeds_with_message() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &[]);
    let old_commit = lock(&repo).template.commit;
    let notes = tpl.path().join("pieces/base/files/notes.txt.jinja");
    let text = std::fs::read_to_string(&notes).unwrap();
    std::fs::write(&notes, format!("{text}Moved.\n")).unwrap();
    amend_tip(tpl.path(), "v2.0.0");

    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("moved"));
    assert_ne!(lock(&repo).template.commit, old_commit);
    assert!(read(&repo, "notes.txt").contains("Moved."));
}

#[test]
fn downgrade_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &[]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &["--to", "1.0.0"])
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert!(messages(&json(&out)).contains("downgrade"));
    assert_eq!(lock_bytes(&repo), before);
}

#[test]
fn conflicts_written_lock_records_target() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let notes = read(&repo, "notes.txt");
    let mut lines: Vec<&str> = notes.lines().collect();
    lines[1] = "My own second line.";
    std::fs::write(repo.path().join("notes.txt"), lines.join("\n") + "\n").unwrap();
    commit_all(repo.path());

    let out = update(&repo, &[]).code(1).get_output().stdout.clone();
    let v = json(&out);
    assert!(messages(&v).contains("notes.txt"));
    let text = read(&repo, "notes.txt");
    assert!(text.contains("<<<<<<< ours") && text.contains(">>>>>>> template"));
    assert_eq!(lock(&repo).template.version.to_string(), "2.0.0");
}

#[test]
fn min_splice_refused_before_write() {
    let tpl = template_repo(&["v1", "v2", "v3"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let out = update(&repo, &["--to", "3.0.0"])
        .code(3)
        .get_output()
        .stdout
        .clone();
    assert!(messages(&json(&out)).contains("99.0.0"));
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn owned_drift_refused_then_forced() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    std::fs::write(
        repo.path().join("apps/web/tsconfig.json"),
        "{ \"mine\": 1 }\n",
    )
    .unwrap();
    commit_all(repo.path());

    let out = update(&repo, &[]).code(1).get_output().stdout.clone();
    let v = json(&out);
    assert_eq!(v["ok"], false);
    assert_eq!(v["changes"].as_array().unwrap().len(), 0);
    assert!(messages(&v).contains("apps/web/tsconfig.json"));
    assert_eq!(porcelain(&repo), "");

    let out = update(&repo, &["--force"])
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(&out);
    assert!(read(&repo, "apps/web/tsconfig.json").contains("noUncheckedIndexedAccess"));
    let change = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "apps/web/tsconfig.json")
        .expect("tsconfig change");
    assert_eq!(change["action"], "update");
    assert_eq!(change["class"], "owned");
}

#[test]
fn new_var_takes_default_with_yes() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    update(&repo, &[]).success();
    let l = lock(&repo);
    assert_eq!(l.vars.get("license").map(String::as_str), Some("MIT"));
    assert_eq!(l.vars.get("project").map(String::as_str), Some("demo"));
}

#[test]
fn new_var_from_data_and_unknown_data_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    update(&repo, &["--data", "nope=1"]).code(2);
    update(&repo, &["--data", "license=ISC"]).success();
    assert_eq!(
        lock(&repo).vars.get("license").map(String::as_str),
        Some("ISC")
    );
}

#[test]
fn locked_template_var_in_data_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &["--data", "project=other"])
        .code(2)
        .get_output()
        .stdout
        .clone();
    assert!(
        messages(&json(&out)).contains("`project` is locked at `demo`; update keeps lock values")
    );
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn piece_var_in_data_refused() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    let before = lock_bytes(&repo);
    update(&repo, &["--data", "name=other"]).code(2);
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn prerelease_lock_without_newer_stable_is_up_to_date() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2", "v2.0.0-rc.1")]);
    let repo = new_repo(&tpl, &["--ref", "v2.0.0-rc.1"]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(
        messages(&json(&out)).contains(
            "up to date (no newer stable than 2.0.0-rc.1; use --to <tag> for a prerelease)"
        )
    );
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
    // An explicit older --to is still a downgrade.
    update(&repo, &["--to", "1.0.0"]).code(2);
}

#[test]
fn prerelease_only_template_is_up_to_date() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0-rc.1"), ("v2", "v2.0.0-rc.1")]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0-rc.1"]);
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(
        messages(&json(&out)).contains(
            "up to date (no newer stable than 1.0.0-rc.1; use --to <tag> for a prerelease)"
        ),
        "{}",
        messages(&json(&out))
    );
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn removed_piece_is_template_error() {
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2-nolib", "v2.0.0")]);
    let repo = new_repo(
        &tpl,
        &["--ref", "v1.0.0", "--piece", "app", "--piece", "lib"],
    );
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).code(3).get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("lib"));
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn remove_paths_reach_the_planner() {
    // A [[remove]] of a path the target still renders is ignored with a warning: proof that the
    // rendered `[[remove]]` paths are passed to the planner.
    let tpl = template_repo(&["v1", "v2"]);
    let mig = tpl.path().join("migrations/2.0.0.toml");
    let text = std::fs::read_to_string(&mig).unwrap();
    std::fs::write(&mig, format!("{text}\n[[remove]]\npath = \"notes.txt\"\n")).unwrap();
    amend_tip(tpl.path(), "v2.0.0");
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert!(messages(&json(&out)).contains("still renders it"));
    assert!(repo.path().join("notes.txt").exists());
}

#[test]
fn remove_paths_render_with_the_from_version() {
    // Contract §9.22: `template.version` in a [[remove]] path is the version updated FROM
    // (1.0.0), not the target (2.0.0). `template.name` renders too (strict undefined: exit 3).
    let tpl = template_repo(&["v1", "v2"]);
    let mig = tpl.path().join("migrations/2.0.0.toml");
    let text = std::fs::read_to_string(&mig).unwrap();
    std::fs::write(
        &mig,
        format!("{text}\n[[remove]]\npath = \"{{{{ template.name }}}}-{{{{ template.version }}}}.cfg\"\n"),
    )
    .unwrap();
    amend_tip(tpl.path(), "v2.0.0");
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    std::fs::write(repo.path().join("fixture-1.0.0.cfg"), "from\n").unwrap();
    std::fs::write(repo.path().join("fixture-2.0.0.cfg"), "to\n").unwrap();
    commit_all(repo.path());
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    let msgs = messages(&json(&out));
    assert!(
        msgs.contains("`fixture-1.0.0.cfg` is listed in a [[remove]]"),
        "{msgs}"
    );
    assert!(!msgs.contains("fixture-2.0.0.cfg"), "{msgs}");
    assert_eq!(read(&repo, "fixture-1.0.0.cfg"), "from\n");
    assert_eq!(lock(&repo).template.version.to_string(), "2.0.0");
}

#[test]
fn update_refuses_a_target_that_adds_sections() {
    let tpl = template_repo(&["v1"]);
    let repo = new_repo(&tpl, &[]);
    let base = tpl.path().join("pieces/base");
    let toml = std::fs::read_to_string(base.join("piece.toml")).unwrap();
    std::fs::write(
        base.join("piece.toml"),
        format!("{toml}\n[sections]\n\"NOTES.md\" = \"frag.md\"\n"),
    )
    .unwrap();
    std::fs::write(base.join("files/frag.md"), "hello\n").unwrap();
    // A template with [sections] needs splice 0.2.0 (contract §9.26).
    let manifest = tpl.path().join("splice.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(&manifest, text.replace("\"0.1.0\"", "\"0.2.0\"")).unwrap();
    commit_tag(tpl.path(), "v2 with sections", "v2.0.0");
    let (lock, tree) = (lock_bytes(&repo), tree_snapshot(repo.path()));
    let out = update(&repo, &[]).code(3).get_output().clone();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("piece `base` declares [sections]"), "{text}");
    assert_eq!(lock_bytes(&repo), lock);
    assert_eq!(tree_snapshot(repo.path()), tree);
}

#[test]
fn main_lock_identical_with_channels_present() {
    // Guard: a channel lock next to the main lock does not change what `update` writes there.
    let tpl = template_repo(&["v1", "v2"]);
    let plain = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let with = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lock/channel-rig.toml");
    let chan_path = with.path().join(".splice/channels/rig.toml");
    std::fs::create_dir_all(chan_path.parent().unwrap()).unwrap();
    std::fs::copy(&fixture, &chan_path).unwrap();
    commit_all(with.path());
    let chan_before = std::fs::read(&chan_path).unwrap();
    update(&plain, &[]).success();
    update(&with, &[]).success();
    assert_eq!(lock(&plain).template.version.to_string(), "2.0.0");
    assert_eq!(lock_bytes(&plain), lock_bytes(&with));
    assert_eq!(std::fs::read(&chan_path).unwrap(), chan_before);
}

#[test]
fn update_from_a_base_that_breaks_new_rules_succeeds() {
    // The from-version is only the merge base: it loads and renders without the §9.15-26 gates
    // (raw with min_splice 0.1.0, a var named `is`); the target stays strict.
    let tpl = template_repo(&["v1"]);
    let repo = new_repo(&tpl, &[]);
    let old_commit = lock(&repo).template.commit.clone();
    let manifest = tpl.path().join("splice.toml");
    let compliant = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        format!("{compliant}is = {{ prompt = \"p\", default = \"x\" }}\n"),
    )
    .unwrap();
    std::fs::write(
        tpl.path().join("pieces/base/files/raw.txt.jinja"),
        "{% raw %}{{ x }}{% endraw %}\n",
    )
    .unwrap();
    amend_tip(tpl.path(), "v1.0.0");
    let new_commit = common::git_stdout(tpl.path(), &["rev-parse", "v1.0.0^{commit}"]);
    let new_commit = new_commit.trim();
    assert_ne!(old_commit, new_commit);
    let lock_path = repo.path().join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock_path).unwrap();
    std::fs::write(&lock_path, text.replace(&old_commit, new_commit)).unwrap();
    std::fs::write(repo.path().join("raw.txt"), "{{ x }}\n").unwrap();
    commit_all(repo.path());
    // v2: compliant (min_splice 0.2.0, no `is`).
    std::fs::write(
        &manifest,
        compliant.replace("min_splice = \"0.1.0\"", "min_splice = \"0.2.0\""),
    )
    .unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    let out = update(&repo, &[]).success().get_output().stdout.clone();
    assert_eq!(
        lock(&repo).template.version.to_string(),
        "2.0.0",
        "{}",
        messages(&json(&out))
    );
}

#[test]
fn locked_instance_name_that_breaks_rules_is_exit_3_naming_the_lock() {
    let tpl = template_repo(&["v1", "v2"]);
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app:web"]);
    let lock_path = repo.path().join(".splice/lock.toml");
    let text = std::fs::read_to_string(&lock_path).unwrap();
    assert!(text.contains("\"web\""));
    std::fs::write(&lock_path, text.replace("\"web\"", "\"nul\"")).unwrap();
    commit_all(repo.path());
    let before = lock_bytes(&repo);
    let out = update(&repo, &[]).code(3).get_output().stdout.clone();
    let msgs = messages(&json(&out));
    assert!(
        msgs.contains(".splice/lock.toml") && msgs.contains("nul"),
        "{msgs}"
    );
    assert_eq!(lock_bytes(&repo), before);
}

// --- `update --ref` and untagged versions (spec §4, §10 item 21) ---

fn short(sha: &str) -> &str {
    &sha[..7]
}

/// A template with tag v1.0.0 plus one untagged descendant commit `x`.
fn untagged_tpl() -> (TempDir, String) {
    let tpl = template_repo(&["v1"]);
    let x = common::untagged_commit(tpl.path(), "x");
    (tpl, x)
}

#[test]
fn ref_moves_a_tagged_lock_to_an_untagged_descendant() {
    let (tpl, x) = untagged_tpl();
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    update(&repo, &["--ref", &x]).success();
    let l = lock(&repo);
    assert_eq!(
        l.template.version.to_string(),
        format!("0.0.0+g{}", short(&x))
    );
    assert_eq!(l.template.commit, x);
    assert!(read(&repo, ".editorconfig").contains("# x"));
}

#[test]
fn ref_between_untagged_commits_ignores_text_order_of_the_shas() {
    let tpl = template_repo(&["v1"]);
    let x = common::untagged_commit(tpl.path(), "x");
    // Descendants of x until one has a sha7 that sorts below x's (what semver would call older).
    let y = (0..40)
        .map(|i| common::untagged_commit(tpl.path(), &format!("y{i}")))
        .find(|y| short(y) < short(&x))
        .expect("a descendant whose sha7 sorts below x's");
    let repo = new_repo(&tpl, &["--ref", &x]);
    update(&repo, &["--ref", &y]).success();
    assert_eq!(lock(&repo).template.commit, y);
}

#[test]
fn ref_to_a_non_descendant_is_refused_unless_forced() {
    let (tpl, x) = untagged_tpl();
    let y = common::untagged_commit(tpl.path(), "y");
    let repo = new_repo(&tpl, &["--ref", &y]);
    let before = lock_bytes(&repo);
    let v = json(&update(&repo, &["--ref", &x]).code(2).get_output().stdout);
    let msg = messages(&v);
    assert!(msg.contains("not a descendant"), "{msg}");
    assert!(msg.contains("--force"), "{msg}");
    assert_eq!(lock_bytes(&repo), before);
    let v = json(
        &update(&repo, &["--ref", &x, "--force"])
            .success()
            .get_output()
            .stdout,
    );
    let msg = messages(&v);
    assert!(
        msg.contains("--force") && msg.contains("not a descendant"),
        "{msg}"
    );
    assert_eq!(lock(&repo).template.commit, x);
}

#[test]
fn ref_at_the_lock_commit_is_up_to_date() {
    let (tpl, x) = untagged_tpl();
    let repo = new_repo(&tpl, &["--ref", &x]);
    let before = lock_bytes(&repo);
    let v = json(&update(&repo, &["--ref", &x]).success().get_output().stdout);
    assert!(messages(&v).contains("up to date"), "{v}");
    assert_eq!(lock_bytes(&repo), before);
    assert_eq!(porcelain(&repo), "");
}

#[test]
fn ref_to_an_older_tag_is_refused() {
    let (tpl, x) = untagged_tpl();
    let repo = new_repo(&tpl, &["--ref", &x]);
    let v = json(
        &update(&repo, &["--ref", "v1.0.0"])
            .code(2)
            .get_output()
            .stdout,
    );
    assert!(messages(&v).contains("not a descendant"), "{v}");
}

#[test]
fn to_with_build_metadata_names_ref() {
    let (tpl, _x) = untagged_tpl();
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    let v = json(
        &update(&repo, &["--to", "0.0.0+gabc1234"])
            .code(2)
            .get_output()
            .stdout,
    );
    assert!(messages(&v).contains("--ref abc1234"), "{v}");
}

#[test]
fn to_a_tag_from_an_untagged_lock_needs_a_descendant() {
    // v2.0.0 on a side branch: not a descendant of x.
    let (tpl, x) = untagged_tpl();
    let date = "2026-02-02T00:00:00+00:00";
    common::git(
        tpl.path(),
        date,
        &["checkout", "-q", "-b", "side", "v1.0.0"],
    );
    common::untagged_commit(tpl.path(), "side");
    common::git(tpl.path(), date, &["tag", "v2.0.0"]);
    common::git(tpl.path(), date, &["checkout", "-q", "main"]);
    let repo = new_repo(&tpl, &["--ref", &x]);
    let v = json(
        &update(&repo, &["--to", "2.0.0"])
            .code(2)
            .get_output()
            .stdout,
    );
    assert!(messages(&v).contains("not a descendant"), "{v}");

    // v2.0.0 on a descendant of x: fine.
    let (tpl, x) = untagged_tpl();
    common::untagged_commit(tpl.path(), "next");
    common::git(tpl.path(), date, &["tag", "v2.0.0"]);
    let repo = new_repo(&tpl, &["--ref", &x]);
    update(&repo, &["--to", "2.0.0"]).success();
    assert_eq!(lock(&repo).template.version.to_string(), "2.0.0");
}

#[test]
fn ref_conflicts_with_to_and_all() {
    let (tpl, x) = untagged_tpl();
    let repo = new_repo(&tpl, &["--ref", "v1.0.0"]);
    update(&repo, &["--ref", &x, "--to", "1.0.0"]).code(2);
    update(&repo, &["--all", "--ref", &x]).code(2);
}

// --- latest and migrations from an untagged lock (spec §10 item 21) ---

#[test]
fn latest_from_an_untagged_lock_ignores_older_tags() {
    let (tpl, x) = untagged_tpl();
    let repo = new_repo(&tpl, &["--ref", &x]);
    let before = lock_bytes(&repo);
    let v = json(&update(&repo, &[]).success().get_output().stdout);
    let m = messages(&v);
    assert!(m.contains("no stable tag descends"), "{m}");
    assert!(m.contains(short(&x)), "{m}");
    assert!(m.contains("--ref"), "{m}");
    assert_eq!(lock_bytes(&repo), before);
    // `--all` takes the same path: exit 0, nothing written.
    let v = json(&update(&repo, &["--all"]).success().get_output().stdout);
    assert!(messages(&v).contains("no stable tag descends"), "{v}");
    assert_eq!(lock_bytes(&repo), before);
}

/// Tags v1.0.0 and v2.0.0 (migration 2.0.0), then untagged descendant `y`.
fn migration_tpl() -> (TempDir, String) {
    let tpl = template_repo(&["v1", "v2"]);
    let y = common::untagged_commit(tpl.path(), "y");
    (tpl, y)
}

#[test]
fn migration_runs_from_a_tagged_lock_to_an_untagged_target() {
    let (tpl, y) = migration_tpl();
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    assert!(repo.path().join("tool.config.mts").exists());
    update(&repo, &["--ref", &y]).success();
    assert!(repo.path().join(".splice/migrated.txt").exists());
    assert!(!repo.path().join("tool.config.mts").exists());
}

#[test]
fn migration_before_an_untagged_lock_does_not_run() {
    let tpl = template_repo(&["v1", "v2"]);
    let x = common::untagged_commit(tpl.path(), "x");
    let y = common::untagged_commit(tpl.path(), "y");
    let repo = new_repo(&tpl, &["--ref", &x, "--piece", "app"]);
    update(&repo, &["--ref", &y]).success();
    assert!(!repo.path().join(".splice/migrated.txt").exists());
    assert_eq!(lock(&repo).template.commit, y);
}

#[test]
fn migration_without_its_tag_does_not_run_with_an_untagged_side() {
    // The v2 tree (with migrations/2.0.0.toml) is tagged v1.5.0: no tag v2.0.0 exists.
    let tpl = template_repo_tagged(&[("v1", "v1.0.0"), ("v2", "v1.5.0")]);
    let y = common::untagged_commit(tpl.path(), "y");
    let repo = new_repo(&tpl, &["--ref", "v1.0.0", "--piece", "app"]);
    update(&repo, &["--ref", &y]).success();
    assert!(!repo.path().join(".splice/migrated.txt").exists());
    assert_eq!(lock(&repo).template.commit, y);
}
