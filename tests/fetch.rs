mod common;

use common::template_repo;
use semver::Version;
use splice::error::SpliceError;
use splice::fetch::Fetcher;
use splice::source::TemplateSource;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn source(repo: &Path) -> TemplateSource {
    TemplateSource::parse(repo.to_str().unwrap()).unwrap()
}

#[test]
fn latest_stable_and_cache_reuse() {
    // Only fixture v1 exists: add a v2.0.0 tag on a new commit inside the test.
    let repo = template_repo(&["v1"]);
    git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "v2"]);
    git(repo.path(), &["tag", "v2.0.0"]);
    let cache = tempdir().unwrap();
    let f = Fetcher::new(cache.path().into());
    let src = source(repo.path());
    let r = f.resolve(&src, None, false).unwrap();
    assert_eq!(r.version, Version::new(2, 0, 0));
    assert!(r.root.starts_with(cache.path().join("trees")));
    assert!(r.root.join("splice.toml").exists());
    assert_eq!(
        f.available_versions(&src, false).unwrap(),
        vec![Version::new(1, 0, 0), Version::new(2, 0, 0)]
    );
    let old = f.resolve_version(&src, &Version::new(1, 0, 0)).unwrap();
    assert_eq!(old.version, Version::new(1, 0, 0));
    assert_ne!(old.commit, r.commit);
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap(); // source gone
    let again = f.checkout(&src, &r.commit, r.version.clone()).unwrap(); // served from cache
    assert_eq!(again.root, r.root);
}

#[test]
fn existing_tree_dir_is_a_hit() {
    let repo = template_repo(&["v1"]);
    let f = Fetcher::new(tempdir().unwrap().keep());
    let src = source(repo.path());
    let a = f.resolve(&src, None, false).unwrap();
    let b = f.checkout(&src, &a.commit, a.version.clone()).unwrap();
    assert_eq!(a.root, b.root);
}

#[test]
fn ref_and_prerelease() {
    let repo = template_repo(&["v1"]);
    git(repo.path(), &["tag", "v1.1.0-rc.1"]);
    let src = source(repo.path());
    let f = Fetcher::new(tempdir().unwrap().keep());
    assert_eq!(
        f.resolve(&src, None, false).unwrap().version,
        Version::parse("1.0.0").unwrap()
    );
    assert_eq!(
        f.resolve(&src, None, true).unwrap().version,
        Version::parse("1.1.0-rc.1").unwrap()
    );
    assert_eq!(
        f.resolve(&src, Some("v1.0.0"), false).unwrap().version,
        Version::new(1, 0, 0)
    );
    let v = f.resolve(&src, Some("main"), false).unwrap().version;
    assert!(v.to_string().starts_with("0.0.0+g"), "{v}");
}

#[test]
fn missing_subdir_or_commit_is_template_error() {
    let repo = template_repo(&["v1"]);
    let f = Fetcher::new(tempdir().unwrap().keep());
    let spec = format!("{}//nope", repo.path().display());
    let src = TemplateSource::parse(&spec).unwrap();
    let err = f.resolve(&src, None, false).unwrap_err();
    assert!(matches!(err, SpliceError::Template(_)), "{err:?}");
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("nope"), "{err}");

    let sha = "0123456789abcdef0123456789abcdef01234567";
    let err = f
        .checkout(&source(repo.path()), sha, Version::new(1, 0, 0))
        .unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains(sha), "{err}");
}

#[test]
fn no_version_tags_is_template_error() {
    let repo = template_repo(&[]);
    git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "x"]);
    let f = Fetcher::new(tempdir().unwrap().keep());
    let err = f.resolve(&source(repo.path()), None, false).unwrap_err();
    assert!(matches!(err, SpliceError::Template(_)), "{err:?}");
}
