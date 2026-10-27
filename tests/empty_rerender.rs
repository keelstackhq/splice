//! Contract rule 9.27: an owned/shared file whose new render is empty is removed under the §7
//! rule for removed files and dropped from the lock; `check` is clean afterwards.

mod common;

use common::{commit_all, commit_tag, git, inline_template, splice_cmd};
use splice::lock::Lock;
use tempfile::TempDir;

const GUARDED: &str = "{% if \"off\" in pieces %}{% else %}x{% endif %}";
const EMPTY: &str = "{% if \"nope\" in pieces %}x{% endif %}";

fn template(ci: &str, shared: bool) -> TempDir {
    let class = if shared { "shared" } else { "owned" };
    let merge = if shared {
        "[merge]\n\"ci.yml\" = \"text\"\n"
    } else {
        ""
    };
    let tpl = inline_template(&[
        (
            "base",
            &format!(
                "id = \"base\"\ndescription = \"d\"\nmultiple = false\n[files]\n{class} = [\"ci.yml\"]\n{merge}"
            ),
            &[("ci.yml.jinja", ci)],
        ),
        (
            "off",
            "id = \"off\"\ndescription = \"d\"\nmultiple = false\n[files]\nowned = [\"off.txt\"]\n",
            &[("off.txt", "off\n")],
        ),
    ]);
    git(
        tpl.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(tpl.path(), "v1", "v1.0.0");
    tpl
}

fn new_repo(tpl: &TempDir) -> TempDir {
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
    assert_eq!(
        std::fs::read_to_string(repo.path().join("ci.yml")).unwrap(),
        "x"
    );
    repo
}

fn run(repo: &TempDir, cmd: &str, args: &[&str]) -> assert_cmd::assert::Assert {
    commit_all(repo.path());
    splice_cmd()
        .current_dir(repo.path())
        .arg(cmd)
        .args(args)
        .args(["--json"])
        .assert()
}

fn json(out: Vec<u8>) -> serde_json::Value {
    serde_json::from_slice(&out).expect("stdout is exactly one JSON object")
}

fn tracked(repo: &TempDir) -> bool {
    Lock::read(repo.path())
        .unwrap()
        .unwrap()
        .files
        .contains_key("ci.yml")
}

#[test]
fn add_deletes_unedited_file_and_drops_it_from_lock() {
    let repo = new_repo(&template(GUARDED, false));
    assert!(tracked(&repo));
    let out = run(&repo, "add", &["off", "--yes"])
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    let change = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "ci.yml")
        .unwrap_or_else(|| panic!("{v}"));
    assert_eq!(change["action"], "delete", "{v}");
    assert!(!repo.path().join("ci.yml").exists());
    assert!(!tracked(&repo));
    let out = run(&repo, "check", &[])
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json(out)["command"], "check");
}

#[test]
fn add_keeps_edited_file_reports_it_and_drops_it_from_lock() {
    let repo = new_repo(&template(GUARDED, false));
    std::fs::write(repo.path().join("ci.yml"), "mine\n").unwrap();
    let out = run(&repo, "add", &["off", "--yes"])
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert!(v["messages"].to_string().contains("ci.yml"), "{v}");
    assert!(
        v["messages"].to_string().contains("no longer shipped"),
        "{v}"
    );
    let change = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "ci.yml")
        .unwrap_or_else(|| panic!("{v}"));
    assert_eq!(change["action"], "skip", "{v}");
    assert_eq!(change["class"], "owned", "{v}");
    assert_eq!(
        std::fs::read_to_string(repo.path().join("ci.yml")).unwrap(),
        "mine\n"
    );
    assert!(!tracked(&repo));
    run(&repo, "check", &[]).success();
}

#[test]
fn update_deletes_shared_file_that_now_renders_empty() {
    let tpl = template(GUARDED, true);
    let repo = new_repo(&tpl);
    std::fs::write(tpl.path().join("pieces/base/files/ci.yml.jinja"), EMPTY).unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    let out = run(&repo, "update", &[])
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    let change = v["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["path"] == "ci.yml")
        .unwrap_or_else(|| panic!("{v}"));
    assert_eq!(change["action"], "delete", "{v}");
    assert!(!repo.path().join("ci.yml").exists());
    assert!(!tracked(&repo));
    run(&repo, "check", &[]).success();
}

#[test]
fn update_keeps_edited_file_that_now_renders_empty() {
    let tpl = template(GUARDED, false);
    let repo = new_repo(&tpl);
    std::fs::write(repo.path().join("ci.yml"), "mine\n").unwrap();
    std::fs::write(tpl.path().join("pieces/base/files/ci.yml.jinja"), EMPTY).unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    let out = run(&repo, "update", &[])
        .success()
        .get_output()
        .stdout
        .clone();
    let v = json(out);
    assert!(
        v["messages"].to_string().contains("no longer shipped"),
        "{v}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("ci.yml")).unwrap(),
        "mine\n"
    );
    assert!(!tracked(&repo));
}
