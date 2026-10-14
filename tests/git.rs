mod common;
use common::template_repo;
use splice::git::Git;
use tempfile::tempdir;

#[test]
fn lists_tags_and_extracts_tree() {
    let repo = template_repo(&["v1", "v2"]);
    let tags = Git::ls_remote_tags(repo.path().to_str().unwrap()).unwrap();
    assert_eq!(
        tags.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
        ["v1.0.0", "v2.0.0"]
    );
    let dest = tempdir().unwrap().path().join("t");
    Git::extract_tree(&repo.path().join(".git"), &tags[0].1, &dest).unwrap();
    assert!(dest.join("splice.toml").exists());
}

#[test]
fn option_like_args_are_not_interpreted_by_git() {
    let marker = tempdir().unwrap();
    let m = marker.path().join("pwned");
    let url = format!("--upload-pack=touch {}", m.display());
    assert!(Git::ls_remote_tags(&url).is_err());
    let mirror = tempdir().unwrap().path().join("m.git");
    assert!(Git::ensure_mirror(&url, &mirror).is_err());
    assert!(!m.exists());
    let repo = template_repo(&["v1"]);
    let gd = repo.path().join(".git");
    assert!(Git::rev_parse(&gd, "--all").is_err());
    let dest = tempdir().unwrap().path().join("t");
    assert!(Git::extract_tree(&gd, "--help", &dest).is_err());
}
