//! Detection of leftover diff3 conflict markers (contract section 5).

/// 1-based line numbers of leftover conflict markers (spec §8.7): every `<<<<<<< ours` line
/// (a block start), plus every `||||||| base` or `>>>>>>> template` line outside a block (a
/// half-resolved file). `=======` only counts inside a block, so a lone one (a Markdown heading
/// underline) is not a marker. Inside a block, `>>>>>>> template` closes it and is not listed.
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
        } else if line.starts_with("||||||| base") || line.starts_with(">>>>>>> template") {
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
    fn stray_base_and_template_lines_count_but_a_lone_separator_does_not() {
        assert_eq!(find_markers("a\n||||||| base\nb\n"), vec![2]);
        assert_eq!(find_markers("a\n>>>>>>> template\n"), vec![2]);
        assert!(find_markers("a\n=======\nb\n").is_empty());
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
