use std::path::Path;

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

/// Until the `[sections]` manifest key is known (Task 5) it is the only warning.
fn only_sections_warnings(t: &Template) {
    for w in &t.warnings {
        assert!(w.ends_with("unknown key `sections` ignored"), "{w}");
    }
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
    assert_eq!(t.warnings.len(), 2, "{:?}", t.warnings); // core and claude declare sections
    only_sections_warnings(&t);
    assert!(t.migrations.is_empty());

    let v2 = channel_fixture("channel", "v2");
    only_sections_warnings(&v2);
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
        only_sections_warnings(&channel_fixture("channel", name));
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
    only_sections_warnings(&v2);
    assert_eq!(v2.migrations.len(), 1);
    assert!(
        v2.migrations[0].steps[0]
            .run
            .contains("SPLICE_FIXTURE_FAIL")
    );
}
