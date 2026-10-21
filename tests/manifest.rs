mod common;

use std::path::Path;

use common::inline_template;
use splice::manifest::{MergeStrategy, Template, When};

fn fixture() -> Template {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/template/v1");
    Template::load(&root).unwrap()
}

#[test]
fn loads_fixture() {
    let t = fixture();
    assert_eq!(
        (t.manifest.name.as_str(), t.manifest.base.as_str()),
        ("fixture", "base")
    );
    assert_eq!(t.pieces.keys().collect::<Vec<_>>(), ["app", "base", "lib"]);
    assert!(t.pieces["app"].multiple);
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn fixture_covers_the_contract_features() {
    let t = fixture();
    let (base, app, lib) = (&t.pieces["base"], &t.pieces["app"], &t.pieces["lib"]);
    assert!(!base.multiple && !lib.multiple);
    assert_eq!(t.manifest.vars["scope"].default.as_deref(), Some("acme"));
    assert!(t.manifest.vars["project"].default.is_none());
    assert_eq!(app.requires, ["base"]);
    assert_eq!(lib.requires, ["app"]);
    assert_eq!(app.target.as_deref(), Some("apps/{{ name }}"));
    let name = &app.vars["name"];
    assert_eq!(name.default.as_deref(), Some("web"));
    assert!(name.pattern.as_ref().unwrap().is_match("my-app"));
    assert!(!name.pattern.as_ref().unwrap().is_match("My"));
    assert_eq!(base.merge["package.json"], MergeStrategy::Json);
    assert_eq!(base.merge["config/settings.toml"], MergeStrategy::Toml);
    assert_eq!(base.merge[".github/workflows/ci.yml"], MergeStrategy::Yaml);
    assert_eq!(base.merge["notes.txt"], MergeStrategy::Text);
    assert_eq!(base.post[0].when, [When::New, When::Update]);
    assert!(base.files_dir.join("logo.png").is_file());
}

fn fixture_v(name: &str) -> Template {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/template")
        .join(name);
    Template::load(&root).unwrap()
}

#[test]
fn fixture_v2_loads_with_migration() {
    let t = fixture_v("v2");
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    assert_eq!(t.migrations.len(), 1);
    let m = &t.migrations[0];
    assert_eq!(m.version.to_string(), "2.0.0");
    assert_eq!(m.removes[0].path, "tool.config.mts");
    assert!(m.steps[0].run.contains("SPLICE_FIXTURE_FAIL"));
    assert!(fixture().migrations.is_empty());
}

#[test]
fn fixture_v3_needs_a_newer_splice() {
    let t = fixture_v("v3");
    assert_eq!(t.manifest.min_splice.to_string(), "99.0.0");
    assert!(
        t.check_min_splice(&semver::Version::parse("0.1.0").unwrap())
            .is_err()
    );
}

#[test]
fn fixture_v2_without_lib_has_no_lib_piece() {
    let t = fixture_v("v2-nolib");
    assert_eq!(t.pieces.keys().collect::<Vec<_>>(), ["app", "base"]);
}

fn channel_fixture(group: &str, name: &str) -> Template {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(group)
        .join(name);
    Template::load(&root).unwrap()
}

fn no_warnings(t: &Template) {
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
}

#[test]
fn channel_fixture_v1_loads() {
    let t = channel_fixture("channel", "v1");
    assert_eq!(
        (t.manifest.name.as_str(), t.manifest.base.as_str()),
        ("rig", "core")
    );
    assert_eq!(
        t.pieces.keys().collect::<Vec<_>>(),
        ["claude", "collide", "core", "hooks", "skills"]
    );
    assert!(t.pieces["skills"].multiple);
    assert_eq!(
        t.pieces["hooks"].post[0].run,
        "echo rig-post >> .rig-post.log"
    );
    no_warnings(&t);
    assert!(t.migrations.is_empty());

    let v2 = channel_fixture("channel", "v2");
    no_warnings(&v2);
    assert_eq!(v2.migrations.len(), 1);
    assert_eq!(
        v2.migrations[0].removes[0].path,
        ".claude/agents/explorer.md"
    );
    assert_eq!(v2.migrations[0].removes[0].piece.as_deref(), Some("claude"));
}

#[test]
fn channel_variant_fixtures_load() {
    for name in ["v2-collide", "v2-x"] {
        no_warnings(&channel_fixture("channel", name));
    }
    assert!(channel_fixture("template", "v1-claude").warnings.is_empty());
    assert!(channel_fixture("template", "v2-x").warnings.is_empty());
}

#[test]
fn channel_b_fixture_loads() {
    let t = channel_fixture("channel-b", "v1");
    assert_eq!(
        (t.manifest.name.as_str(), t.manifest.base.as_str()),
        ("rigb", "notes")
    );
    assert_eq!(t.pieces["notes"].post.len(), 2);
    assert!(t.warnings.is_empty(), "{:?}", t.warnings);
    let v2 = channel_fixture("channel-b", "v2");
    no_warnings(&v2);
    assert_eq!(v2.migrations.len(), 1);
    assert!(
        v2.migrations[0].steps[0]
            .run
            .contains("SPLICE_FIXTURE_FAIL")
    );
}

#[test]
fn sections_parsed() {
    let t = channel_fixture("channel", "v1");
    no_warnings(&t);
    assert_eq!(
        t.pieces["core"].sections["AGENTS.md"],
        "sections/rules.md.jinja"
    );
    assert_eq!(
        t.pieces["claude"].sections["CLAUDE.md"],
        "sections/pointer.md"
    );
    assert!(t.pieces["hooks"].sections.is_empty());
    assert_eq!(t.has_sections(&["hooks", "claude", "core"]), Some("claude"));
    assert_eq!(t.has_sections(&["hooks", "skills"]), None);
}

const SECTION_PIECE: &str = "id = \"p\"\ndescription = \"d\"\n";

fn section_err(sections: &str, files: &[(&str, &str)]) -> String {
    let toml = format!("{SECTION_PIECE}\n[sections]\n{sections}\n");
    let dir = inline_template(&[("p", &toml, files)]);
    let e = Template::load(dir.path()).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    e.to_string()
}

#[test]
fn non_md_host_or_fragment_rejected() {
    let m = section_err("\"AGENTS.txt\" = \"s.md\"", &[("s.md", "x\n")]);
    assert!(m.contains("piece.toml") && m.contains("AGENTS.txt"), "{m}");
    let m = section_err("\"AGENTS.md\" = \"x.txt\"", &[("x.txt", "x\n")]);
    assert!(m.contains("piece.toml") && m.contains("x.txt"), "{m}");
}

#[test]
fn missing_or_escaping_fragment_rejected() {
    let m = section_err("\"AGENTS.md\" = \"nope.md\"", &[]);
    assert!(m.contains("piece.toml") && m.contains("nope.md"), "{m}");
    let m = section_err("\"AGENTS.md\" = \"../s.md\"", &[]);
    assert!(m.contains("piece.toml") && m.contains("../s.md"), "{m}");
}

#[cfg(unix)]
mod symlinks {
    use super::*;

    const TOML: &str = "id = \"p\"\ndescription = \"d\"\n\n[sections]\n\"AGENTS.md\" = \"FRAG\"\n";

    /// A template whose fragment `frag` is reached through a symlink to a secret outside it.
    fn load_through_link(frag: &str, link: &str, link_to_dir: bool) -> String {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("s.md"), "SECRET\n").unwrap();
        let toml = TOML.replace("FRAG", frag);
        let dir = inline_template(&[("p", &toml, &[])]);
        let files = dir.path().join("pieces/p/files");
        let link_path = files.join(link);
        let target = if link_to_dir {
            outside.path().to_path_buf()
        } else {
            outside.path().join("s.md")
        };
        std::os::unix::fs::symlink(target, link_path).unwrap();
        let e = Template::load(dir.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        e.to_string()
    }

    #[test]
    fn symlinked_fragment_rejected() {
        let m = load_through_link("s.md", "s.md", false);
        assert!(m.contains("piece.toml") && m.contains("s.md"), "{m}");
    }

    #[test]
    fn fragment_below_symlinked_dir_rejected() {
        let m = load_through_link("sub/s.md", "sub", true);
        assert!(m.contains("piece.toml") && m.contains("sub/s.md"), "{m}");
    }
}

/// A template dir with `splice.toml` (min_splice `min`) and one base piece `p`.
fn one_piece(min: &str, piece_extra: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let toml = format!("id = \"p\"\ndescription = \"d\"\n{piece_extra}");
    let dir = inline_template(&[("p", &toml, files)]);
    std::fs::write(
        dir.path().join("splice.toml"),
        format!("format = 1\nname = \"t\"\nmin_splice = \"{min}\"\nbase = \"p\"\n"),
    )
    .unwrap();
    dir
}

#[test]
fn description_and_prompt_have_defaults() {
    let dir = inline_template(&[("p", "id = \"p\"\n[vars]\nname = { default = \"x\" }\n", &[])]);
    let t = Template::load(dir.path()).unwrap();
    assert_eq!(t.pieces["p"].description, "");
    assert_eq!(t.pieces["p"].vars["name"].prompt, "name");
}

#[test]
fn keyword_var_names_are_reserved() {
    for name in ["true", "false", "none", "not", "and", "or", "in", "is"] {
        let piece = format!("[vars]\n{name} = {{ prompt = \"p\" }}\n");
        let e = Template::load(one_piece("0.2.0", &piece, &[]).path()).unwrap_err();
        assert_eq!(e.exit_code(), 3, "{name}");
        assert!(e.to_string().contains("reserved"), "{name}: {e}");
    }
}

#[test]
fn sections_need_min_splice_0_2_0() {
    let sections = "[sections]\n\"AGENTS.md\" = \"s.md\"\n";
    let files: &[(&str, &str)] = &[("s.md", "x\n")];
    let e = Template::load(one_piece("0.1.0", sections, files).path()).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    let m = e.to_string();
    assert!(
        m.contains("[sections]") && m.contains("min_splice") && m.contains("0.2.0"),
        "{m}"
    );
    Template::load(one_piece("0.2.0", sections, files).path()).unwrap();
    Template::load(one_piece("1.0.0", sections, files).path()).unwrap();
}

#[test]
fn raw_needs_min_splice_0_2_0() {
    let raw: &[(&str, &str)] = &[("sub/a.txt.jinja", "x{% raw %}{{ y }}{% endraw %}\n")];
    let e = Template::load(one_piece("0.1.0", "", raw).path()).unwrap_err();
    assert_eq!(e.exit_code(), 3);
    let m = e.to_string();
    assert!(
        m.contains("raw") && m.contains("min_splice") && m.contains("0.2.0"),
        "{m}"
    );
    assert!(m.contains("a.txt.jinja"), "{m}");
    Template::load(one_piece("0.2.0", "", raw).path()).unwrap();
    // Not a raw use: a non-`.jinja` file, and a raw tag inside a comment.
    let plain: &[(&str, &str)] = &[
        ("a.txt", "{% raw %}x{% endraw %}\n"),
        ("b.jinja", "{# {% raw %} #}\n"),
    ];
    Template::load(one_piece("0.1.0", "", plain).path()).unwrap();
}

#[cfg(unix)]
mod manifest_symlinks {
    use super::*;
    use std::path::Path;

    /// Move `path` out of the template and leave a symlink to it in its place.
    fn relink(path: &Path, outside: &Path) {
        let real = outside.join("real");
        std::fs::rename(path, &real).unwrap();
        std::os::unix::fs::symlink(&real, path).unwrap();
    }

    fn load_err(dir: &Path) -> String {
        let e = Template::load(dir).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        e.to_string()
    }

    #[test]
    fn symlinked_manifest_parts_rejected() {
        for rel in [
            "splice.toml",
            "pieces/p/piece.toml",
            "pieces",
            "pieces/p",
            "migrations",
            "migrations/1.0.0.toml",
        ] {
            let dir = one_piece("0.1.0", "", &[]);
            std::fs::create_dir_all(dir.path().join("migrations")).unwrap();
            std::fs::write(dir.path().join("migrations/1.0.0.toml"), "").unwrap();
            Template::load(dir.path()).unwrap();
            let outside = tempfile::tempdir().unwrap();
            relink(&dir.path().join(rel), outside.path());
            let m = load_err(dir.path());
            assert!(m.contains("symlink"), "{rel}: {m}");
        }
    }

    #[test]
    fn symlinked_extra_piece_dir_is_an_error_not_skipped() {
        let dir = one_piece("0.1.0", "", &[]);
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("piece.toml"), "id = \"q\"\n").unwrap();
        std::os::unix::fs::symlink(other.path(), dir.path().join("pieces/q")).unwrap();
        let m = load_err(dir.path());
        assert!(m.contains("symlink") && m.contains("q"), "{m}");
    }
}

#[test]
fn merge_base_loads_without_the_new_gates() {
    let raw: &[(&str, &str)] = &[("a.txt.jinja", "{% raw %}x{% endraw %}\n")];
    let dir = one_piece("0.1.0", "[vars]\nis = { prompt = \"p\" }\n", raw);
    // `.git` breaks the new segment rule and, unlike `con`, can be created on Windows too.
    std::fs::create_dir_all(dir.path().join("pieces/.git/files")).unwrap();
    std::fs::write(dir.path().join("pieces/.git/piece.toml"), "id = \".git\"\n").unwrap();
    assert_eq!(Template::load(dir.path()).unwrap_err().exit_code(), 3);
    let t = Template::load_merge_base(dir.path()).unwrap();
    assert!(t.pieces.contains_key(".git") && t.pieces["p"].vars.contains_key("is"));
    // Old reserved names stay rejected even in a merge base.
    let dir = one_piece("0.2.0", "[vars]\npieces = { prompt = \"p\" }\n", &[]);
    assert_eq!(
        Template::load_merge_base(dir.path())
            .unwrap_err()
            .exit_code(),
        3
    );
}
