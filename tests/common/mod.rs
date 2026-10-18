//! Shared helpers for the CLI integration tests.
#![allow(dead_code)] // each test binary uses a different subset

use assert_cmd::Command;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command as Proc;
use tempfile::TempDir;

const AUTHOR: [(&str, &str); 2] = [("NAME", "Test"), ("EMAIL", "test@example.com")];

/// An empty file used as `GIT_CONFIG_GLOBAL`, so the user's git config never leaks in.
fn empty_git_config() -> PathBuf {
    let p = std::env::temp_dir().join("splice-test-empty-gitconfig");
    if !p.exists() {
        let _ = std::fs::write(&p, "");
    }
    p
}

fn apply_git_env(cmd: &mut Proc, date: &str) {
    cmd.env("GIT_CONFIG_GLOBAL", empty_git_config())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date);
    for (k, v) in AUTHOR {
        cmd.env(format!("GIT_AUTHOR_{k}"), v)
            .env(format!("GIT_COMMITTER_{k}"), v);
    }
}

pub fn git(dir: &Path, date: &str, args: &[&str]) {
    let mut cmd = Proc::new("git");
    cmd.current_dir(dir).args(args);
    apply_git_env(&mut cmd, date);
    let out = cmd.output().expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// A git repo holding the fixture template. For each name in `versions` (for example `"v1"`),
/// the repo content is replaced by `tests/fixtures/template/<name>`, committed on `main` and
/// tagged `v<semver>`: `v1` becomes `v1.0.0`. Commit dates are fixed, one day apart.
pub fn template_repo(versions: &[&str]) -> TempDir {
    let tags: Vec<String> = versions.iter().map(|v| format!("{v}.0.0")).collect();
    let pairs: Vec<(&str, &str)> = versions
        .iter()
        .zip(&tags)
        .map(|(v, t)| (*v, t.as_str()))
        .collect();
    template_repo_tagged(&pairs)
}

/// Like [`template_repo`], with explicit `(fixture dir, tag)` pairs, for example
/// `("v2-nolib", "v2.0.0")`.
pub fn template_repo_tagged(versions: &[(&str, &str)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/template");
    let first = "2026-01-01T00:00:00+00:00";
    git(repo.path(), first, &["init", "-q", "-b", "main"]);
    for (i, (version, tag)) in versions.iter().enumerate() {
        let date = format!("2026-01-{:02}T00:00:00+00:00", i + 1);
        for entry in std::fs::read_dir(repo.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            if path.is_dir() {
                std::fs::remove_dir_all(&path).unwrap();
            } else {
                std::fs::remove_file(&path).unwrap();
            }
        }
        copy_dir(&fixtures.join(version), repo.path());
        git(repo.path(), &date, &["add", "-A"]);
        git(repo.path(), &date, &["commit", "-q", "-m", version]);
        git(repo.path(), &date, &["tag", tag]);
    }
    repo
}

/// Commit the working tree of a `template_repo` as a new commit and tag it `tag`.
pub fn commit_tag(repo: &Path, msg: &str, tag: &str) {
    let date = "2026-02-01T00:00:00+00:00";
    git(repo, date, &["add", "-A"]);
    git(repo, date, &["commit", "-q", "-m", msg]);
    git(repo, date, &["tag", tag]);
}

/// Fold the working tree into the tip commit and move `tag` to it, so the latest version stays
/// the one tag the test already knows.
pub fn amend_tip(repo: &Path, tag: &str) {
    let date = "2026-01-01T00:00:00+00:00";
    git(repo, date, &["add", "-A"]);
    git(repo, date, &["commit", "-q", "--amend", "--no-edit"]);
    git(repo, date, &["tag", "-f", tag]);
}

/// The `splice` binary under test, with an isolated cache and git identity.
pub fn splice_cmd() -> Command {
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("splice");
    let cache = std::env::temp_dir().join(format!("splice-test-cache-{}", std::process::id()));
    cmd.env("SPLICE_CACHE_DIR", cache)
        .env("GIT_CONFIG_GLOBAL", empty_git_config())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    for (k, v) in AUTHOR {
        cmd.env(format!("GIT_AUTHOR_{k}"), v)
            .env(format!("GIT_COMMITTER_{k}"), v);
    }
    cmd
}

fn collect(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if path.is_dir() {
            collect(&path, root, out);
        } else {
            out.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

/// Sorted `path sha256` lines, then the text content of each file, ignoring `.git`.
pub fn tree_snapshot(dir: &Path) -> String {
    let mut files = Vec::new();
    collect(dir, dir, &mut files);
    files.sort();
    let mut lines = String::new();
    let mut texts = String::new();
    for rel in files {
        let bytes = std::fs::read(dir.join(&rel)).unwrap();
        let name = rel.to_string_lossy().replace('\\', "/");
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let _ = writeln!(lines, "{name} {hash}");
        if let Ok(text) = String::from_utf8(bytes) {
            let _ = write!(texts, "== {name} ==\n{text}");
            if !text.ends_with('\n') {
                texts.push('\n');
            }
        }
    }
    format!("{lines}{texts}")
}
