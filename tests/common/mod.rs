#![allow(dead_code)]
use assert_cmd::Command;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command as Proc;
use tempfile::TempDir;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/template")
}

fn empty_git_config() -> PathBuf {
    let p = std::env::temp_dir().join("splice-test-gitconfig");
    if !p.exists() {
        let _ = std::fs::write(&p, "");
    }
    p
}

fn set_git_env(c: &mut Proc) {
    c.env("GIT_CONFIG_GLOBAL", empty_git_config())
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .env("GIT_TERMINAL_PROMPT", "0");
}

fn git(dir: &Path, args: &[&str]) {
    let mut c = Proc::new("git");
    set_git_env(&mut c);
    let out = c
        .current_dir(dir)
        .args(["-c", "core.autocrlf=false"])
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

pub fn splice_cmd() -> Command {
    let mut c = Command::cargo_bin("splice").unwrap();
    let cache = std::env::temp_dir().join("splice-test-cache");
    c.env("SPLICE_CACHE_DIR", cache)
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
