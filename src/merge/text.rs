//! Line-based three-way text merge with the contract's diff3 markers.

use super::Merged;
use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions};

const OURS: &str = "<<<<<<< ours";
const BASE: &str = "||||||| base";
const SEP: &str = "=======";
const THEIRS: &str = ">>>>>>> template";

/// Three-way merge of `ours` and `theirs` against `base`.
///
/// The line ending is decided by `ours`: if it contains CRLF, all three sides are
/// normalised to LF for the merge and CRLF is restored in the result.
pub fn merge_text(base: &str, ours: &str, theirs: &str) -> Merged {
    let crlf = ours.contains("\r\n");
    let (b, o, t) = if crlf {
        (lf(base), lf(ours), lf(theirs))
    } else {
        (base.to_string(), ours.to_string(), theirs.to_string())
    };

    // diffy cannot take custom labels, so pick a marker length n such that no
    // input line starts with a run of n marker characters. Every line diffy
    // emits with that length is then certainly a delimiter and safe to relabel.
    let n = marker_len(&[&b, &o, &t]);
    let mut opts = MergeOptions::new();
    opts.set_conflict_style(ConflictStyle::Diff3)
        .set_incomplete_hunk_style(IncompleteHunkStyle::Git)
        .set_conflict_marker_length(n);
    let (raw, clean) = match opts.merge(&b, &o, &t) {
        Ok(s) => (s, true),
        Err(s) => (s, false),
    };
    if clean {
        return Merged {
            text: eol(raw, crlf),
            conflicts: vec![],
        };
    }

    let ours_d = "<".repeat(n) + " ours";
    let base_d = "|".repeat(n) + " original";
    let sep_d = "=".repeat(n);
    let theirs_d = ">".repeat(n) + " theirs";
    let mut out = String::with_capacity(raw.len());
    let mut conflicts = Vec::new();
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let repl = if body == ours_d {
            conflicts.push(format!("line {}", i + 1));
            OURS
        } else if body == base_d {
            BASE
        } else if body == sep_d {
            SEP
        } else if body == theirs_d {
            THEIRS
        } else {
            body
        };
        out.push_str(repl);
        out.push_str(nl);
    }
    Merged {
        text: eol(out, crlf),
        conflicts,
    }
}

/// The whole file as one diff3 block (ours / base / theirs), in ours' line endings.
pub fn wrap_whole(base: &str, ours: &str, theirs: &str) -> String {
    let crlf = ours.contains("\r\n");
    let mut out = String::new();
    for (head, body) in [(OURS, ours), (BASE, base), (SEP, theirs)] {
        out.push_str(head);
        out.push('\n');
        push_lines(&mut out, &lf(body));
    }
    out.push_str(THEIRS);
    out.push('\n');
    eol(out, crlf)
}

fn push_lines(out: &mut String, body: &str) {
    out.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
}

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn eol(s: String, crlf: bool) -> String {
    if crlf { s.replace('\n', "\r\n") } else { s }
}

fn marker_len(texts: &[&str]) -> usize {
    let mut n = 7;
    loop {
        let clash = texts.iter().any(|t| {
            t.lines().any(|l| {
                ['<', '|', '=', '>']
                    .iter()
                    .any(|c| l.chars().take(n).filter(|x| x == c).count() == n)
            })
        });
        if !clash {
            return n;
        }
        n += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markers::find_markers;

    #[test]
    fn clean_merge() {
        assert_eq!(
            merge_text("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n").text,
            "A\nb\nC\n"
        );
    }

    #[test]
    fn conflict_uses_contract_markers() {
        let m = merge_text("x\n", "ours\n", "theirs\n");
        assert_eq!(
            m.text,
            "<<<<<<< ours\nours\n||||||| base\nx\n=======\ntheirs\n>>>>>>> template\n"
        );
        assert_eq!(m.conflicts, vec!["line 1".to_string()]);
        assert_eq!(find_markers(&m.text), vec![1]);
    }

    #[test]
    fn crlf_ours_is_kept() {
        assert_eq!(
            merge_text("a\nb\nc\n", "a\r\nb\r\nC\r\n", "A\nb\nc\n").text,
            "A\r\nb\r\nC\r\n"
        );
    }

    #[test]
    fn all_crlf_merges_line_by_line() {
        let m = merge_text("a\r\nb\r\nc\r\n", "A\r\nb\r\nc\r\n", "a\r\nb\r\nC\r\n");
        assert_eq!(m.text, "A\r\nb\r\nC\r\n");
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn crlf_conflict_keeps_crlf_everywhere() {
        let m = merge_text("x\r\n", "ours\r\n", "theirs\n");
        assert_eq!(
            m.text,
            "<<<<<<< ours\r\nours\r\n||||||| base\r\nx\r\n=======\r\ntheirs\r\n>>>>>>> template\r\n"
        );
        assert_eq!(find_markers(&m.text), vec![1]);
    }

    #[test]
    fn literal_marker_text_outside_hunks_untouched() {
        let base = "||||||| original\nk\n";
        let m = merge_text(base, base, "||||||| original\nK\n");
        assert_eq!(m.text, "||||||| original\nK\n");
    }

    #[test]
    fn literal_marker_text_beside_a_real_conflict_untouched() {
        let base = "||||||| original\n=======\n>>>>>>> theirs\nx\n";
        let ours = "||||||| original\n=======\n>>>>>>> theirs\nours\n";
        let theirs = "||||||| original\n=======\n>>>>>>> theirs\ntheirs\n";
        let m = merge_text(base, ours, theirs);
        assert_eq!(
            m.text,
            "||||||| original\n=======\n>>>>>>> theirs\n<<<<<<< ours\nours\n||||||| base\nx\n=======\ntheirs\n>>>>>>> template\n"
        );
        assert_eq!(m.conflicts, vec!["line 4".to_string()]);
    }

    #[test]
    fn missing_final_newline_clean() {
        assert_eq!(merge_text("a\nb", "A\nb", "a\nb").text, "A\nb");
        assert_eq!(merge_text("a\nb\n", "a\nb\n", "a\nb").text, "a\nb");
        assert_eq!(merge_text("a\nb\nc", "A\nb\nc", "a\nb\nC").text, "A\nb\nC");
    }

    #[test]
    fn missing_final_newline_conflict_has_markers_on_own_lines() {
        for (b, o, t) in [
            ("x", "ours", "theirs"),
            ("x\n", "ours", "theirs\n"),
            ("x\n", "ours\n", "theirs"),
        ] {
            let m = merge_text(b, o, t);
            assert_eq!(m.conflicts.len(), 1, "{b:?} {o:?} {t:?}");
            assert_eq!(find_markers(&m.text), vec![1]);
            assert!(m.text.contains("\n=======\n"));
            assert!(m.text.ends_with(">>>>>>> template\n") || m.text.ends_with(">>>>>>> template"));
        }
    }

    #[test]
    fn missing_final_newline_with_crlf_ours() {
        assert_eq!(
            merge_text("a\nb\nc", "a\r\nb\r\nC", "A\nb\nc").text,
            "A\r\nb\r\nC"
        );
    }

    #[test]
    fn wrap_whole_is_one_block() {
        let w = wrap_whole("b\n", "o\n", "t");
        assert_eq!(
            w,
            "<<<<<<< ours\no\n||||||| base\nb\n=======\nt\n>>>>>>> template\n"
        );
        assert_eq!(find_markers(&w), vec![1]);
        let c = wrap_whole("b\n", "o\r\n", "t\n");
        assert_eq!(
            c,
            "<<<<<<< ours\r\no\r\n||||||| base\r\nb\r\n=======\r\nt\r\n>>>>>>> template\r\n"
        );
    }
}
