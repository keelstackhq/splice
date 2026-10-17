//! Section markers and block edits for shared host files (channels spec, sections).

use crate::error::SpliceError;

/// Opening marker line (without line ending); the dash is U+2014.
pub fn begin_marker(channel: &str) -> String {
    format!(
        "<!-- splice:{channel}:begin — managed by `splice update --channel {channel}`; edit the source -->"
    )
}

pub fn end_marker(channel: &str) -> String {
    format!("<!-- splice:{channel}:end -->")
}

/// A marker-delimited block. `begin` and `end` are 1-based marker lines.
#[derive(Debug, PartialEq, Eq)]
pub struct Block {
    pub begin: usize,
    pub end: usize,
    /// Text between the markers, LF line endings.
    pub body: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum MarkerError {
    /// Lines of every begin marker, when there is more than one.
    Duplicate(Vec<usize>),
    /// Line of the begin marker without end, or of the end marker without begin.
    Unbalanced(usize),
}

impl MarkerError {
    pub fn message(&self, host: &str, channel: &str) -> String {
        match self {
            MarkerError::Duplicate(lines) => {
                let list: Vec<String> = lines.iter().map(usize::to_string).collect();
                format!(
                    "{host}: channel {channel} has {} begin markers (lines {}); keep one block and remove the rest by hand",
                    lines.len(),
                    list.join(", ")
                )
            }
            MarkerError::Unbalanced(line) => format!(
                "{host}: unbalanced splice:{channel} marker at line {line}; restore the matching begin/end pair or remove it by hand"
            ),
        }
    }
}

fn eol_of(line: &str) -> &str {
    if line.ends_with("\r\n") {
        "\r\n"
    } else if line.ends_with('\n') {
        "\n"
    } else {
        ""
    }
}

fn is_blank(line: &str) -> bool {
    line.trim_end_matches(['\r', '\n']).is_empty()
}

/// A line without its ending; a UTF-8 BOM on line 1 is skipped so a marker there is recognised.
fn marker_text(line: &str, first: bool) -> &str {
    let t = line.trim_end_matches(['\r', '\n']);
    if first {
        t.strip_prefix('\u{feff}').unwrap_or(t)
    } else {
        t
    }
}

/// `<!-- splice:<ch>:begin`, then whitespace (or `-->`), ending in `-->`; `:beginX` does not match.
fn is_begin(text: &str, channel: &str) -> bool {
    let prefix = format!("<!-- splice:{channel}:begin");
    let Some(rest) = text.strip_prefix(&prefix) else {
        return false;
    };
    let rest = rest.trim_end();
    rest.ends_with("-->") && (rest.starts_with(char::is_whitespace) || rest.starts_with("-->"))
}

/// LF body ending in a newline (empty stays empty); refuses a line that would parse as this
/// channel's marker, which would corrupt the host on the next read.
fn clean_body(channel: &str, body: &str) -> Result<String, SpliceError> {
    let mut body = body.replace("\r\n", "\n");
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    let end = end_marker(channel);
    if body
        .lines()
        .any(|l| is_begin(l.trim_end(), channel) || l.trim_end() == end)
    {
        return Err(SpliceError::Template(format!(
            "channel {channel}: fragment text contains a splice:{channel} marker line; remove it from the source"
        )));
    }
    Ok(body)
}

fn to_eol(text: &str, eol: &str) -> String {
    if eol == "\n" || eol.is_empty() {
        text.to_string()
    } else {
        text.replace('\n', eol)
    }
}

/// Find the block of `channel`. Begin matches by prefix, so a reworded tail still parses; other
/// channels' markers are ignored.
pub fn find_block(host: &str, channel: &str) -> Result<Option<Block>, MarkerError> {
    let end = end_marker(channel);
    let lines: Vec<&str> = host.split_inclusive('\n').collect();
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = marker_text(l, i == 0);
        if is_begin(t, channel) {
            begins.push(i + 1);
        } else if t.trim_end() == end {
            ends.push(i + 1);
        }
    }
    if begins.len() > 1 {
        return Err(MarkerError::Duplicate(begins));
    }
    let Some(&begin) = begins.first() else {
        return match ends.first() {
            Some(&e) => Err(MarkerError::Unbalanced(e)),
            None => Ok(None),
        };
    };
    if let Some(&e) = ends.iter().find(|&&e| e < begin) {
        return Err(MarkerError::Unbalanced(e));
    }
    let Some(&end_line) = ends.first() else {
        return Err(MarkerError::Unbalanced(begin));
    };
    if let Some(&extra) = ends.get(1) {
        return Err(MarkerError::Unbalanced(extra));
    }
    let body = lines[begin..end_line - 1]
        .iter()
        .map(|l| format!("{}\n", l.trim_end_matches(['\r', '\n'])))
        .collect();
    Ok(Some(Block {
        begin,
        end: end_line,
        body,
    }))
}

/// SHA-256 hex of the body with CRLF normalised to LF (G7).
pub fn body_sha(body: &str) -> String {
    crate::fsio::sha256_hex(body.replace("\r\n", "\n").as_bytes())
}

/// Add a block: alone when the host is absent or empty, else after exactly one blank line. A CRLF
/// host (first line ending) gets CRLF.
pub fn insert(host: Option<&str>, channel: &str, body: &str) -> Result<String, SpliceError> {
    let body = clean_body(channel, body)?;
    let host = host.unwrap_or("");
    let eol = host.split_inclusive('\n').next().map_or("\n", |l| {
        if eol_of(l).is_empty() {
            "\n"
        } else {
            eol_of(l)
        }
    });
    let block = format!(
        "{}\n{}{}\n",
        begin_marker(channel),
        body,
        end_marker(channel)
    );
    let mut out = String::from(host);
    if !host.is_empty() {
        if !host.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(eol);
    }
    out.push_str(&to_eol(&block, eol));
    Ok(out)
}

/// Swap the body between the markers; every byte outside stays. The body takes the begin line's
/// line ending.
pub fn replace(
    host: &str,
    channel: &str,
    block: &Block,
    body: &str,
) -> Result<String, SpliceError> {
    let body = clean_body(channel, body)?;
    let lines: Vec<&str> = host.split_inclusive('\n').collect();
    let eol = eol_of(lines[block.begin - 1]);
    let mut out: String = lines[..block.begin].concat();
    out.push_str(&to_eol(&body, eol));
    out.push_str(&lines[block.end - 1..].concat());
    Ok(out)
}

/// Drop the block and the one blank line `insert` put before it. Inverts `insert` exactly for a
/// host that ends in a newline (or is empty); a host without a final newline gains one.
pub fn remove(host: &str, block: &Block) -> String {
    let lines: Vec<&str> = host.split_inclusive('\n').collect();
    let mut start = block.begin - 1;
    let after_blank = lines.get(block.end).is_none_or(|l| is_blank(l));
    if start > 0 && is_blank(lines[start - 1]) && after_blank {
        start -= 1;
    }
    let mut out: String = lines[..start].concat();
    out.push_str(&lines[block.end..].concat());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: &str =
        "<!-- splice:rig:begin — managed by `splice update --channel rig`; edit the source -->";
    const E: &str = "<!-- splice:rig:end -->";

    #[test]
    fn markers_are_exact() {
        assert_eq!(begin_marker("rig"), B);
        assert_eq!(end_marker("rig"), E);
    }

    #[test]
    fn insert_into_absent_and_present() {
        assert_eq!(
            insert(None, "rig", "x\n").unwrap(),
            format!("{B}\nx\n{E}\n")
        );
        assert_eq!(
            insert(Some("# T\n"), "rig", "x\n").unwrap(),
            format!("# T\n\n{B}\nx\n{E}\n")
        );
        assert_eq!(
            insert(Some("# T"), "rig", "x\n").unwrap(),
            format!("# T\n\n{B}\nx\n{E}\n")
        );
    }

    #[test]
    fn insert_into_crlf_host_stays_crlf() {
        assert_eq!(
            insert(Some("# T\r\n"), "rig", "x\n").unwrap(),
            format!("# T\r\n\r\n{B}\r\nx\r\n{E}\r\n")
        );
    }

    #[test]
    fn find_replace_keeps_outside_bytes() {
        let host = format!("a\r\n{B}\r\nold\r\n{E}\r\nz\r\n");
        let b = find_block(&host, "rig").unwrap().unwrap();
        assert_eq!((b.begin, b.end, b.body.as_str()), (2, 4, "old\n"));
        assert_eq!(
            replace(&host, "rig", &b, "new\n").unwrap(),
            format!("a\r\n{B}\r\nnew\r\n{E}\r\nz\r\n")
        );
    }

    #[test]
    fn replace_with_empty_body_and_lf_host() {
        let host = format!("a\n{B}\nold\nmore\n{E}\n");
        let b = find_block(&host, "rig").unwrap().unwrap();
        assert_eq!(
            replace(&host, "rig", &b, "").unwrap(),
            format!("a\n{B}\n{E}\n")
        );
    }

    #[test]
    fn reworded_begin_tail_still_parses() {
        let host = "<!-- splice:rig:begin — old words -->\nx\n<!-- splice:rig:end -->\n";
        let b = find_block(host, "rig").unwrap().unwrap();
        assert_eq!((b.begin, b.end, b.body.as_str()), (1, 3, "x\n"));
    }

    #[test]
    fn other_channels_ignored() {
        let host = "<!-- splice:decisions:begin — managed by `splice update --channel decisions`; edit the source -->\nx\n<!-- splice:decisions:end -->\n";
        assert_eq!(find_block(host, "rig"), Ok(None));
        assert_eq!(find_block("plain\n", "rig"), Ok(None));
    }

    #[test]
    fn prefix_channel_names_do_not_collide() {
        let host = "<!-- splice:rig2:begin — x -->\nx\n<!-- splice:rig2:end -->\n";
        assert_eq!(find_block(host, "rig"), Ok(None));
    }

    #[test]
    fn duplicate_and_unbalanced() {
        assert_eq!(
            find_block(&format!("{B}\n{E}\n{B}\n{E}\n"), "rig"),
            Err(MarkerError::Duplicate(vec![1, 3]))
        );
        assert_eq!(
            find_block(&format!("x\n{B}\nbody\n"), "rig"),
            Err(MarkerError::Unbalanced(2))
        );
        assert_eq!(
            find_block(&format!("{E}\n"), "rig"),
            Err(MarkerError::Unbalanced(1))
        );
        assert_eq!(
            find_block(&format!("{E}\n{B}\n"), "rig"),
            Err(MarkerError::Unbalanced(1))
        );
    }

    #[test]
    fn error_messages_name_host_and_lines() {
        let m = MarkerError::Duplicate(vec![1, 3]).message("CLAUDE.md", "rig");
        assert!(
            m.contains("CLAUDE.md") && m.contains("rig") && m.contains("1, 3"),
            "{m}"
        );
        let m = MarkerError::Unbalanced(2).message("CLAUDE.md", "rig");
        assert!(m.contains("CLAUDE.md") && m.contains("line 2"), "{m}");
    }

    #[test]
    fn remove_round_trips_insert() {
        for host in ["# T\n", "# T\n\n", "a\r\nb\r\n", "\n"] {
            let with = insert(Some(host), "rig", "x\n").unwrap();
            let b = find_block(&with, "rig").unwrap().unwrap();
            assert_eq!(remove(&with, &b), host);
        }
        let only = insert(None, "rig", "x\n").unwrap();
        let b = find_block(&only, "rig").unwrap().unwrap();
        assert_eq!(remove(&only, &b), "");
    }

    #[test]
    fn sha_ignores_crlf() {
        assert_eq!(body_sha("a\r\nb\r\n"), body_sha("a\nb\n"));
        assert_ne!(body_sha("a\n"), body_sha("b\n"));
        assert_eq!(body_sha("").len(), 64);
    }

    #[test]
    fn remove_after_insert_without_final_newline_gains_the_newline() {
        let with = insert(Some("# T"), "rig", "x\n").unwrap();
        let b = find_block(&with, "rig").unwrap().unwrap();
        assert_eq!(remove(&with, &b), "# T\n");
    }

    #[test]
    fn body_without_final_newline_is_normalised() {
        let ins = insert(None, "rig", "x").unwrap();
        assert_eq!(ins, format!("{B}\nx\n{E}\n"));
        assert!(find_block(&ins, "rig").unwrap().is_some());
        let host = format!("{B}\nold\n{E}\n");
        let b = find_block(&host, "rig").unwrap().unwrap();
        let rep = replace(&host, "rig", &b, "new").unwrap();
        assert_eq!(rep, format!("{B}\nnew\n{E}\n"));
        assert!(find_block(&rep, "rig").unwrap().is_some());
    }

    #[test]
    fn crlf_body_on_crlf_host_is_not_doubled() {
        let host = format!("a\r\n{B}\r\nold\r\n{E}\r\n");
        let b = find_block(&host, "rig").unwrap().unwrap();
        assert_eq!(
            replace(&host, "rig", &b, "new\r\nmore\r\n").unwrap(),
            format!("a\r\n{B}\r\nnew\r\nmore\r\n{E}\r\n")
        );
        assert_eq!(
            insert(Some("a\r\n"), "rig", "x\r\n").unwrap(),
            format!("a\r\n\r\n{B}\r\nx\r\n{E}\r\n")
        );
    }

    #[test]
    fn marker_lines_in_body_are_refused() {
        for body in [
            format!("x\n{E}\ny\n"),
            format!("x\n{B}\n"),
            "<!-- splice:rig:begin -->\n".to_string(),
            format!("x\r\n{E}"),
        ] {
            let err = insert(None, "rig", &body).unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert!(err.to_string().contains("rig"), "{err}");
            let host = format!("{B}\nold\n{E}\n");
            let b = find_block(&host, "rig").unwrap().unwrap();
            assert!(replace(&host, "rig", &b, &body).is_err());
        }
        // other channels' markers and mere mentions are fine
        let ok = "<!-- splice:other:end -->\nsee <!-- splice:rig:end --> inline\n";
        assert!(insert(None, "rig", ok).is_ok());
    }

    #[test]
    fn begin_match_is_exact_and_bom_is_recognised() {
        assert_eq!(
            find_block(&format!("<!-- splice:rig:beginX -->\nx\n{E}\n"), "rig"),
            Err(MarkerError::Unbalanced(3))
        );
        let host = format!("\u{feff}{B}\nx\n{E}\n");
        let b = find_block(&host, "rig").unwrap().unwrap();
        assert_eq!((b.begin, b.end, b.body.as_str()), (1, 3, "x\n"));
        let rep = replace(&host, "rig", &b, "y\n").unwrap();
        assert_eq!(rep, format!("\u{feff}{B}\ny\n{E}\n"));
    }
}
