//! Section markers and block edits for shared host files (channels spec, sections).

use crate::error::SpliceError;

use std::collections::{BTreeMap, BTreeSet};

use crate::filter::Filter;
use crate::lock::SectionLock;
use crate::output::Action;
use crate::planner::Disk;
use crate::render::RenderedSections;

pub struct SectionInput<'a> {
    pub channel: &'a str,
    pub theirs: &'a RenderedSections,
    pub disk: &'a Disk,
    pub locked: &'a BTreeMap<String, SectionLock>,
    pub filter: &'a Filter,
    /// `channel remove`: every locked section is planned as gone.
    pub removing: bool,
    pub force: bool,
}

/// One host-file change. `bytes` is the new host; `None` with `Delete` deletes the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionChange {
    pub host: String,
    pub action: Action,
    pub bytes: Option<Vec<u8>>,
    pub note: Option<String>,
}

impl SectionChange {
    /// `HOST#CHANNEL` (`#` cannot occur in a rendered path).
    pub fn report_path(&self, channel: &str) -> String {
        format!("{}#{channel}", self.host)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionPlan {
    pub changes: Vec<SectionChange>,
    /// `HOST#CHANNEL` of every section edited or deleted locally.
    pub drift: Vec<String>,
    /// Marker problems, with line numbers; `--force` never fixes them.
    pub broken: Vec<String>,
    /// The new `[sections]` table.
    pub locked: BTreeMap<String, SectionLock>,
}

/// Plan every section of one channel (channels spec §6). Pure: reads only `input`.
///
/// Rules: a block whose fragment is unchanged is skipped even if edited (G8); an existing block
/// whose body equals the fragment is adopted; edited or deleted blocks are drift unless `force`;
/// broken markers are never fixed, not even by `force` (G9).
pub fn plan_sections(input: &SectionInput) -> SectionPlan {
    let mut plan = SectionPlan::default();
    let hosts = host_paths(input.theirs, input.locked);
    for host in hosts {
        let path = format!("{host}#{}", input.channel);
        let lk = input.locked.get(&host);
        let want = if input.removing || !input.filter.keeps(&host) {
            None
        } else {
            input.theirs.get(&host)
        };
        let keep_lock = |plan: &mut SectionPlan| {
            if let Some(l) = lk {
                plan.locked.insert(host.clone(), l.clone());
            }
        };
        let disk_bytes = input.disk.files.get(&host).and_then(|o| o.as_ref());
        let disk_text = match disk_bytes.map(|b| std::str::from_utf8(b)) {
            None => None,
            Some(Ok(t)) => Some(t),
            Some(Err(_)) => {
                plan.broken
                    .push(format!("{host}: not valid UTF-8; fix the file by hand"));
                keep_lock(&mut plan);
                continue;
            }
        };
        let block = match disk_text.map_or(Ok(None), |t| find_block(t, input.channel)) {
            Ok(b) => b,
            Err(e) => {
                plan.broken.push(e.message(&host, input.channel));
                keep_lock(&mut plan);
                continue;
            }
        };
        let change = |action, bytes: Option<String>, note| SectionChange {
            host: host.clone(),
            action,
            bytes: bytes.map(String::into_bytes),
            note,
        };
        let Some(sec) = want else {
            // Gone: section removed from the target, filtered out, or `channel remove`.
            let (Some(lk), Some(block), Some(text)) = (lk, &block, disk_text) else {
                continue; // nothing locked, or the markers are already gone
            };
            if body_sha(&block.body) == lk.sha256 {
                let rest = remove(text, block);
                if lk.created && rest.trim().is_empty() {
                    plan.changes.push(change(Action::Delete, None, None));
                } else {
                    plan.changes.push(change(Action::Delete, Some(rest), None));
                }
            } else {
                plan.changes.push(change(
                    Action::Skip,
                    None,
                    Some(format!("{path}: block edited locally; kept")),
                ));
            }
            continue;
        };
        let body = match clean_body(input.channel, &sec.body) {
            Ok(b) => b,
            Err(e) => {
                plan.broken.push(format!("{host}: {e}"));
                keep_lock(&mut plan);
                continue;
            }
        };
        let sha = body_sha(&body);
        let new_lock = |created| SectionLock {
            sha256: sha.clone(),
            created,
        };
        let apply =
            |plan: &mut SectionPlan, r: Result<String, SpliceError>, action, created| match r {
                Ok(text) => {
                    plan.changes.push(change(action, Some(text), None));
                    plan.locked.insert(host.clone(), new_lock(created));
                }
                Err(e) => {
                    plan.broken.push(format!("{host}: {e}"));
                    keep_lock(plan);
                }
            };
        let drift = |plan: &mut SectionPlan| {
            plan.drift.push(path.clone());
            keep_lock(plan);
        };
        match (lk, &block) {
            (None, None) => {
                // Insert: host absent -> created with only the block.
                let r = insert(disk_text, input.channel, &body);
                apply(&mut plan, r, Action::Create, disk_text.is_none());
            }
            (None, Some(b)) => {
                if body_sha(&b.body) == sha {
                    plan.locked.insert(host.clone(), new_lock(false)); // adopt
                } else if input.force {
                    let r = replace(disk_text.unwrap_or_default(), input.channel, b, &body);
                    apply(&mut plan, r, Action::Update, false);
                } else {
                    plan.drift.push(path.clone());
                }
            }
            (Some(_), None) => {
                if input.force {
                    let r = insert(disk_text, input.channel, &body);
                    apply(&mut plan, r, Action::Create, disk_text.is_none());
                } else {
                    drift(&mut plan);
                }
            }
            (Some(l), Some(b)) => {
                if sha == l.sha256 {
                    keep_lock(&mut plan); // G8: fragment unchanged
                } else if input.force || body_sha(&b.body) == l.sha256 {
                    let r = replace(disk_text.unwrap_or_default(), input.channel, b, &body);
                    apply(&mut plan, r, Action::Update, l.created);
                } else {
                    drift(&mut plan);
                }
            }
        }
    }
    plan
}

pub fn host_paths(
    theirs: &RenderedSections,
    locked: &BTreeMap<String, SectionLock>,
) -> BTreeSet<String> {
    theirs.keys().chain(locked.keys()).cloned().collect()
}

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

    mod planner {
        use super::*;
        use crate::render::RenderedSection;

        fn blk(body: &str) -> String {
            format!("{B}\n{body}{E}\n")
        }

        type Case = (
            &'static str,
            Option<&'static str>,
            Option<String>,
            Option<&'static str>,
            bool,
            bool,
            Option<Action>,
            Option<String>,
            Option<bool>,
        );

        fn run(
            theirs_body: Option<&str>,
            disk: Option<&str>,
            locked: Option<(&str, bool)>,
            force: bool,
            filter: &Filter,
            removing: bool,
        ) -> SectionPlan {
            let mut theirs = RenderedSections::new();
            if let Some(b) = theirs_body {
                theirs.insert(
                    "AGENTS.md".into(),
                    RenderedSection {
                        host: "AGENTS.md".into(),
                        body: b.into(),
                        piece_id: "core".into(),
                        piece_name: "core".into(),
                    },
                );
            }
            let mut disk_m = Disk::default();
            disk_m
                .files
                .insert("AGENTS.md".into(), disk.map(|d| d.as_bytes().to_vec()));
            let mut lk = BTreeMap::new();
            if let Some((b, created)) = locked {
                lk.insert(
                    "AGENTS.md".to_string(),
                    SectionLock {
                        sha256: body_sha(b),
                        created,
                    },
                );
            }
            plan_sections(&SectionInput {
                channel: "rig",
                theirs: &theirs,
                disk: &disk_m,
                locked: &lk,
                filter,
                removing,
                force,
            })
        }

        #[test]
        fn table() {
            use Action::*;
            let t = |s: &str| format!("# T\n\n{}", blk(s));
            let cases: Vec<Case> = vec![
                (
                    "insert, host absent",
                    Some("x\n"),
                    None,
                    None,
                    false,
                    false,
                    Some(Create),
                    Some(blk("x\n")),
                    Some(true),
                ),
                (
                    "insert, host present",
                    Some("x\n"),
                    Some("# T\n".into()),
                    None,
                    false,
                    false,
                    Some(Create),
                    Some(t("x\n")),
                    Some(false),
                ),
                (
                    "adopt equal markers",
                    Some("x\n"),
                    Some(blk("x\n")),
                    None,
                    false,
                    false,
                    None,
                    None,
                    Some(false),
                ),
                (
                    "adopt differing markers",
                    Some("x\n"),
                    Some(blk("u\n")),
                    None,
                    false,
                    false,
                    None,
                    None,
                    None,
                ),
                (
                    "replace unedited",
                    Some("y\n"),
                    Some(blk("x\n")),
                    Some("x\n"),
                    false,
                    false,
                    Some(Update),
                    Some(blk("y\n")),
                    Some(false),
                ),
                (
                    "fragment unchanged",
                    Some("x\n"),
                    Some(blk("x\n")),
                    Some("x\n"),
                    false,
                    false,
                    None,
                    None,
                    Some(false),
                ),
                (
                    "unchanged, body edited",
                    Some("x\n"),
                    Some(blk("u\n")),
                    Some("x\n"),
                    false,
                    false,
                    None,
                    None,
                    Some(false),
                ),
                (
                    "drift",
                    Some("y\n"),
                    Some(blk("u\n")),
                    Some("x\n"),
                    false,
                    false,
                    None,
                    None,
                    None,
                ),
                (
                    "drift --force",
                    Some("y\n"),
                    Some(blk("u\n")),
                    Some("x\n"),
                    false,
                    true,
                    Some(Update),
                    Some(blk("y\n")),
                    Some(false),
                ),
                (
                    "markers deleted",
                    Some("y\n"),
                    Some("# T\n".into()),
                    Some("x\n"),
                    false,
                    false,
                    None,
                    None,
                    None,
                ),
                (
                    "markers deleted --force",
                    Some("y\n"),
                    Some("# T\n".into()),
                    Some("x\n"),
                    false,
                    true,
                    Some(Create),
                    Some(t("y\n")),
                    Some(false),
                ),
                (
                    "gone, unedited",
                    None,
                    Some(t("x\n")),
                    Some("x\n"),
                    false,
                    false,
                    Some(Delete),
                    Some("# T\n".into()),
                    None,
                ),
                (
                    "gone, edited",
                    None,
                    Some(blk("u\n")),
                    Some("x\n"),
                    false,
                    false,
                    Some(Skip),
                    None,
                    None,
                ),
                (
                    "gone, created host emptied",
                    None,
                    Some(blk("x\n")),
                    Some("x\n"),
                    true,
                    false,
                    Some(Delete),
                    None,
                    None,
                ),
            ];
            let filter = Filter::new(&[], &[]).unwrap();
            for (name, theirs, disk, locked, created, force, action, after, created_after) in cases
            {
                let plan = run(
                    theirs,
                    disk.as_deref(),
                    locked.map(|b| (b, created)),
                    force,
                    &filter,
                    false,
                );
                assert!(plan.broken.is_empty(), "{name}: {:?}", plan.broken);
                match action {
                    None => assert!(plan.changes.is_empty(), "{name}: {:?}", plan.changes),
                    Some(a) => {
                        assert_eq!(plan.changes.len(), 1, "{name}");
                        let c = &plan.changes[0];
                        assert_eq!(c.action, a, "{name}");
                        assert_eq!(c.bytes, after.map(String::into_bytes), "{name}");
                        assert_eq!(c.report_path("rig"), "AGENTS.md#rig");
                    }
                }
                let drift = action.is_none() && created_after.is_none();
                assert_eq!(
                    plan.drift,
                    if drift {
                        vec!["AGENTS.md#rig".to_string()]
                    } else {
                        vec![]
                    },
                    "{name}"
                );
                let got = plan.locked.get("AGENTS.md");
                // Drift keeps the old lock entry (it still records the last applied body).
                let expected = if drift {
                    locked.map(|_| created)
                } else {
                    created_after
                };
                assert_eq!(got.map(|l| l.created), expected, "{name}");
                if let (Some(l), Some(b), false) = (got, theirs, drift) {
                    assert_eq!(l.sha256, body_sha(b), "{name}");
                }
            }
        }

        #[test]
        fn broken_markers_refused_even_with_force() {
            let filter = Filter::new(&[], &[]).unwrap();
            let host = format!("{B}\n{E}\n{B}\n{E}\n");
            let plan = run(
                Some("y\n"),
                Some(&host),
                Some(("x\n", false)),
                true,
                &filter,
                false,
            );
            assert!(plan.changes.is_empty());
            assert_eq!(plan.broken.len(), 1);
            assert!(plan.broken[0].contains("lines 1, 3"), "{:?}", plan.broken);
            assert!(plan.locked.contains_key("AGENTS.md"), "lock entry kept");
        }

        #[test]
        fn filtered_host_is_removed() {
            let filter = Filter::new(&[], &["AGENTS.md".to_string()]).unwrap();
            let plan = run(
                Some("x\n"),
                Some(&format!("# T\n\n{}", blk("x\n"))),
                Some(("x\n", false)),
                false,
                &filter,
                false,
            );
            assert_eq!(plan.changes.len(), 1);
            assert_eq!(plan.changes[0].action, Action::Delete);
            assert_eq!(plan.changes[0].bytes, Some(b"# T\n".to_vec()));
            assert!(plan.locked.is_empty());
        }

        #[test]
        fn removing_plans_every_locked_section_as_gone() {
            let filter = Filter::new(&[], &[]).unwrap();
            let plan = run(
                None,
                Some(&blk("x\n")),
                Some(("x\n", true)),
                false,
                &filter,
                true,
            );
            assert_eq!(plan.changes[0].action, Action::Delete);
            assert_eq!(plan.changes[0].bytes, None);
            // theirs still lists the section: removing wins
            let plan = run(
                Some("x\n"),
                Some(&blk("x\n")),
                Some(("x\n", true)),
                false,
                &filter,
                true,
            );
            assert_eq!(plan.changes[0].bytes, None);
            assert!(plan.locked.is_empty());
        }

        #[test]
        fn locked_section_with_missing_markers_and_gone_just_drops_the_entry() {
            let filter = Filter::new(&[], &[]).unwrap();
            let plan = run(
                None,
                Some("# T\n"),
                Some(("x\n", false)),
                false,
                &filter,
                false,
            );
            assert!(plan.changes.is_empty() && plan.drift.is_empty() && plan.locked.is_empty());
        }

        #[test]
        fn host_paths_is_theirs_and_locked() {
            let mut theirs = RenderedSections::new();
            theirs.insert(
                "A.md".into(),
                RenderedSection {
                    host: "A.md".into(),
                    body: "x\n".into(),
                    piece_id: "p".into(),
                    piece_name: "p".into(),
                },
            );
            let mut lk = BTreeMap::new();
            lk.insert(
                "B.md".to_string(),
                SectionLock {
                    sha256: "0".repeat(64),
                    created: false,
                },
            );
            assert_eq!(
                host_paths(&theirs, &lk).into_iter().collect::<Vec<_>>(),
                ["A.md", "B.md"]
            );
        }
    }
}
