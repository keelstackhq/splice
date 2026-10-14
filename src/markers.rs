//! Detection of leftover diff3 conflict markers (contract section 5).

/// 1-based line numbers of every `<<<<<<< ours` block. `=======` and
/// `||||||| base` only count inside an open block; `>>>>>>> template` closes it.
/// A lone `=======` (for example a Markdown heading underline) is not a marker.
pub fn find_markers(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut open = false;
    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if open {
            if line.starts_with(">>>>>>> template") {
                open = false;
            }
        } else if line.starts_with("<<<<<<< ours") {
            open = true;
            found.push(i + 1);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_found_only_in_blocks() {
        assert_eq!(find_markers("=======\nplain\n"), Vec::<usize>::new());
        assert_eq!(
            find_markers("a\n<<<<<<< ours\nx\n=======\ny\n>>>>>>> template\n"),
            vec![2]
        );
    }

    #[test]
    fn stray_delimiters_outside_a_block_are_ignored() {
        assert!(find_markers("||||||| base\n=======\n>>>>>>> template\n").is_empty());
    }

    #[test]
    fn crlf_lines_and_multiple_blocks() {
        let t = "<<<<<<< ours\r\na\r\n=======\r\nb\r\n>>>>>>> template\r\nok\r\n<<<<<<< ours\r\n";
        assert_eq!(find_markers(t), vec![1, 7]);
    }

    #[test]
    fn nested_open_does_not_double_count() {
        assert_eq!(
            find_markers("<<<<<<< ours\n<<<<<<< ours\n>>>>>>> template\n"),
            vec![1]
        );
    }
}
