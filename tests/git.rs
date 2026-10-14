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
