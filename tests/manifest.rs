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
