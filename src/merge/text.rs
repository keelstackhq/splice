//! Line-based 3-way merge with the contract's diff3 markers.

use diffy::{ConflictStyle, IncompleteHunkStyle, MergeOptions};

use super::Merged;

/// 3-way merge of `ours` (the file on disk) and `theirs` (the new template render) against `base`
/// (the old render). A conflict yields diff3 blocks with the contract labels `ours`, `base`,
/// `template`. If `ours` uses CRLF the result does too.
pub fn merge_text(base: &str, ours: &str, theirs: &str) -> Merged {
    let crlf = ours.contains("\r\n");
    let (b, o, t) = (lf(base), lf(ours), lf(theirs));
    let result = MergeOptions::new()
        .set_conflict_style(ConflictStyle::Diff3)
        .set_incomplete_hunk_style(IncompleteHunkStyle::Git)
        .merge(&b, &o, &t);
    let (text, conflicts) = match result {
        Ok(text) => (text, Vec::new()),
        Err(raw) => relabel(&raw),
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

#[derive(Clone, Copy)]
enum State {
    Outside,
    Ours,
    Base,
    Theirs,
}

/// Rename diffy's `||||||| original` / `>>>>>>> theirs` markers, inside conflict hunks only, and
/// collect `line N` for each hunk. Literal marker-like text outside hunks stays as it is.
fn relabel(raw: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(raw.len());
    let mut conflicts = Vec::new();
    let mut state = State::Outside;
    for (i, line) in raw.split_inclusive('\n').enumerate() {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let (next, text) = match state {
            State::Outside if body.starts_with("<<<<<<< ") => {
                conflicts.push(format!("line {}", i + 1));
                (State::Ours, "<<<<<<< ours")
            }
            State::Ours if body.starts_with("||||||| ") => (State::Base, "||||||| base"),
            State::Base if body == "=======" => (State::Theirs, body),
            State::Theirs if body.starts_with(">>>>>>> ") => (State::Outside, ">>>>>>> template"),
            s => (s, body),
        };
        state = next;
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
    fn wrap_whole_is_one_block() {
        assert_eq!(
            wrap_whole("b\n", "o", "t\n"),
            "<<<<<<< ours\no\n||||||| base\nb\n=======\nt\n>>>>>>> template\n"
        );
    }
}
