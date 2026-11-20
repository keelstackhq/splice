mod common;

use common::{flags, splice_cmd, unknown_flags};

/// Flags of other tools that the docs may show. Keep this short.
const OTHER_TOOLS: &[&str] = &[
    "--locked",  // cargo install --locked (README, source install)
    "--git",     // cargo install --git (README, source install)
    "--release", // cargo build --release (getting-started)
];

#[test]
fn docs_mention_only_real_flags() {
    // Help is pooled across subcommands and the allowlist applies to both files on purpose:
    // this catches invented flags, not flags used on the wrong subcommand.
    let mut help = String::new();
    for args in [
        vec!["--help"], // --version
        vec!["new", "--help"],
        vec!["adopt", "--help"],
        vec!["add", "--help"],
        vec!["update", "--help"],
        vec!["check", "--help"],
        vec!["diff", "--help"],
        vec!["explain", "--help"],
        vec!["pieces", "--help"],
        vec!["channel", "add", "--help"],
        vec!["channel", "remove", "--help"],
    ] {
        let out = splice_cmd()
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        help.push_str(&String::from_utf8_lossy(&out));
    }
    for path in [
        "README.md",
        "docs/reference.md",
        "skills/splice/SKILL.md",
        "docs/getting-started.md",
    ] {
        let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let unknown: Vec<&str> = unknown_flags(&text, &help)
            .into_iter()
            .filter(|f| !OTHER_TOOLS.contains(f))
            .collect();
        assert!(unknown.is_empty(), "{path} names unknown flags {unknown:?}");
        assert!(!flags(&text).is_empty(), "{path} shows no flags");
    }
}

#[test]
fn skill_body_is_short_and_named() {
    let text = std::fs::read_to_string("skills/splice/SKILL.md").unwrap();
    assert!(text.starts_with("---\nname: splice\ndescription: Use when "));
    assert!(text.lines().count() <= 60, "SKILL.md over 60 lines");
}

/// `docs/PROGRESS.md` Status names the version in `Cargo.toml` (a stale status misleads agents).
#[test]
fn progress_status_version_is_the_cargo_version() {
    let progress =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/PROGRESS.md"))
            .expect("docs/PROGRESS.md");
    let status = progress
        .split("## Status")
        .nth(1)
        .expect("a ## Status section")
        .split("\n## ")
        .next()
        .unwrap();
    let version = status
        .split("splice ")
        .skip(1)
        .find_map(|rest| {
            let v: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            (v.matches('.').count() == 2).then_some(v)
        })
        .expect("Status names `splice X.Y.Z`");
    assert_eq!(
        version,
        env!("CARGO_PKG_VERSION"),
        "update docs/PROGRESS.md Status"
    );
}
