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
pub fn empty_git_config() -> PathBuf {
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
        // `-c core.autocrlf=false` for every git a test runs, also one started by `sh -c`.
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.autocrlf")
        .env("GIT_CONFIG_VALUE_0", "false")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date);
    for (k, v) in AUTHOR {
        cmd.env(format!("GIT_AUTHOR_{k}"), v)
            .env(format!("GIT_COMMITTER_{k}"), v);
    }
}

pub fn git(dir: &Path, date: &str, args: &[&str]) {
    let mut cmd = Proc::new("git");
    cmd.current_dir(dir)
        .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf"])
        .args(args);
    apply_git_env(&mut cmd, date);
    let out = cmd.output().expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Run `script` with `sh -c` in `dir` under the same git environment as [`git`] (the restore
/// command an error message tells the user to run).
pub fn sh_with_git_env(dir: &Path, script: &str) -> std::process::Output {
    let mut cmd = Proc::new("sh");
    cmd.current_dir(dir).args(["-c", script]);
    apply_git_env(&mut cmd, STATE_DATE);
    cmd.output().expect("run sh")
}

/// Stage the whole work tree, rebuilding the index from file contents.
///
/// A plain `git add -A` trusts the cached stat data (size and mtime). A same-size edit made within
/// the filesystem's timestamp granularity (seen on Windows CI: `indent_size = 2` to `4`) can then
/// be missed, and the commit silently keeps the old bytes. Dropping the index entries first forces
/// every file to be hashed again.
pub fn stage_all(dir: &Path, date: &str) {
    git(
        dir,
        date,
        &["rm", "-r", "-q", "--cached", "--ignore-unmatch", "."],
    );
    git(dir, date, &["add", "-A"]);
}

/// Assert HEAD's tree is byte for byte the fixture directory (blob ids, no filters).
fn assert_head_matches(repo: &Path, fixture: &Path) {
    use std::io::Write;
    use std::process::Stdio;
    let mut files = Vec::new();
    collect(fixture, fixture, &mut files);
    let mut rels: Vec<String> = files
        .iter()
        .map(|f| f.to_string_lossy().replace('\\', "/"))
        .collect();
    rels.sort();
    let mut stdin = String::new();
    for r in &rels {
        stdin.push_str(&format!("{}\n", fixture.join(r).display()));
    }
    let mut hash = Proc::new("git");
    apply_git_env(&mut hash, STATE_DATE);
    let mut child = hash
        .current_dir(repo)
        .args(["hash-object", "--no-filters", "--stdin-paths"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "git hash-object failed");
    let hashes = String::from_utf8(out.stdout).unwrap();
    let want: Vec<String> = rels
        .iter()
        .zip(hashes.lines())
        .map(|(r, h)| format!("{r} {h}"))
        .collect();
    let tree = git_stdout(repo, &["ls-tree", "-r", "HEAD"]);
    let mut got: Vec<String> = tree
        .lines()
        .map(|l| {
            let (meta, path) = l.split_once('\t').unwrap();
            let id = meta.split(' ').nth(2).unwrap();
            format!("{path} {id}")
        })
        .collect();
    got.sort();
    assert_eq!(got, want, "commit tree differs from fixture {fixture:?}");
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
        stage_all(repo.path(), &date);
        git(repo.path(), &date, &["commit", "-q", "-m", version]);
        assert_head_matches(repo.path(), &fixtures.join(version));
        git(repo.path(), &date, &["tag", tag]);
    }
    repo
}

/// Commit the working tree of a `template_repo` as a new commit and tag it `tag`.
pub fn commit_tag(repo: &Path, msg: &str, tag: &str) {
    let date = "2026-02-01T00:00:00+00:00";
    stage_all(repo, date);
    git(repo, date, &["commit", "-q", "-m", msg]);
    git(repo, date, &["tag", tag]);
}

/// Fold the working tree into the tip commit and move `tag` to it, so the latest version stays
/// the one tag the test already knows.
pub fn amend_tip(repo: &Path, tag: &str) {
    let date = "2026-01-01T00:00:00+00:00";
    stage_all(repo, date);
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

const STATE_DATE: &str = "2026-03-01T00:00:00+00:00";

/// Commit the whole working tree as "state" (also when nothing changed); `git init` first when
/// `repo` is not a repository yet.
pub fn commit_all(repo: &Path) {
    if !repo.join(".git").exists() {
        git(repo, STATE_DATE, &["init", "-q", "-b", "main"]);
    }
    stage_all(repo, STATE_DATE);
    git(
        repo,
        STATE_DATE,
        &["commit", "-q", "--allow-empty", "-m", "state"],
    );
}

pub fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Proc::new("git");
    cmd.current_dir(dir).args(args);
    apply_git_env(&mut cmd, STATE_DATE);
    let out = cmd.output().expect("run git");
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// `splice new` of the main fixture (app piece `web`) at `tag` into a fresh, committed repo.
pub fn new_repo_at(tpl: &TempDir, tag: &str) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    splice_cmd()
        .args([
            "new",
            tpl.path().to_str().unwrap(),
            repo.path().to_str().unwrap(),
        ])
        .args(["--ref", tag])
        .args([
            "--piece",
            "app:web",
            "--data",
            "project=demo",
            "--yes",
            "--json",
        ])
        .assert()
        .success();
    commit_all(repo.path());
    repo
}

/// An empty git repo with one commit (`README.md`) and no lock.
pub fn bare_repo() -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), STATE_DATE, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.path().join("README.md"), "# bare\n").unwrap();
    commit_all(repo.path());
    repo
}

/// A git repo whose `channel/` subdir holds `tests/fixtures/<fixture>/<dir>` for each
/// `(dir, tag)` in turn (each committed on `main` and tagged), next to a top-level `README.md`.
/// Use `format!("{}//channel", repo.path().display())` as the source spec.
pub fn channel_repo_tagged(fixture: &str, versions: &[(&str, &str)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    git(
        repo.path(),
        "2026-01-01T00:00:00+00:00",
        &["init", "-q", "-b", "main"],
    );
    for (i, (dir, tag)) in versions.iter().enumerate() {
        let date = format!("2026-01-{:02}T00:00:00+00:00", i + 1);
        let sub = repo.path().join("channel");
        if sub.exists() {
            std::fs::remove_dir_all(&sub).unwrap();
        }
        copy_dir(&fixtures.join(dir), &sub);
        std::fs::write(repo.path().join("README.md"), "# channel repo\n").unwrap();
        stage_all(repo.path(), &date);
        git(repo.path(), &date, &["commit", "-q", "-m", dir]);
        git(repo.path(), &date, &["tag", tag]);
    }
    repo
}

/// [`channel_repo_tagged`] for the `channel` fixture: `"v1"` is tagged `v1.0.0`.
pub fn channel_repo(versions: &[&str]) -> TempDir {
    let tags: Vec<String> = versions.iter().map(|v| format!("{v}.0.0")).collect();
    let pairs: Vec<(&str, &str)> = versions
        .iter()
        .zip(&tags)
        .map(|(v, t)| (*v, t.as_str()))
        .collect();
    channel_repo_tagged("channel", &pairs)
}

/// A new commit on `main` (channel fixture only): inside the channel subdir (`inside`) or only `README.md` outside it.
/// Returns the full sha.
pub fn track_commit(repo: &Path, inside: bool) -> String {
    let n = git_stdout(repo, &["rev-list", "--count", "HEAD"]);
    let (rel, msg) = if inside {
        ("channel/pieces/core/files/docs/agents/rules.md", "inside")
    } else {
        ("README.md", "outside")
    };
    let path = repo.join(rel);
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(&format!("change {n}\n"));
    std::fs::write(&path, text).unwrap();
    let date = "2026-02-01T00:00:00+00:00";
    stage_all(repo, date);
    git(repo, date, &["commit", "-q", "-m", msg]);
    git_stdout(repo, &["rev-parse", "HEAD"])
}

/// `splice -C repo --yes --json <args>`; no commit afterwards.
pub fn chan(repo: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    splice_cmd()
        .arg("-C")
        .arg(repo)
        .args(["--yes", "--json"])
        .args(args)
        .assert()
}

/// [`chan`], asserts exit 0, then commits the tree so the next command sees a clean tree.
pub fn chan_commit(repo: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let out = chan(repo, args).success();
    commit_all(repo);
    out
}

/// `(path under files/, content)`.
pub type FileSpec<'a> = &'a [(&'a str, &'a str)];

/// A template dir made of `(piece id, piece.toml, [(path under files/, content)])`; the first
/// piece is the base. Template name `t`, no template vars.
pub fn inline_template(pieces: &[(&str, &str, FileSpec)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = pieces[0].0;
    std::fs::write(
        dir.path().join("splice.toml"),
        format!("format = 1\nname = \"t\"\nmin_splice = \"0.1.0\"\nbase = \"{base}\"\n"),
    )
    .unwrap();
    for (id, toml, files) in pieces {
        let p = dir.path().join("pieces").join(id);
        std::fs::create_dir_all(p.join("files")).unwrap();
        std::fs::write(p.join("piece.toml"), toml).unwrap();
        for (rel, text) in *files {
            let f = p.join("files").join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
    }
    dir
}

pub const USER_README: &str = "# demo\n\nMy own words, edited by the user.\n";

pub fn edit(repo: &TempDir, rel: &str, f: impl FnOnce(String) -> String) {
    let path = repo.path().join(rel);
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, f(text)).unwrap();
}

/// The "User edit" column of the plan's fixture table (never the owned tsconfig: owned drift
/// refuses the update, covered in cli_update).
pub fn apply_user_edits(repo: &TempDir) {
    edit(repo, "package.json", |t| {
        t.replace(
            "  \"private\": true,\n",
            "  \"private\": true,\n  \"dependencies\": {\n    \"left-pad\": \"1.3.0\"\n  },\n",
        )
    });
    edit(repo, "config/settings.toml", |t| {
        format!("# local note\n{t}\n[local]\nx = 1\n")
    });
    edit(repo, ".github/workflows/ci.yml", |t| {
        t.replace("ubuntu-latest", "macos-latest")
    });
    edit(repo, "notes.txt", |t| {
        t.replace("line five", "line 5, mine")
    });
    std::fs::write(repo.path().join("README.md"), USER_README).unwrap();
    commit_all(repo.path());
}

/// Hide what depends on the machine: the lock's source path, commit and splice version.
pub fn redact(snapshot: &str) -> String {
    let mut out = snapshot.to_string();
    for (re, to) in [
        // the lock writer uses a literal ('...') string for Windows paths with backslashes
        (r#"source = (".*"|'.*')"#, r#"source = "[source]""#),
        (r#"commit = ".*""#, r#"commit = "[commit]""#),
        (r#"splice = ".*""#, r#"splice = "[splice]""#),
        (
            r"\.splice/(lock|channels/[a-z-]+)\.toml [0-9a-f]{64}",
            ".splice/$1.toml [sha]",
        ),
    ] {
        out = regex::Regex::new(re)
            .unwrap()
            .replace_all(&out, to)
            .into_owned();
    }
    out
}
