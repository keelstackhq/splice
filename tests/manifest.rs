use splice::manifest::{MergeStrategy, Template, When};
use std::path::Path;

#[test]
fn loads_fixture() {
    let t = Template::load(Path::new("tests/fixtures/template/v1")).unwrap();
    assert_eq!(
        (t.manifest.name.as_str(), t.manifest.base.as_str()),
        ("fixture", "base")
    );
    assert_eq!(t.pieces.keys().collect::<Vec<_>>(), ["app", "base", "lib"]);
    assert!(t.pieces["app"].multiple);
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    assert_eq!(t.manifest.vars["scope"].default.as_deref(), Some("acme"));
    assert!(t.manifest.vars["project"].default.is_none());
    let base = &t.pieces["base"];
    assert_eq!(base.merge["config/settings.toml"], MergeStrategy::Toml);
    assert_eq!(base.post[0].when, [When::New, When::Update]);
    assert_eq!(t.pieces["app"].target.as_deref(), Some("apps/{{ name }}"));
    assert_eq!(t.pieces["lib"].requires, ["app"]);
    assert!(base.files_dir.ends_with("pieces/base/files"));
}

#[cfg(unix)]
mod symlinks {
    use super::*;
    use std::os::unix::fs::symlink;

    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }

    fn tpl() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        copy(Path::new("tests/fixtures/template/v1"), d.path());
        d
    }

    fn assert_rejected(d: &tempfile::TempDir) {
        let e = Template::load(d.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
    }

    #[test]
    fn symlinked_manifests_and_dirs_are_rejected() {
        let out = tempfile::tempdir().unwrap();
        std::fs::write(out.path().join("x.toml"), "").unwrap();
        std::fs::create_dir(out.path().join("d")).unwrap();

        let d = tpl();
        let p = d.path().join("splice.toml");
        std::fs::rename(&p, out.path().join("s.toml")).unwrap();
        symlink(out.path().join("s.toml"), &p).unwrap();
        assert_rejected(&d);

        let d = tpl();
        let p = d.path().join("pieces/app/piece.toml");
        let orig = std::fs::read(&p).unwrap();
        std::fs::write(out.path().join("p.toml"), orig).unwrap();
        std::fs::remove_file(&p).unwrap();
        symlink(out.path().join("p.toml"), &p).unwrap();
        assert_rejected(&d);

        let d = tpl();
        symlink(out.path().join("d"), d.path().join("pieces/evil")).unwrap();
        assert_rejected(&d);

        let d = tpl();
        std::fs::create_dir(d.path().join("migrations")).unwrap();
        symlink(
            out.path().join("x.toml"),
            d.path().join("migrations/1.0.0.toml"),
        )
        .unwrap();
        assert_rejected(&d);
    }
}
