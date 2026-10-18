//! Detection of unresolved diff3 conflict markers (spec §8.7).

const OURS: &str = "<<<<<<< ours";
const BASE: &str = "||||||| base";
const THEIRS: &str = ">>>>>>> template";

/// 1-based lines with conflict markers. A `<<<<<<< ours` line opens a block and is reported once
/// per block; `||||||| base` and `=======` inside an open block belong to it, and `>>>>>>> template`
/// closes it. Outside a block, `=======` is plain text, but a stray `||||||| base` or
/// `>>>>>>> template` line is reported on its own (spec §8.7).
pub fn find_markers(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut open = false;
    for (i, line) in text.lines().enumerate() {
        let n = i + 1;
        if line.starts_with(OURS) {
            if !open {
                found.push(n);
            }
            open = true;
        } else if line.starts_with(THEIRS) {
            if !open {
                found.push(n);
            }
            open = false;
        } else if !open && line.starts_with(BASE) {
            found.push(n);
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
    fn stray_base_and_template_lines_count() {
        assert_eq!(find_markers("a\n||||||| base\n"), vec![2]);
        assert_eq!(find_markers("a\nb\n>>>>>>> template\n"), vec![3]);
    }

    #[test]
    fn diff3_block_counts_once_and_unterminated_block_counts() {
        let t = "<<<<<<< ours\no\n||||||| base\nb\n=======\nt\n>>>>>>> template\nz\n<<<<<<< ours\n";
        assert_eq!(find_markers(t), vec![1, 9]);
    }

    #[test]
    fn two_blocks() {
        let t =
            "<<<<<<< ours\n=======\n>>>>>>> template\n<<<<<<< ours\n=======\n>>>>>>> template\n";
        assert_eq!(find_markers(t), vec![1, 4]);
    }
}
