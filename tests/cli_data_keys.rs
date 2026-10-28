//! Contract rule 9.28: which `--data` keys each command accepts.

mod common;

use common::{commit_all, commit_tag, git, inline_template, splice_cmd};
use splice::lock::Lock;
use tempfile::TempDir;

const VAR: &str = "[vars]\nport = { prompt = \"p\", default = \"1\" }\n";

fn piece(id: &str, multiple: bool, vars: &str, files: &str) -> String {
    format!("id = \"{id}\"\ndescription = \"d\"\nmultiple = {multiple}\n{vars}{files}")
}

/// base (`port`, renders `base.txt`), `x` (`port`, `x.txt`), `y` (`k`, `y.txt`) and multiple `p`
/// (`name`, `p.txt`); `extra` is appended to splice.toml.
fn template(extra: &str) -> TempDir {
    let f = |n: &str| format!("[files]\nowned = [\"{n}.txt\"]\n");
    let base = piece("base", false, VAR, &f("base"));
    let x = piece("x", false, VAR, &f("x"));
    let y = piece(
        "y",
        false,
        "[vars]\nk = { prompt = \"k\", default = \"d\" }\n",
        &f("y"),
    );
    let p = piece(
        "p",
        true,
        "[vars]\nname = { prompt = \"n\", default = \"def\" }\n",
        &f("p"),
    );
    let tpl = inline_template(&[
        ("base", &base, &[("base.txt.jinja", "b{{ port }}\n")]),
        ("x", &x, &[("x.txt.jinja", "x{{ port }}\n")]),
        ("y", &y, &[("y.txt", "y\n")]),
        ("p", &p, &[("p.txt.jinja", "{{ name }}\n")]),
    ]);
    let toml = tpl.path().join("splice.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, text + extra).unwrap();
    git(
        tpl.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    commit_tag(tpl.path(), "v1", "v1.0.0");
    tpl
}

fn new(tpl: &TempDir, repo: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--yes", "--json"])
        .args(args)
        .assert()
}

fn in_repo(repo: &TempDir, args: &[&str]) -> assert_cmd::assert::Assert {
    commit_all(repo.path());
    splice_cmd()
        .current_dir(repo.path())
        .args(args)
        .arg("--json")
        .assert()
}

fn fails(a: assert_cmd::assert::Assert, code: i32, msg: &str) {
    let out = a.code(code).get_output().clone();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(all.contains(msg), "{msg:?} not in {all}");
}

fn read(repo: &TempDir, rel: &str) -> String {
    std::fs::read_to_string(repo.path().join(rel)).unwrap()
}

#[test]
fn new_rejects_dotted_and_unselected_piece_keys() {
    let tpl = template("");
    let msg = "no such variable in the template or the selected pieces";
    for args in [
        &["--piece", "x", "--data", "x.port=1"][..],
        &["--piece", "x", "--data", "base.port=1"],
        &["--data", "k=1"],
    ] {
        let repo = tempfile::tempdir().unwrap();
        fails(new(&tpl, &repo, args), 2, msg);
    }
}

#[test]
fn add_and_update_reject_dotted_keys() {
    let tpl = template("");
    let repo = tempfile::tempdir().unwrap();
    new(&tpl, &repo, &[]).success();
    fails(
        in_repo(&repo, &["add", "x", "--yes", "--data", "x.port=1"]),
        2,
        "no such variable in piece",
    );
    std::fs::write(tpl.path().join("pieces/base/files/base.txt.jinja"), "B\n").unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    fails(
        in_repo(&repo, &["update", "--data", "base.port=2"]),
        2,
        "no such variable in template",
    );
}

#[test]
fn add_name_wins_over_name_var() {
    let repo = tempfile::tempdir().unwrap();
    new(&template(""), &repo, &[]).success();
    in_repo(
        &repo,
        &["add", "p", "--yes", "--name", "api", "--data", "name=other"],
    )
    .success();
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert!(lock.pieces.iter().any(|p| p.id == "p" && p.name == "api"));
    assert_eq!(read(&repo, "p.txt"), "api\n");
}

#[test]
fn new_applies_one_piece_var_to_every_selected_piece() {
    let repo = tempfile::tempdir().unwrap();
    new(&template(""), &repo, &["--piece", "x", "--data", "port=7"]).success();
    assert_eq!(read(&repo, "base.txt"), "b7\n");
    assert_eq!(read(&repo, "x.txt"), "x7\n");
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.pieces.len(), 2);
    for p in &lock.pieces {
        assert_eq!(p.vars["port"], "7", "{}", p.id);
    }
}

#[test]
fn add_rejects_template_var() {
    let tpl = template("[vars]\nscope = { prompt = \"s\", default = \"a\" }\n");
    let repo = tempfile::tempdir().unwrap();
    new(&tpl, &repo, &[]).success();
    fails(
        in_repo(&repo, &["add", "x", "--yes", "--data", "scope=z"]),
        2,
        "no such variable in piece",
    );
}

#[test]
fn update_rejects_piece_var() {
    let tpl = template("");
    let repo = tempfile::tempdir().unwrap();
    new(&tpl, &repo, &[]).success();
    std::fs::write(
        tpl.path().join("pieces/base/files/base.txt.jinja"),
        "B{{ port }}\n",
    )
    .unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    fails(
        in_repo(&repo, &["update", "--data", "port=2"]),
        2,
        "is a piece variable",
    );
}

#[test]
fn piece_var_shadowing_template_var_is_exit_3() {
    let tpl = template("[vars]\nport = { prompt = \"p\", default = \"1\" }\n");
    let repo = tempfile::tempdir().unwrap();
    fails(new(&tpl, &repo, &[]), 3, "shadows the template variable");
}

#[test]
fn piece_name_wins_over_name_var() {
    let repo = tempfile::tempdir().unwrap();
    new(
        &template(""),
        &repo,
        &["--piece", "p:api", "--data", "name=other"],
    )
    .success();
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert!(lock.pieces.iter().any(|p| p.id == "p" && p.name == "api"));
    assert_eq!(read(&repo, "p.txt"), "api\n");
}

#[test]
fn two_unnamed_instances_collide_under_data_name() {
    let repo = tempfile::tempdir().unwrap();
    let args = ["--piece", "p", "--piece", "p", "--data", "name=x"];
    fails(
        new(&template(""), &repo, &args),
        2,
        "piece `p` has two instances named `x`",
    );
}

/// `new` with port=7, then v2 adds base var `extra` (`decl`) and `update` runs with `args`.
fn update_v2(decl: &str, args: &[&str]) -> (TempDir, assert_cmd::assert::Assert) {
    let tpl = template("");
    let repo = tempfile::tempdir().unwrap();
    new(&tpl, &repo, &["--data", "port=7"]).success();
    let toml = tpl.path().join("pieces/base/piece.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(&toml, text.replace(VAR, &format!("{VAR}extra = {decl}\n"))).unwrap();
    std::fs::write(
        tpl.path().join("pieces/base/files/base.txt.jinja"),
        "b{{ port }}{{ extra }}\n",
    )
    .unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    let a = in_repo(&repo, &[&["update"], args].concat());
    (repo, a)
}

#[test]
fn update_keeps_locked_piece_vars_and_defaults_new_one() {
    let (repo, a) = update_v2("{ prompt = \"e\", default = \"e\" }", &["--yes"]);
    a.success();
    assert_eq!(read(&repo, "base.txt"), "b7e\n");
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    let vars = &lock.pieces[0].vars;
    assert_eq!((vars["port"].as_str(), vars["extra"].as_str()), ("7", "e"));
}

#[test]
fn update_new_piece_var_without_default_is_exit_2() {
    let (_repo, a) = update_v2("{ prompt = \"e\" }", &["--yes"]);
    fails(a, 2, "missing value for var `extra`");
}

#[test]
fn update_data_cannot_set_new_piece_var() {
    let (_repo, a) = update_v2("{ prompt = \"e\" }", &["--yes", "--data", "extra=z"]);
    fails(a, 2, "is a piece variable");
}

#[test]
fn new_sets_template_var() {
    let tpl = template("[vars]\nscope = { prompt = \"s\", default = \"a\" }\n");
    let file = tpl.path().join("pieces/base/files/base.txt.jinja");
    std::fs::write(file, "b{{ port }}{{ scope }}\n").unwrap();
    commit_tag(tpl.path(), "v2", "v2.0.0");
    let repo = tempfile::tempdir().unwrap();
    new(&tpl, &repo, &["--data", "scope=z"]).success();
    assert_eq!(read(&repo, "base.txt"), "b1z\n");
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    assert_eq!(lock.vars["scope"], "z");
}

#[test]
fn add_piece_var_leaves_locked_pieces_alone() {
    let repo = tempfile::tempdir().unwrap();
    new(&template(""), &repo, &["--data", "port=7"]).success();
    in_repo(&repo, &["add", "x", "--yes", "--data", "port=9"]).success();
    assert_eq!(
        (read(&repo, "base.txt"), read(&repo, "x.txt")),
        ("b7\n".into(), "x9\n".into())
    );
    let lock = Lock::read(repo.path()).unwrap().unwrap();
    let port = |id: &str| lock.pieces.iter().find(|p| p.id == id).unwrap().vars["port"].clone();
    assert_eq!(
        (port("base"), port("x")),
        ("7".to_string(), "9".to_string())
    );
}

#[test]
fn add_rejects_var_of_another_locked_piece() {
    let repo = tempfile::tempdir().unwrap();
    new(&template(""), &repo, &["--piece", "y"]).success();
    fails(
        in_repo(&repo, &["add", "x", "--yes", "--data", "k=1"]),
        2,
        "no such variable in piece",
    );
}
