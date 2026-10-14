#![allow(dead_code)]
use assert_cmd::Command;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command as Proc;
use std::sync::OnceLock;
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/template")
}

fn empty_git_config() -> PathBuf {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    let d = DIR.get_or_init(|| tempfile::tempdir().unwrap());
    let p = d.path().join("gitconfig");
    if !p.exists() {
        std::fs::write(&p, "").unwrap();
    }
    p
}

fn cache_dir() -> PathBuf {
    static DIR: OnceLock<TempDir> = OnceLock::new();
    DIR.get_or_init(|| tempfile::tempdir().unwrap())
        .path()
        .to_path_buf()
}

fn set_git_env(c: &mut Proc) {
    c.env("GIT_CONFIG_GLOBAL", empty_git_config())
        .env("GIT_CONFIG_SYSTEM", empty_git_config())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
}

pub fn git(dir: &Path, args: &[&str]) {
    let mut c = Proc::new("git");
    set_git_env(&mut c);
    let out = c
        .current_dir(dir)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn clear_worktree(dir: &Path) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.file_name().unwrap() == ".git" {
            continue;
        }
        if p.is_dir() {
            std::fs::remove_dir_all(&p).unwrap();
        } else {
            std::fs::remove_file(&p).unwrap();
        }
    }
}

/// Create a git repo containing each fixture version in order, one commit and tag `v<N>.0.0` each
/// (fixture `v2` -> tag `v2.0.0`).
pub fn template_repo(versions: &[&str]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    for v in versions {
        clear_worktree(dir.path());
        copy_dir(&fixtures().join(v), dir.path());
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", v]);
        let n = v.trim_start_matches('v');
        git(dir.path(), &["tag", &format!("v{n}.0.0")]);
    }
    dir
}

/// A one-version template repo (tag `v1.0.0`) from fixture `v1` with `min_splice` rewritten.
pub fn template_repo_min_splice(min: &str) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    copy_dir(&fixtures().join("v1"), dir.path());
    let p = dir.path().join("splice.toml");
    let text = std::fs::read_to_string(&p).unwrap();
    assert!(text.contains("min_splice = \"0.1.0\""));
    std::fs::write(
        &p,
        text.replace("min_splice = \"0.1.0\"", &format!("min_splice = \"{min}\"")),
    )
    .unwrap();
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "v1"]);
    git(dir.path(), &["tag", "v1.0.0"]);
    dir
}

/// Tag the current HEAD of `repo`.
pub fn tag(repo: &TempDir, name: &str) {
    git(repo.path(), &["tag", name]);
}

pub fn splice_cmd() -> Command {
    let mut c = Command::cargo_bin("splice").unwrap();
    c.env("SPLICE_CACHE_DIR", cache_dir())
        .env("GIT_CONFIG_SYSTEM", empty_git_config())
        .env("GIT_CONFIG_GLOBAL", empty_git_config())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
    c
}

fn walk(base: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.file_name().unwrap() == ".git" {
            continue;
        }
        if p.is_dir() {
            walk(base, &p, out);
        } else {
            out.push(p.strip_prefix(base).unwrap().to_path_buf());
        }
    }
}

/// Sorted `path sha256` lines plus text contents, ignoring `.git`.
pub fn tree_snapshot(dir: &Path) -> String {
    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort();
    let mut s = String::new();
    for f in files {
        let bytes = std::fs::read(dir.join(&f)).unwrap();
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        s.push_str(&format!(
            "{} {hash}\n",
            f.to_string_lossy().replace('\\', "/")
        ));
        if let Ok(t) = std::str::from_utf8(&bytes) {
            s.push_str(t);
            if !t.ends_with('\n') {
                s.push('\n');
            }
        }
    }
    s
}

/// Stage and commit everything in `dir`.
pub fn git_commit_all(dir: &Path, msg: &str) {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// Make `dir` a git work tree (init only, no commit needed).
pub fn git_commit_all_init(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
}
