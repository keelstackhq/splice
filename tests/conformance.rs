//! Conformance corpus: `tests/fixtures/subset/<case>.{jinja,ctx.json,out}` triples.
//! hull renders the same corpus with Nunjucks; any difference is a contract bug.

use std::path::Path;

use minijinja::{Value, context};
use splice::render::Renderer;

fn corpus() -> &'static Path {
    Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/subset"
    ))
}

const CASES: [&str; 11] = [
    "var",
    "filters",
    "if-elif-else",
    "in-pieces",
    "for-pieces",
    "comment",
    "dotted",
    "trailing-newline",
    "no-trailing-newline",
    "blank-lines-around-blocks",
    "unicode",
];

#[test]
fn corpus_has_every_case_and_nothing_else() {
    let mut found: Vec<String> = std::fs::read_dir(corpus())
        .unwrap()
        .filter_map(|e| {
            let n = e.unwrap().file_name().into_string().unwrap();
            n.strip_suffix(".jinja").map(str::to_string)
        })
        .collect();
    found.sort();
    let mut want: Vec<String> = CASES.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(found, want);
}

#[test]
fn corpus_renders_exactly() {
    let r = Renderer::new();
    for case in CASES {
        let read =
            |ext: &str| std::fs::read_to_string(corpus().join(format!("{case}.{ext}"))).unwrap();
        let ctx: serde_json::Value = serde_json::from_str(&read("ctx.json")).unwrap();
        let got = r
            .render_str(&read("jinja"), &Value::from_serialize(&ctx), case)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        assert_eq!(got, read("out"), "case {case}");
    }
}

#[test]
fn strict_undefined() {
    let r = Renderer::new();
    for t in ["{{ nope }}", "{{ a.nope }}", "{% if nope %}x{% endif %}"] {
        let e = r
            .render_str(t, &context! { a => context! {} }, "t")
            .unwrap_err();
        assert_eq!(e.exit_code(), 3, "{t}");
    }
}

#[test]
fn crlf_input_gives_lf_output() {
    let r = Renderer::new();
    let out = r
        .render_str("a\r\n{{ x }}\r\n", &context! { x => "b" }, "t")
        .unwrap();
    assert_eq!(out, "a\nb\n");
}

#[test]
fn gate_runs_before_render() {
    let r = Renderer::new();
    let e = r
        .render_str("{% set x = 1 %}", &context! {}, "f.jinja")
        .unwrap_err();
    assert_eq!(e.exit_code(), 3);
    assert!(e.to_string().contains("f.jinja:1"), "{e}");
}

#[test]
fn no_autoescape_and_only_two_filters() {
    let r = Renderer::new();
    assert_eq!(
        r.render_str("{{ x }}", &context! { x => "<&>\"" }, "t.html")
            .unwrap(),
        "<&>\""
    );
}
