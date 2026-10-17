//! Line-based 3-way merge with the contract's diff3 markers.

use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions};

use super::Merged;

/// 3-way merge of `ours` (the file on disk) and `theirs` (the new template render) against `base`
/// (the old render). A conflict yields diff3 blocks with the contract labels `ours`, `base`,
/// `template`. CRLF policy: if `ours` contains any CRLF the whole result uses CRLF; an LF `ours`
/// stays LF, whatever `base` and `theirs` use. Diffy runs with a marker length longer than any
/// line-start run in the inputs, so file content can never be mistaken for a marker.
pub fn merge_text(base: &str, ours: &str, theirs: &str) -> Merged {
    let crlf = ours.contains("\r\n");
    let (b, o, t) = (lf(base), lf(ours), lf(theirs));
    let len = [&b, &o, &t]
        .into_iter()
        .map(|s| longest_marker_run(s))
        .max()
        .unwrap_or(0)
        .max(7)
        + 1;
    let result = MergeOptions::new()
        .set_conflict_marker_length(len)
        .set_conflict_style(ConflictStyle::Diff3)
        .set_incomplete_hunk_style(IncompleteHunkStyle::Git)
        .merge(&b, &o, &t);
    let (text, conflicts) = match result {
        Ok(text) => (text, Vec::new()),
        Err(raw) => relabel(&raw, len),
    };
    let text = if crlf {
        text.replace('\n', "\r\n")
    } else {
        text
    };
    Merged { text, conflicts }
}

/// The whole file as one diff3 conflict block, for files that cannot be merged line by line.
pub fn wrap_whole(base: &str, ours: &str, theirs: &str) -> String {
    let mut out = String::from("<<<<<<< ours\n");
    for (part, sep) in [(ours, "||||||| base\n"), (base, "=======\n"), (theirs, "")] {
        out.push_str(part);
        if !part.is_empty() && !part.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(sep);
    }
    out.push_str(">>>>>>> template\n");
    out
}

fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Length of the longest run of marker characters at the start of any line.
fn longest_marker_run(text: &str) -> usize {
    text.lines()
        .map(|l| {
            let c = l.chars().next().filter(|c| "<=|>".contains(*c));
            c.map_or(0, |c| l.chars().take_while(|&x| x == c).count())
        })
        .max()
        .unwrap_or(0)
}

/// Rewrite diffy's markers, which have exactly `len` characters (no input line starts with a run
/// that long, so they cannot be confused with content), to the contract's 7-character labels, and
/// collect `line N` for each hunk.
fn relabel(raw: &str, len: usize) -> (String, Vec<String>) {
    let (open, base, sep, close) = (
        format!("{} ", "<".repeat(len)),
        format!("{} ", "|".repeat(len)),
        "=".repeat(len),
        format!("{} ", ">".repeat(len)),
    );
    let mut out = String::with_capacity(raw.len());
    let mut conflicts = Vec::new();
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let text = if body.starts_with(&open) {
            conflicts.push(format!("line {}", i + 1));
            "<<<<<<< ours"
        } else if body.starts_with(&base) {
            "||||||| base"
        } else if body == sep {
            "======="
        } else if body.starts_with(&close) {
            ">>>>>>> template"
        } else {
            body
        };
        out.push_str(text);
        out.push_str(nl);
    }
    (out, conflicts)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(m.conflicts, vec!["line 1"]);
    }

    #[test]
    fn crlf_ours_is_kept() {
        assert_eq!(
            merge_text("a\nx\nb\n", "a\r\nx\r\nB\r\n", "A\nx\nb\n").text,
            "A\r\nx\r\nB\r\n"
        );
    }

    #[test]
    fn literal_marker_text_outside_hunks_untouched() {
        let base = "||||||| original\nk\n";
        let m = merge_text(base, base, "||||||| original\nK\n");
        assert_eq!(m.text, "||||||| original\nK\n");
        assert!(m.conflicts.is_empty());
    }

    #[test]
    fn literal_marker_text_outside_a_conflict_hunk_untouched() {
        let base = "||||||| original\nk\nm\nn\no\nx\n";
        let ours = "||||||| original\nk\nm\nn\no\nours\n";
        let theirs = "||||||| original\nk\nm\nn\no\ntheirs\n";
        let m = merge_text(base, ours, theirs);
        assert!(m.text.starts_with("||||||| original\nk\n"));
        assert!(
            m.text.contains(
                "<<<<<<< ours\nours\n||||||| base\nx\n=======\ntheirs\n>>>>>>> template\n"
            )
        );
        assert_eq!(m.conflicts, vec!["line 6"]);
    }

    #[test]
    fn missing_final_newline_conflict_still_has_line_start_markers() {
        let m = merge_text("x", "ours", "theirs");
        assert_eq!(
            m.text,
            "<<<<<<< ours\nours\n||||||| base\nx\n=======\ntheirs\n>>>>>>> template\n"
        );
    }

    #[test]
    fn literal_head_marker_outside_hunk_stays_and_line_is_correct() {
        let base = "<<<<<<< HEAD\nk\nm\nn\no\nx\n";
        let ours = "<<<<<<< HEAD\nk\nm\nn\no\nours\n";
        let theirs = "<<<<<<< HEAD\nk\nm\nn\no\ntheirs\n";
        let m = merge_text(base, ours, theirs);
        assert!(m.text.starts_with("<<<<<<< HEAD\nk\n"));
        assert_eq!(m.conflicts, vec!["line 6"]);
    }

    #[test]
    fn literal_theirs_marker_outside_hunk_stays() {
        let base = ">>>>>>> theirs\nk\nm\nn\no\nx\n";
        let ours = ">>>>>>> theirs\nk\nm\nn\no\nours\n";
        let theirs = ">>>>>>> theirs\nk\nm\nn\no\ntheirs\n";
        let m = merge_text(base, ours, theirs);
        assert!(m.text.starts_with(">>>>>>> theirs\nk\n"));
        assert_eq!(m.conflicts, vec!["line 6"]);
    }

    #[test]
    fn theirs_line_starting_with_marker_inside_hunk_does_not_end_it() {
        let m = merge_text("x\n", "ours\n", ">>>>>>> theirs\nmore\n");
        assert_eq!(
            m.text,
            "<<<<<<< ours\nours\n||||||| base\nx\n=======\n>>>>>>> theirs\nmore\n>>>>>>> template\n"
        );
        assert_eq!(m.conflicts, vec!["line 1"]);
    }

    #[test]
    fn wrap_whole_is_one_block() {
        assert_eq!(
            wrap_whole("b\n", "o", "t\n"),
            "<<<<<<< ours\no\n||||||| base\nb\n=======\nt\n>>>>>>> template\n"
        );
    }
}
