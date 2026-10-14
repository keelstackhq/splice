use splice::render::Renderer;
use std::path::Path;

#[test]
fn corpus_renders_exactly() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/subset");
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        let Some(name) = p
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .strip_suffix(".jinja")
        else {
            continue;
        };
        let src = std::fs::read_to_string(&p).unwrap();
        let ctx: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("{name}.ctx.json"))).unwrap(),
        )
        .unwrap();
        let want = std::fs::read(dir.join(format!("{name}.out"))).unwrap();
        let got = Renderer::new()
            .render_str(&src, &minijinja::Value::from_serialize(&ctx), name)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(got.as_bytes(), want.as_slice(), "case {name}");
        n += 1;
    }
    assert_eq!(n, 14);
}

#[test]
fn strict_undefined() {
    let e = Renderer::new()
        .render_str("{{ nope }}", &minijinja::context! {}, "t")
        .unwrap_err();
    assert_eq!(e.exit_code(), 3);
}

#[test]
fn crlf_normalised() {
    let out = Renderer::new()
        .render_str("a\r\n{{ v }}\r\n", &minijinja::context! { v => "x" }, "t")
        .unwrap();
    assert_eq!(out, "a\nx\n");
}

#[test]
fn gate_runs_before_engine() {
    let e = Renderer::new()
        .render_str("ok\n{% set x = 1 %}", &minijinja::context! {}, "f.jinja")
        .unwrap_err();
    assert!(e.to_string().contains("f.jinja:2"), "{e}");
}
