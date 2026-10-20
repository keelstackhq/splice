mod common;

use common::{flags, splice_cmd, unknown_flags};

/// Flags of other tools that the docs may show. Keep this short.
const OTHER_TOOLS: &[&str] = &[
    "--locked", // cargo install --locked (README, source install)
    "--git",    // cargo install --git (README, source install)
];

#[test]
fn docs_mention_only_real_flags() {
    // Help is pooled across subcommands and the allowlist applies to both files on purpose:
    // this catches invented flags, not flags used on the wrong subcommand.
    let mut help = String::new();
    for args in [
        vec!["new", "--help"],
        vec!["add", "--help"],
        vec!["update", "--help"],
        vec!["check", "--help"],
        vec!["diff", "--help"],
        vec!["explain", "--help"],
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
    for path in ["README.md", "skills/splice/SKILL.md"] {
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
    let body = text.splitn(3, "---\n").nth(2).unwrap();
    assert!(body.lines().count() <= 80, "SKILL.md body over 80 lines");
}
