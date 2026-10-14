use splice::manifest::{MergeStrategy, Template};
use splice::output::Class;
use splice::render::{PieceInstance, RenderInput, RenderedTree, render_tree};
use std::path::Path;

const V1: &str = "tests/fixtures/template/v1";

fn input(specs: &[&str], vars: &[(&str, &str)]) -> RenderInput {
    let mut template_vars: indexmap::IndexMap<String, String> = indexmap::IndexMap::new();
    template_vars.insert("scope".into(), "acme".into());
    for (k, v) in vars {
        template_vars.insert(k.to_string(), v.to_string());
    }
    let instances = specs
        .iter()
        .map(|s| {
            let (id, name) = s.split_once(':').unwrap_or((s, s));
            let mut v = indexmap::IndexMap::new();
            if id == "app" {
                v.insert("name".to_string(), name.to_string());
            }
            PieceInstance {
                id: id.into(),
                name: name.into(),
                vars: v,
            }
        })
        .collect();
    RenderInput {
        template_vars,
        instances,
        version: semver::Version::new(1, 0, 0),
    }
}

fn render_at(root: &Path, specs: &[&str], vars: &[(&str, &str)]) -> splice::error::SpliceError {
    let t = Template::load(root).unwrap();
    render_tree(&t, &input(specs, vars)).unwrap_err()
}

fn render_fixture(specs: &[&str], vars: &[(&str, &str)]) -> RenderedTree {
    let t = Template::load(Path::new(V1)).unwrap();
    render_tree(&t, &input(specs, vars)).unwrap()
}

fn tree_listing(tree: &RenderedTree) -> String {
    tree.values()
        .map(|f| {
            format!(
                "{} | {:?} | {:?} | {}:{} | {:?} | {} bytes\n",
                f.path,
                f.class,
                f.strategy,
                f.piece_id,
                f.piece_name,
                f.glob,
                f.bytes.len()
            )
        })
        .collect()
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

fn temp_template() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    copy_dir(Path::new(V1), d.path());
    d
}

#[test]
fn renders_fixture() {
    let tree = render_fixture(&["base", "app:web", "app:admin"], &[("project", "demo")]);
    assert_eq!(tree["apps/web/package.json"].class, Class::Shared);
    assert_eq!(tree["apps/web/package.json"].strategy, MergeStrategy::Json);
    assert_eq!(tree["apps/web/src/main.ts"].class, Class::Seed);
    assert!(tree.contains_key("demo.code-workspace"));
    assert!(tree.contains_key("only-with-app.txt"));
    assert_eq!(
        tree["logo.png"].bytes,
        std::fs::read("tests/fixtures/template/v1/pieces/base/files/logo.png").unwrap()
    );
    assert_eq!(
        std::str::from_utf8(&tree["apps.txt"].bytes).unwrap(),
        "base\napp\n"
    );
    insta::assert_snapshot!(tree_listing(&tree));
}

#[test]
fn empty_render_not_written() {
    assert!(!render_fixture(&["base"], &[("project", "demo")]).contains_key("only-with-app.txt"));
}

#[test]
fn same_path_twice_is_error() {
    let t = Template::load(Path::new(V1)).unwrap();
    let e = render_tree(
        &t,
        &input(&["base", "app:web", "app:web"], &[("project", "demo")]),
    )
    .unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("apps/web/package.json"), "{e}");
}

#[test]
fn binary_shared_is_error() {
    let d = temp_template();
    let p = d.path().join("pieces/base/piece.toml");
    let s = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        s.replace("\"logo.png\"]", "]")
            .replace("\"notes.txt\"]", "\"notes.txt\", \"logo.png\"]"),
    )
    .unwrap();
    let e = render_at(d.path(), &["base"], &[("project", "demo")]);
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("logo.png"), "{e}");
}

#[test]
fn var_escaping_the_repo_is_error() {
    let t = Template::load(Path::new(V1)).unwrap();
    for bad in ["../../etc", "a/b"] {
        let e = render_tree(&t, &input(&["base"], &[("project", bad)])).unwrap_err();
        assert_eq!(e.exit_code(), 3, "{bad}");
    }
}

#[cfg(unix)]
mod symlinks {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn symlinked_file_under_files_is_rejected() {
        let d = temp_template();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "s3cret").unwrap();
        symlink(
            outside.path().join("secret"),
            d.path().join("pieces/base/files/leak.txt"),
        )
        .unwrap();
        let e = render_at(d.path(), &["base"], &[("project", "demo")]);
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
        assert!(e.to_string().contains("leak.txt"), "{e}");
    }

    #[test]
    fn symlinked_dir_under_files_is_rejected() {
        let d = temp_template();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "s3cret").unwrap();
        symlink(outside.path(), d.path().join("pieces/base/files/linkdir")).unwrap();
        let e = render_at(d.path(), &["base"], &[("project", "demo")]);
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
    }

    #[test]
    fn symlinked_files_dir_is_rejected() {
        let d = temp_template();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "s3cret").unwrap();
        let files = d.path().join("pieces/base/files");
        std::fs::remove_dir_all(&files).unwrap();
        symlink(outside.path(), &files).unwrap();
        let e = render_at(d.path(), &["base"], &[("project", "demo")]);
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
    }
}
