//! Drift guard: every `--flag` the README and the agent skill mention must exist in the
//! `--help` of the built binary, unless it belongs to another tool (allowlist below).

use std::collections::BTreeSet;
use std::process::Command;

/// Flags of other tools that the docs show (git, gh, npm, pnpm). Keep this short.
const OTHER_TOOLS: &[&str] = &[];

fn help(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_splice"))
        .args(args)
        .arg("--help")
        .output()
        .expect("run splice --help");
    assert!(out.status.success(), "splice {args:?} --help failed");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn flags_in(text: &str) -> BTreeSet<String> {
    let b = text.as_bytes();
    let mut set = BTreeSet::new();
    let mut i = 0;
    while i + 2 < b.len() {
        let at_start = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'-');
        if at_start && b[i] == b'-' && b[i + 1] == b'-' && b[i + 2].is_ascii_lowercase() {
            let mut j = i + 2;
            while j < b.len() && (b[j].is_ascii_lowercase() || b[j] == b'-') {
                j += 1;
            }
            set.insert(text[i..j].trim_end_matches('-').to_string());
            i = j;
        } else {
            i += 1;
        }
    }
    set
}

#[test]
fn docs_mention_only_real_flags() {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut all_help = help(&[]);
    for sub in ["new", "add", "update", "check", "diff", "explain"] {
        all_help.push_str(&help(&[sub]));
    }
    let known = flags_in(&all_help);
    assert!(known.contains("--offline") && known.contains("--allow-dirty"));
    for file in ["README.md", "skills/splice/SKILL.md"] {
        let text = std::fs::read_to_string(format!("{root}/{file}"))
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let used = flags_in(&text);
        assert!(!used.is_empty(), "{file} mentions no flags");
        for f in used {
            assert!(
                known.contains(&f) || OTHER_TOOLS.contains(&f.as_str()),
                "{file} mentions {f}, which is in no `splice --help`"
            );
        }
    }
}
