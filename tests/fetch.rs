mod common;
use common::{tag, template_repo};
use semver::Version;
use splice::fetch::Fetcher;
use splice::source::TemplateSource;
use tempfile::tempdir;

#[test]
fn latest_stable_and_cache_reuse() {
    let repo = template_repo(&["v1", "v2"]);
    let cache = tempdir().unwrap();
    let f = Fetcher::new(cache.path().into());
    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    let r = f.resolve(&src, None, false).unwrap();
    assert_eq!(r.version, Version::new(2, 0, 0));
    assert!(r.root.starts_with(cache.path().join("trees")));
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap(); // source gone
    assert!(f.checkout(&src, &r.commit, r.version.clone()).is_ok()); // served from cache
}

#[test]
fn ref_and_prerelease() {
    let repo = template_repo(&["v1"]);
    tag(&repo, "v1.1.0-rc.1");
    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    let f = Fetcher::new(tempdir().unwrap().keep());
    assert_eq!(
        f.resolve(&src, None, false).unwrap().version,
        Version::parse("1.0.0").unwrap()
    );
    assert_eq!(
        f.resolve(&src, None, true).unwrap().version,
        Version::parse("1.1.0-rc.1").unwrap()
    );
    assert!(
        f.resolve(&src, Some("main"), false)
            .unwrap()
            .version
            .to_string()
            .starts_with("0.0.0+g")
    );
    assert_eq!(
        f.available_versions(&src, true).unwrap(),
        vec![Version::new(1, 0, 0), Version::parse("1.1.0-rc.1").unwrap()]
    );
    assert_eq!(
        f.resolve_version(&src, &Version::new(1, 0, 0))
            .unwrap()
            .version,
        Version::new(1, 0, 0)
    );
}

#[test]
fn missing_subdir_or_commit_is_template_error() {
    let repo = template_repo(&["v1"]);
    let f = Fetcher::new(tempdir().unwrap().keep());
    let spec = format!("{}//nope", repo.path().to_str().unwrap());
    let src = TemplateSource::parse(&spec).unwrap();
    let e = f.resolve(&src, None, false).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("nope"), "{e}");

    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let e = f.checkout(&src, sha, Version::new(1, 0, 0)).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains(sha), "{e}");
}

#[test]
fn ref_tag_and_non_semver_tags() {
    let repo = template_repo(&["v1"]);
    for t in ["latest", "1.0.0", "vfoo"] {
        tag(&repo, t);
    }
    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    let f = Fetcher::new(tempdir().unwrap().keep());
    assert_eq!(
        f.resolve(&src, Some("v1.0.0"), false).unwrap().version,
        Version::new(1, 0, 0)
    );
    assert_eq!(
        f.available_versions(&src, true).unwrap(),
        vec![Version::new(1, 0, 0)]
    );
    let v = f.resolve(&src, Some("latest"), false).unwrap().version;
    assert!(v.to_string().starts_with("0.0.0+g"), "{v}");
}

#[test]
fn checkout_rejects_non_commit_ids() {
    let repo = template_repo(&["v1"]);
    let f = Fetcher::new(tempdir().unwrap().keep());
    let src = TemplateSource::parse(repo.path().to_str().unwrap()).unwrap();
    for bad in [
        "../x",
        "abcdef1",
        "ABCDEF0123456789ABCDEF0123456789ABCDEF01",
    ] {
        let e = f.checkout(&src, bad, Version::new(1, 0, 0)).unwrap_err();
        assert_eq!(e.exit_code(), 3, "{bad}");
    }
}

#[cfg(unix)]
#[test]
fn symlinked_subdir_escaping_the_tree_is_rejected() {
    let repo = template_repo(&["v1"]);
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("splice.toml"), "format = 1\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), repo.path().join("link")).unwrap();
    common::git_commit_all(repo.path(), "add link");
    let cache = tempdir().unwrap();
    let f = Fetcher::new(cache.path().into());
    let spec = format!("{}//link", repo.path().to_str().unwrap());
    let src = TemplateSource::parse(&spec).unwrap();
    let e = f.resolve(&src, Some("main"), false).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("symlink"), "{e}");
}
