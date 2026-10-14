//! `splice diff` (spec §8.8): steps 2-6 of `update` without writing anything.

use super::{parse_to, usage};
use crate::cli::{DiffArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::fsio::read_opt;
use crate::ops::update::prepare;
use crate::output::{Action, Report};
use crate::planner::Planned;
use indexmap::IndexMap;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

/// The unified patch of one planned write, `a/` and `b/` paths (`/dev/null` for a side that does
/// not exist). Non-UTF-8 content is reported as binary.
fn patch_lines(repo: &Path, c: &Planned, new: &[u8]) -> Result<Vec<String>> {
    let old = read_opt(repo, &c.path)?;
    let (Ok(o), Ok(n)) = (
        std::str::from_utf8(old.as_deref().unwrap_or_default()),
        std::str::from_utf8(new),
    ) else {
        let side = |s: &str, exists: bool| {
            if exists {
                format!("{s}/{}", c.path)
            } else {
                "/dev/null".to_string()
            }
        };
        return Ok(vec![format!(
            "Binary files {} and {} differ",
            side("a", old.is_some()),
            side("b", true)
        )]);
    };
    let name = |side: &str, exists: bool| {
        if exists {
            format!("{side}/{}", c.path)
        } else {
            "/dev/null".to_string()
        }
    };
    let patch = diffy::DiffOptions::new()
        .set_original_filename(name("a", old.is_some()))
        .set_modified_filename(name("b", true))
        .create_patch(o, n);
    Ok(patch
        .to_string()
        .split_terminator('\n')
        .map(str::to_string)
        .collect())
}

pub fn run(g: &GlobalArgs, a: &DiffArgs) -> Result<Report> {
    let repo = g.dir.as_path();
    let to = parse_to(a.to.as_deref())?;

    // never prompts and never forces: new vars take their default, and owned drift is reported
    // rather than planned as an update
    let p = prepare(g, to.as_ref(), &IndexMap::new(), false, false).map_err(|e| match e {
        SpliceError::Usage(m) if m.starts_with(crate::vars::MISSING_VALUE) => usage(format!(
            "{m} (a var that is new in the target and has no default; `splice diff` does not ask: \
             run `splice update --data NAME=VALUE`)"
        )),
        e => e,
    })?;

    let mut report = Report::new("diff");
    report.messages.extend(p.messages.iter().cloned());
    for d in &p.plan.drift {
        report.msg(format!(
            "{d}: owned file was edited or deleted locally; update would refuse (--force replaces it)"
        ));
    }
    for m in &p.plan.marked {
        report.msg(format!(
            "resolve conflict markers in {m} first (splice check); update would refuse"
        ));
    }
    for c in &p.plan.changes {
        match (&c.bytes, c.action) {
            (Some(bytes), _) => report.lines.extend(patch_lines(repo, c, bytes)?),
            (None, Action::Delete) => report.lines.push(format!("delete   {}", c.path)),
            _ => {}
        }
        if let Some(n) = &c.note {
            report.msg(format!("{}: {n}", c.path));
        }
        report.push(c.path.clone(), c.action, c.class);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Class;

    fn planned(path: &str) -> Planned {
        Planned {
            path: path.into(),
            action: Action::Update,
            class: Class::Owned,
            bytes: None,
            note: None,
        }
    }

    #[test]
    fn created_file_diffs_against_dev_null() {
        let d = tempfile::tempdir().unwrap();
        let l = patch_lines(d.path(), &planned("n.txt"), b"hi\n").unwrap();
        assert_eq!(l[0], "--- /dev/null");
        assert_eq!(l[1], "+++ b/n.txt");
        assert!(l.contains(&"+hi".to_string()), "{l:?}");
    }

    #[test]
    fn changed_file_uses_a_and_b_paths_and_keeps_crlf() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("f.txt"), "a\r\nb\r\n").unwrap();
        let l = patch_lines(d.path(), &planned("f.txt"), b"a\r\nc\r\n").unwrap();
        assert_eq!(l[0], "--- a/f.txt");
        assert!(l.contains(&"-b\r".to_string()), "{l:?}");
        assert!(l.contains(&"+c\r".to_string()), "{l:?}");
    }

    #[test]
    fn binary_content_is_reported_not_diffed() {
        let d = tempfile::tempdir().unwrap();
        let l = patch_lines(d.path(), &planned("b.bin"), &[0xff, 0xfe]).unwrap();
        assert_eq!(l, ["Binary files /dev/null and b/b.bin differ"]);
        std::fs::write(d.path().join("b.bin"), [0xff]).unwrap();
        let l = patch_lines(d.path(), &planned("b.bin"), &[0xfe]).unwrap();
        assert_eq!(l, ["Binary files a/b.bin and b/b.bin differ"]);
    }
}
