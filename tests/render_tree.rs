mod common;

use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use splice::manifest::Template;
use splice::output::Class;
use splice::render::{PieceInstance, RenderInput, RenderedTree, render_sections, render_tree};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/template/v1")
}

fn map(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// `"base"` or `"app:web"` (id:name). An app's `name` var defaults to `web`.
fn input(pieces: &[&str], vars: &[(&str, &str)]) -> RenderInput {
    let mut template_vars = map(&[("scope", "acme")]);
    template_vars.extend(map(vars));
    let instances = pieces
        .iter()
        .map(|p| {
            let (id, name) = p.split_once(':').unwrap_or((p, p));
            PieceInstance {
                id: id.into(),
                name: name.into(),
                vars: if id == "app" {
                    map(&[("name", name)])
                } else {
                    IndexMap::new()
                },
            }
        })
        .collect();
    RenderInput {
        template_vars,
        instances,
        version: "1.0.0".parse().unwrap(),
    }
}

fn render_in(
    root: &Path,
    pieces: &[&str],
    vars: &[(&str, &str)],
) -> Result<RenderedTree, splice::error::SpliceError> {
    render_tree(&Template::load(root).unwrap(), &input(pieces, vars))
}

fn render_fixture(pieces: &[&str], vars: &[(&str, &str)]) -> RenderedTree {
    render_in(&fixture_root(), pieces, vars).unwrap()
}

fn tree_listing(tree: &RenderedTree) -> String {
    tree.values()
        .map(|f| {
            format!(
                "{}  {}  {:?}  {}:{}  {}  {} bytes\n",
                f.path,
                f.class.as_str(),
                f.strategy,
                f.piece_id,
                f.piece_name,
                f.glob.as_deref().unwrap_or("-"),
                f.bytes.len()
            )
        })
        .collect()
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

fn temp_template() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    copy_dir(&fixture_root(), dir.path());
    dir
}

#[test]
fn renders_fixture() {
    let tree = render_fixture(&["base", "app:web", "app:admin"], &[("project", "demo")]);
    assert_eq!(tree["apps/web/package.json"].class, Class::Shared);
    assert_eq!(tree["apps/web/src/main.ts"].class, Class::Seed);
    assert!(tree.contains_key("demo.code-workspace"));
    assert!(tree.contains_key("only-with-app.txt"));
    assert_eq!(
        tree["logo.png"].bytes,
        std::fs::read(fixture_root().join("pieces/base/files/logo.png")).unwrap()
    );
    assert_eq!(
        std::str::from_utf8(&tree["apps.txt"].bytes).unwrap(),
        "pieces:\n- base\n- app\n\n"
    );
    insta::assert_snapshot!(tree_listing(&tree));
}

#[test]
fn empty_render_not_written() {
    let tree = render_fixture(&["base"], &[("project", "demo")]);
    assert!(!tree.contains_key("only-with-app.txt"));
}

#[test]
fn same_path_twice_is_error() {
    let err = render_in(
        &fixture_root(),
        &["base", "app:web", "app:web"],
        &[("project", "demo")],
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("apps/web/package.json"), "{err}");
}

#[test]
fn binary_shared_is_error() {
    let dir = temp_template();
    let toml = dir.path().join("pieces/base/piece.toml");
    let text = std::fs::read_to_string(&toml).unwrap();
    std::fs::write(
        &toml,
        text.replace(
            r#"owned = [".editorconfig", "tool.config.mts", "logo.png", "old.txt"]"#,
            r#"owned = [".editorconfig", "tool.config.mts", "old.txt"]"#,
        )
        .replace(
            r#"shared = ["package.json","#,
            r#"shared = ["logo.png", "package.json","#,
        ),
    )
    .unwrap();
    let err = render_in(dir.path(), &["base"], &[("project", "demo")]).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("logo.png"), "{err}");
}

#[test]
fn instance_name_sets_name_var_over_instance_vars() {
    let mut inp = input(&["base", "app:admin"], &[("project", "demo")]);
    inp.instances[1].vars = map(&[("name", "other")]);
    let tree = render_tree(&Template::load(&fixture_root()).unwrap(), &inp).unwrap();
    assert!(tree.contains_key("apps/admin/package.json"));
    assert!(!tree.contains_key("apps/other/package.json"));
    assert_eq!(tree["apps/admin/package.json"].piece_name, "admin");
}

#[test]
fn unsafe_instance_name_is_error_and_nothing_returned() {
    for name in ["../../etc", "a/b", "a\\b"] {
        let mut inp = input(&["base", "app:web"], &[("project", "demo")]);
        inp.instances[1].name = name.into();
        let err = render_tree(&Template::load(&fixture_root()).unwrap(), &inp).unwrap_err();
        assert_eq!(err.exit_code(), 3, "{name}");
    }
}

#[test]
fn unsafe_template_var_in_path_is_error() {
    let err = render_in(&fixture_root(), &["base"], &[("project", "../x")]).unwrap_err();
    assert_eq!(err.exit_code(), 3);
}

#[cfg(unix)]
#[test]
fn symlink_in_template_is_error() {
    let dir = temp_template();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("pieces/base/files/link.txt"))
        .unwrap();
    let err = render_in(dir.path(), &["base"], &[("project", "demo")]).unwrap_err();
    assert_eq!(err.exit_code(), 3);
    assert!(err.to_string().contains("symlink"), "{err}");
}

#[test]
fn jinja_files_use_lf_and_non_jinja_bytes_are_untouched() {
    let dir = temp_template();
    std::fs::write(dir.path().join("pieces/base/files/crlf.txt"), b"a\r\nb\r\n").unwrap();
    std::fs::write(
        dir.path().join("pieces/base/files/j.txt.jinja"),
        b"a\r\n{{ project }}\r\n",
    )
    .unwrap();
    let tree = render_in(dir.path(), &["base"], &[("project", "demo")]).unwrap();
    assert_eq!(tree["crlf.txt"].bytes, b"a\r\nb\r\n");
    assert_eq!(tree["j.txt"].bytes, b"a\ndemo\n");
}

#[test]
fn piece_without_files_dir_renders_nothing() {
    let dir = temp_template();
    let piece = dir.path().join("pieces/hooks");
    std::fs::create_dir_all(&piece).unwrap();
    std::fs::write(
        piece.join("piece.toml"),
        "id = \"hooks\"\ndescription = \"Post only\"\n\n[[post]]\nrun = \"true\"\nwhen = [\"new\"]\n",
    )
    .unwrap();
    assert!(!piece.join("files").exists());
    let with = render_in(dir.path(), &["base", "hooks"], &[("project", "demo")]).unwrap();
    let without = render_in(dir.path(), &["base"], &[("project", "demo")]).unwrap();
    assert_eq!(
        with.keys().collect::<Vec<_>>(),
        without.keys().collect::<Vec<_>>()
    );
}

fn channel_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/channel/v1")
}

fn plain_input(pieces: &[&str]) -> RenderInput {
    RenderInput {
        template_vars: IndexMap::new(),
        instances: pieces
            .iter()
            .map(|p| PieceInstance {
                id: p.to_string(),
                name: p.to_string(),
                vars: IndexMap::new(),
            })
            .collect(),
        version: "1.0.0".parse().unwrap(),
    }
}

#[test]
fn fragment_not_in_tree() {
    let t = Template::load(&channel_root()).unwrap();
    let tree = render_tree(&t, &plain_input(&["core", "claude"])).unwrap();
    assert!(
        tree.keys().all(|k| !k.starts_with("sections/")),
        "{:?}",
        tree.keys()
    );
    assert!(tree.contains_key("docs/agents/rules.md"));
}

#[test]
fn sections_render_with_pieces() {
    let t = Template::load(&channel_root()).unwrap();
    let with = render_sections(&t, &plain_input(&["core", "claude"])).unwrap();
    assert_eq!(with.keys().collect::<Vec<_>>(), ["AGENTS.md", "CLAUDE.md"]);
    assert!(
        with["AGENTS.md"].body.contains(".claude/agents"),
        "{:?}",
        with["AGENTS.md"]
    );
    assert_eq!(with["AGENTS.md"].piece_id, "core");
    assert_eq!(with["CLAUDE.md"].body, "See `AGENTS.md` for the rules.\n");
    let without = render_sections(&t, &plain_input(&["core"])).unwrap();
    assert_eq!(without.keys().collect::<Vec<_>>(), ["AGENTS.md"]);
    assert!(!without["AGENTS.md"].body.contains(".claude/agents"));
}

#[test]
fn section_host_is_relative_to_target() {
    let dir = common::inline_template(&[(
        "p",
        "id = \"p\"\ndescription = \"d\"\ntarget = \"sub/{{ name }}\"\n\n\
         [sections]\n\"AGENTS.md\" = \"s.md\"\n",
        &[("s.md", "hi\n")],
    )]);
    let t = Template::load(dir.path()).unwrap();
    let s = render_sections(&t, &plain_input(&["p"])).unwrap();
    assert_eq!(s.keys().collect::<Vec<_>>(), ["sub/p/AGENTS.md"]);
    assert_eq!(s["sub/p/AGENTS.md"].host, "sub/p/AGENTS.md");
}

#[test]
fn two_pieces_same_host_rejected() {
    let piece = |id: &str| {
        format!("id = \"{id}\"\ndescription = \"d\"\n\n[sections]\n\"AGENTS.md\" = \"s.md\"\n")
    };
    let (a, b) = (piece("a"), piece("b"));
    let dir =
        common::inline_template(&[("a", &a, &[("s.md", "a\n")]), ("b", &b, &[("s.md", "b\n")])]);
    let t = Template::load(dir.path()).unwrap();
    let e = render_sections(&t, &plain_input(&["a", "b"])).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    let m = e.to_string();
    assert!(
        m.contains("AGENTS.md") && m.contains("`a") && m.contains("`b"),
        "{m}"
    );
}

#[test]
fn whitespace_fragment_declares_nothing() {
    let dir = common::inline_template(&[(
        "p",
        "id = \"p\"\ndescription = \"d\"\n\n[sections]\n\"AGENTS.md\" = \"s.md.jinja\"\n",
        &[("s.md.jinja", "{% if \"zzz\" in pieces %}x{% endif %}\n")],
    )]);
    let t = Template::load(dir.path()).unwrap();
    assert!(
        render_sections(&t, &plain_input(&["p"]))
            .unwrap()
            .is_empty()
    );
}
