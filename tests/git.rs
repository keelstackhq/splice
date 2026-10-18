mod common;

use common::{template_repo, tree_snapshot};
use splice::git::Git;
use tempfile::tempdir;

#[test]
fn lists_tags_and_extracts_tree() {
    let repo = template_repo(&["v1"]);
    let tags = Git::ls_remote_tags(repo.path().to_str().unwrap()).unwrap();
    assert_eq!(
        tags.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["v1.0.0"]
    );
    let dest = tempdir().unwrap().path().join("t");
    Git::extract_tree(&repo.path().join(".git"), &tags[0].1, &dest).unwrap();
    assert!(dest.join("splice.toml").exists());
    assert!(dest.join("pieces/base/piece.toml").exists());
    assert!(!dest.join(".git").exists());
}

#[test]
fn mirror_clone_then_fetch_and_rev_parse() {
    let repo = template_repo(&["v1"]);
    let url = repo.path().to_str().unwrap();
    let cache = tempdir().unwrap();
    let mirror = cache.path().join("git/abc.git");
    Git::ensure_mirror(url, &mirror).unwrap();
    let sha = Git::rev_parse(&mirror, "v1.0.0").unwrap();
    assert_eq!(sha.len(), 40);
    // second call takes the fetch path and still works
    Git::ensure_mirror(url, &mirror).unwrap();
    assert_eq!(Git::rev_parse(&mirror, "v1.0.0").unwrap(), sha);
    assert!(Git::rev_parse(&mirror, "nope").is_err());
}

#[test]
fn ls_remote_failure_is_env_error_exit_3() {
    let missing = tempdir().unwrap();
    let err = Git::ls_remote_tags(missing.path().join("none").to_str().unwrap()).unwrap_err();
    assert_eq!(err.exit_code(), 3);
}

#[test]
fn init_and_is_clean() {
    let plain = tempdir().unwrap();
    assert_eq!(Git::is_clean(plain.path()).unwrap(), None);
    Git::init(plain.path()).unwrap();
    assert_eq!(Git::is_clean(plain.path()).unwrap(), Some(true));
    std::fs::write(plain.path().join("x"), "1").unwrap();
    assert_eq!(Git::is_clean(plain.path()).unwrap(), Some(false));
}

#[test]
fn template_repo_is_reproducible() {
    let a = template_repo(&["v1"]);
    let b = template_repo(&["v1"]);
    assert_eq!(tree_snapshot(a.path()), tree_snapshot(b.path()));
    let head = |p: &std::path::Path| Git::rev_parse(&p.join(".git"), "HEAD").unwrap();
    assert_eq!(head(a.path()), head(b.path()));
}
