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
