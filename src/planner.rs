//! The update planner (spec §8.4): base, theirs, disk and lock in, a list of changes out. Pure:
//! the disk is read into a [`Disk`] snapshot before planning, and nothing is written here.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

use indexmap::IndexMap;

use crate::error::SpliceError;
use crate::fsio::{self, sha256_hex};
use crate::lock::{LOCK_PATH, Lock};
use crate::manifest::Template;
use crate::merge::{merge3, wrap_whole};
use crate::migrate::Remove;
use crate::output::{Action, Class};
use crate::paths::render_segments;
use crate::render::{PieceInstance, RenderedFile, RenderedTree, Renderer};

type Result<T> = std::result::Result<T, SpliceError>;

/// The bytes on disk for every path the planner looks at; `None` when the file does not exist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Disk {
    pub files: BTreeMap<String, Option<Vec<u8>>>,
}

impl Disk {
    /// Read `paths` below `repo`. A path that is absolute, contains `..` or crosses a symlink is an
    /// error (the same checks as every write), so nothing outside the repo is ever read.
    pub fn snapshot<'a>(repo: &Path, paths: impl Iterator<Item = &'a str>) -> io::Result<Disk> {
        let mut files = BTreeMap::new();
        for path in paths {
            if files.contains_key(path) {
                continue;
            }
            fsio::check_target(repo, path).map_err(|e| match e {
                SpliceError::Io(e) => e,
                other => io::Error::other(other.to_string()),
            })?;
            files.insert(path.to_string(), fsio::read_opt(repo, path)?);
        }
        Ok(Disk { files })
    }
}

/// Everything [`plan`] needs. `disk` must cover every path of `base`, `theirs` and `removes`
/// (a path missing from it counts as absent). `removes` are the rendered `[[remove]]` paths of
/// the selected migrations ([`render_removes`]).
pub struct PlanInput<'a> {
    pub base: &'a RenderedTree,
    pub theirs: &'a RenderedTree,
    pub disk: &'a Disk,
    pub lock: &'a Lock,
    pub removes: Vec<String>,
    pub force: bool,
}

/// One planned change. `bytes` is set when the file is written (`create`, `update`, `merge`, and
/// `conflict` with diff3 markers). `note` becomes a report message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub path: String,
    pub action: Action,
    pub class: Class,
    pub bytes: Option<Vec<u8>>,
    pub note: Option<String>,
}

/// The plan: changes sorted by path; `drift` lists owned files edited (or deleted) locally,
/// which get no change; `conflicts` lists the paths of `conflict` changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub changes: Vec<Planned>,
    pub drift: Vec<String>,
    pub conflicts: Vec<String>,
}

/// What one path needs.
enum Step {
    Nothing,
    Change(Planned),
    Drift,
}

fn change(path: &str, action: Action, class: Class, bytes: Option<&[u8]>) -> Step {
    Step::Change(Planned {
        path: path.to_string(),
        action,
        class,
        bytes: bytes.map(<[u8]>::to_vec),
        note: None,
    })
}

fn noted(path: &str, action: Action, class: Class, bytes: Option<&[u8]>, note: String) -> Step {
    Step::Change(Planned {
        path: path.to_string(),
        action,
        class,
        bytes: bytes.map(<[u8]>::to_vec),
        note: Some(note),
    })
}

/// Plan an update per spec §8.4. Per path: base B (render at the lock), theirs T (target render),
/// ours O (disk) and the lock entry L; O is "unedited" when its sha256 equals L's.
///
/// - **owned:** unedited → T; edited or deleted → drift (`force`: T); new in T → created when
///   absent, drift when a different file exists (`force`: T).
/// - **shared:** O = B → T; T = B → O kept; else `merge3` → `merge` or `conflict` (markers); a
///   user-made file merges against an empty base; deleted locally → stays deleted (`skip`).
/// - **seed:** created only when new in T and absent; never modified or deleted.
/// - **gone from T or `[[remove]]`:** deleted when unedited (or equal to B), else kept (`skip`).
///   A `[[remove]]` path that T still renders is not removed (warning note).
/// - **class changes:** owned/shared → seed: left as is and dropped from the lock (`skip`);
///   seed → owned/shared: created when absent, a `conflict` when a different file exists;
///   owned ↔ shared: the rules of T's class.
pub fn plan(input: &PlanInput) -> Plan {
    let removes: BTreeSet<&str> = input.removes.iter().map(String::as_str).collect();
    let paths: BTreeSet<&str> = input
        .base
        .keys()
        .chain(input.theirs.keys())
        .map(String::as_str)
        .chain(removes.iter().copied())
        .collect();
    let mut out = Plan::default();
    for path in paths {
        let ours = input.disk.files.get(path).and_then(|o| o.as_deref());
        let base = input.base.get(path);
        let mut step = match input.theirs.get(path) {
            Some(theirs) => rendered(input, path, base, theirs, ours),
            None => gone(input, path, base, ours),
        };
        if let Some(theirs) = input.theirs.get(path).filter(|_| removes.contains(path)) {
            let warn = format!(
                "`{path}` is listed in a [[remove]] but the template still renders it; not removed"
            );
            step = match step {
                Step::Change(mut c) => {
                    c.note = Some(match c.note {
                        Some(n) => format!("{n}; {warn}"),
                        None => warn,
                    });
                    Step::Change(c)
                }
                Step::Nothing => noted(path, Action::Skip, theirs.class, None, warn),
                // The drift report names the file already.
                Step::Drift => Step::Drift,
            };
        }
        match step {
            Step::Nothing => {}
            Step::Drift => out.drift.push(path.to_string()),
            Step::Change(c) => {
                if c.action == Action::Conflict {
                    out.conflicts.push(path.to_string());
                }
                out.changes.push(c);
            }
        }
    }
    out
}

fn unedited(input: &PlanInput, path: &str, ours: &[u8]) -> bool {
    input
        .lock
        .files
        .get(path)
        .is_some_and(|l| l.sha256 == sha256_hex(ours))
}

/// A path that `theirs` renders.
fn rendered(
    input: &PlanInput,
    path: &str,
    base: Option<&RenderedFile>,
    theirs: &RenderedFile,
    ours: Option<&[u8]>,
) -> Step {
    let t = theirs.bytes.as_slice();
    let class = theirs.class;
    match (base.map(|b| b.class), class) {
        (Some(from), Class::Seed) if from != Class::Seed => {
            return match ours {
                None => Step::Nothing,
                Some(_) => noted(
                    path,
                    Action::Skip,
                    class,
                    None,
                    format!(
                        "`{path}` is a seed file in the new version: left as is and no longer \
                         tracked in the lock"
                    ),
                ),
            };
        }
        (Some(Class::Seed), to) if to != Class::Seed => {
            return match ours {
                None => change(path, Action::Create, class, Some(t)),
                Some(o) if o == t => Step::Nothing,
                Some(o) => whole_conflict(path, class, o, t),
            };
        }
        _ => {}
    }
    match class {
        Class::Seed => match (base, ours) {
            (None, None) => change(path, Action::Create, class, Some(t)),
            _ => Step::Nothing,
        },
        Class::Owned => match ours {
            Some(o) if o == t => Step::Nothing,
            Some(o) if unedited(input, path, o) => {
                if base.is_some_and(|b| b.bytes == t) {
                    Step::Nothing
                } else {
                    change(path, Action::Update, class, Some(t))
                }
            }
            Some(_) if input.force => change(path, Action::Update, class, Some(t)),
            None if base.is_none() || input.force => change(path, Action::Create, class, Some(t)),
            _ => Step::Drift,
        },
        Class::Shared => shared(path, base, theirs, ours),
    }
}

fn shared(
    path: &str,
    base: Option<&RenderedFile>,
    theirs: &RenderedFile,
    ours: Option<&[u8]>,
) -> Step {
    let (t, class) = (theirs.bytes.as_slice(), theirs.class);
    let Some(o) = ours else {
        return match base {
            None => change(path, Action::Create, class, Some(t)),
            Some(_) => noted(
                path,
                Action::Skip,
                class,
                None,
                format!(
                    "`{path}` was deleted locally; kept deleted (the lock records the template's version)"
                ),
            ),
        };
    };
    let b = base.map_or(&[][..], |b| b.bytes.as_slice());
    if o == t || (base.is_some() && b == t) {
        return Step::Nothing;
    }
    if base.is_some() && o == b {
        return change(path, Action::Update, class, Some(t));
    }
    let (Ok(bs), Ok(os), Ok(ts)) = (
        std::str::from_utf8(b),
        std::str::from_utf8(o),
        std::str::from_utf8(t),
    ) else {
        return not_utf8(path, class);
    };
    let m = merge3(theirs.strategy, bs, os, ts);
    if m.conflicts.is_empty() {
        if m.text.as_bytes() == o {
            Step::Nothing
        } else {
            change(path, Action::Merge, class, Some(m.text.as_bytes()))
        }
    } else {
        noted(
            path,
            Action::Conflict,
            class,
            Some(m.text.as_bytes()),
            format!(
                "`{path}` has conflicts ({}); resolve the diff3 markers",
                m.conflicts.join(", ")
            ),
        )
    }
}

/// A file the user has where the template now ships a different one: the whole file in one diff3
/// block (ours, empty base, theirs), so nothing is lost and `check` finds it.
fn whole_conflict(path: &str, class: Class, ours: &[u8], theirs: &[u8]) -> Step {
    let (Ok(o), Ok(t)) = (std::str::from_utf8(ours), std::str::from_utf8(theirs)) else {
        return not_utf8(path, class);
    };
    noted(
        path,
        Action::Conflict,
        class,
        Some(wrap_whole("", o, t).as_bytes()),
        format!(
            "`{path}` was a seed file and is now {}: resolve the diff3 markers",
            class.as_str()
        ),
    )
}

fn not_utf8(path: &str, class: Class) -> Step {
    noted(
        path,
        Action::Conflict,
        class,
        None,
        format!("`{path}` is not UTF-8 and cannot be merged; kept as is, merge by hand"),
    )
}

/// A path that `theirs` no longer renders: in `base` only, or a `[[remove]]` path.
fn gone(input: &PlanInput, path: &str, base: Option<&RenderedFile>, ours: Option<&[u8]>) -> Step {
    let class = base
        .map(|b| b.class)
        .or_else(|| input.lock.files.get(path).map(|l| l.class));
    let Some(o) = ours else {
        return Step::Nothing;
    };
    match class {
        // Seeds are never deleted.
        Some(Class::Seed) => Step::Nothing,
        Some(class) if unedited(input, path, o) || base.is_some_and(|b| b.bytes == o) => {
            change(path, Action::Delete, class, None)
        }
        Some(class) => noted(
            path,
            Action::Skip,
            class,
            None,
            format!("`{path}` is no longer shipped by the template but was edited locally; kept"),
        ),
        None => noted(
            path,
            Action::Skip,
            Class::Owned,
            None,
            format!("`{path}` is listed in a [[remove]] but is not tracked in the lock; kept"),
        ),
    }
}

/// Render `[[remove]]` paths (spec §8.4): with each instance of `piece`, or with the template
/// `vars` only when `piece` is absent. Repo-root relative, deduplicated, in order. An unknown
/// `piece`, an unsafe path or `.splice/lock.toml` is a template error.
pub fn render_removes(
    t: &Template,
    removes: &[&Remove],
    instances: &[PieceInstance],
    vars: &IndexMap<String, String>,
) -> Result<Vec<String>> {
    let renderer = Renderer::new();
    let mut pieces: Vec<&str> = Vec::new();
    for inst in instances {
        if !pieces.contains(&inst.id.as_str()) {
            pieces.push(&inst.id);
        }
    }
    let mut out: Vec<String> = Vec::new();
    for r in removes {
        let origin = format!("[[remove]] `{}`", r.path);
        let contexts: Vec<minijinja::Value> = match &r.piece {
            None => vec![minijinja::Value::from_serialize(vars)],
            Some(id) => {
                if !t.pieces.contains_key(id) {
                    return Err(SpliceError::Template(format!(
                        "{origin}: unknown piece `{id}`"
                    )));
                }
                instances
                    .iter()
                    .filter(|i| i.id == *id)
                    .map(|i| instance_context(vars, i, &pieces))
                    .collect()
            }
        };
        for ctx in contexts {
            let path = render_segments(&r.path, &renderer, &ctx, &origin)?.join("/");
            if path == LOCK_PATH {
                return Err(SpliceError::Template(format!(
                    "{origin}: `{LOCK_PATH}` is reserved"
                )));
            }
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    Ok(out)
}

/// The variables of one instance, as for its files: template vars, instance vars, `name`,
/// `pieces` and `piece`.
fn instance_context(
    vars: &IndexMap<String, String>,
    inst: &PieceInstance,
    pieces: &[&str],
) -> minijinja::Value {
    let mut ctx: IndexMap<&str, minijinja::Value> = IndexMap::new();
    for (k, v) in vars.iter().chain(inst.vars.iter()) {
        ctx.insert(k, minijinja::Value::from(v.as_str()));
    }
    ctx.insert("name", minijinja::Value::from(inst.name.as_str()));
    ctx.insert("pieces", minijinja::Value::from_serialize(pieces));
    ctx.insert(
        "piece",
        minijinja::context! { id => inst.id.as_str(), name => inst.name.as_str() },
    );
    minijinja::Value::from_serialize(&ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsio::sha256_hex;
    use crate::lock::{LockFile, LockTemplate};
    use crate::manifest::MergeStrategy;
    use crate::render::RenderedFile;

    const P: &str = "f.txt";

    fn file(path: &str, bytes: &[u8], class: Class) -> RenderedFile {
        RenderedFile {
            path: path.into(),
            bytes: bytes.to_vec(),
            class,
            strategy: MergeStrategy::Text,
            piece_id: "p".into(),
            piece_name: "p".into(),
            glob: None,
        }
    }

    fn tree(files: &[RenderedFile]) -> RenderedTree {
        files.iter().map(|f| (f.path.clone(), f.clone())).collect()
    }

    fn lock(files: &[(&str, Class, &[u8])]) -> Lock {
        Lock {
            format: 1,
            splice: semver::Version::new(0, 1, 0),
            template: LockTemplate {
                source: "x".into(),
                version: semver::Version::new(1, 0, 0),
                commit: "0".repeat(40),
            },
            vars: IndexMap::new(),
            pieces: Vec::new(),
            files: files
                .iter()
                .map(|(p, class, b)| {
                    (
                        p.to_string(),
                        LockFile {
                            class: *class,
                            sha256: sha256_hex(b),
                        },
                    )
                })
                .collect(),
        }
    }

    fn disk(files: &[(&str, Option<&[u8]>)]) -> Disk {
        Disk {
            files: files
                .iter()
                .map(|(p, b)| (p.to_string(), b.map(<[u8]>::to_vec)))
                .collect(),
        }
    }

    struct Case<'a> {
        base: Vec<RenderedFile>,
        theirs: Vec<RenderedFile>,
        disk: Vec<(&'a str, Option<&'a [u8]>)>,
        lock: Vec<(&'a str, Class, &'a [u8])>,
        removes: Vec<String>,
        force: bool,
    }

    fn run(c: &Case) -> Plan {
        let (base, theirs) = (tree(&c.base), tree(&c.theirs));
        let (disk, lock) = (disk(&c.disk), lock(&c.lock));
        plan(&PlanInput {
            base: &base,
            theirs: &theirs,
            disk: &disk,
            lock: &lock,
            removes: c.removes.clone(),
            force: c.force,
        })
    }

    /// One path with the same class in base and theirs.
    fn one(
        class: Class,
        base: Option<&str>,
        theirs: Option<&str>,
        on_disk: Option<&str>,
        locked: Option<&str>,
        force: bool,
    ) -> Plan {
        run(&Case {
            base: base
                .map(|b| file(P, b.as_bytes(), class))
                .into_iter()
                .collect(),
            theirs: theirs
                .map(|t| file(P, t.as_bytes(), class))
                .into_iter()
                .collect(),
            disk: vec![(P, on_disk.map(str::as_bytes))],
            lock: locked
                .map(|l| (P, class, l.as_bytes()))
                .into_iter()
                .collect(),
            removes: Vec::new(),
            force,
        })
    }

    /// Cases whose path must be listed in `Plan.drift` (and so have no change).
    const DRIFT: [&str; 3] = [
        "owned drift",
        "owned deleted by user",
        "owned new, user file",
    ];

    type Row = (
        &'static str,
        Class,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
        bool,
        Option<Action>,
        Option<&'static str>,
    );

    fn check_case(c: Row) {
        let (name, class, base, theirs, on_disk, locked, force, action, bytes) = c;
        let p = one(class, base, theirs, on_disk, locked, force);
        let in_drift = DRIFT.contains(&name);
        assert_eq!(
            p.drift,
            if in_drift {
                vec![P.to_string()]
            } else {
                vec![]
            },
            "{name}: drift"
        );
        let Some(action) = action else {
            assert!(
                p.changes.is_empty(),
                "{name}: expected no change, got {:?}",
                p.changes
            );
            assert!(p.conflicts.is_empty(), "{name}");
            return;
        };
        assert_eq!(p.changes.len(), 1, "{name}: {:?}", p.changes);
        let ch = &p.changes[0];
        assert_eq!(
            (ch.path.as_str(), ch.action, ch.class),
            (P, action, class),
            "{name}"
        );
        if action == Action::Conflict {
            assert_eq!(p.conflicts, vec![P.to_string()], "{name}");
            let text = String::from_utf8(ch.bytes.clone().expect("markers")).unwrap();
            assert!(text.contains("<<<<<<< ours\n"), "{name}: {text}");
            assert!(text.contains(">>>>>>> template\n"), "{name}: {text}");
        } else {
            assert!(p.conflicts.is_empty(), "{name}");
            assert_eq!(
                ch.bytes.as_deref(),
                bytes.map(str::as_bytes),
                "{name}: bytes"
            );
        }
        if action == Action::Skip {
            assert!(ch.note.is_some(), "{name}: a skip says why");
        }
    }

    #[test]
    #[rustfmt::skip]
    fn table() {
        use Action::*;
        use Class::*;
        // (name, class, base, theirs, disk, lock_sha_of, force, expected_action, expected_bytes)
        let cases: [Row; 19] = [
            ("owned unedited changed",  Owned,  Some("a"), Some("b"), Some("a"), Some("a"), false, Some(Update), Some("b")),
            ("owned unchanged",         Owned,  Some("a"), Some("a"), Some("a"), Some("a"), false, None,         None),
            ("owned drift",             Owned,  Some("a"), Some("b"), Some("x"), Some("a"), false, None /* in drift */, None),
            ("owned drift --force",     Owned,  Some("a"), Some("b"), Some("x"), Some("a"), true,  Some(Update), Some("b")),
            ("owned deleted by user",   Owned,  Some("a"), Some("b"), None,      Some("a"), false, None /* in drift */, None),
            ("owned new, absent",       Owned,  None,      Some("b"), None,      None,      false, Some(Create), Some("b")),
            ("owned new, user file",    Owned,  None,      Some("b"), Some("u"), None,      false, None /* in drift */, None),
            ("shared ours==base",       Shared, Some("a\n"), Some("b\n"), Some("a\n"), Some("a\n"), false, Some(Update), Some("b\n")),
            ("shared theirs==base",     Shared, Some("a\n"), Some("a\n"), Some("u\n"), Some("a\n"), false, None,   None),
            ("shared merge",            Shared, Some("1\n2\n3\n"), Some("1\n2\nT\n"), Some("U\n2\n3\n"), Some("1\n2\n3\n"), false, Some(Merge), Some("U\n2\nT\n")),
            ("shared conflict",         Shared, Some("x\n"), Some("t\n"), Some("u\n"), Some("x\n"), false, Some(Conflict), None /* has markers */),
            ("shared deleted by user",  Shared, Some("a\n"), Some("b\n"), None,      Some("a\n"), false, Some(Skip), None),
            ("shared new, user file",   Shared, None,      Some("t\n"), Some("u\n"), None,      false, Some(Conflict), None),
            ("seed exists",             Seed,   Some("a"), Some("b"), Some("u"), None,      false, None,         None),
            ("seed new, absent",        Seed,   None,      Some("b"), None,      None,      false, Some(Create), Some("b")),
            ("owned gone, unedited",    Owned,  Some("a"), None,      Some("a"), Some("a"), false, Some(Delete), None),
            ("owned gone, edited",      Owned,  Some("a"), None,      Some("x"), Some("a"), false, Some(Skip),   None),
            ("shared gone, edited",     Shared, Some("a"), None,      Some("x"), Some("a"), false, Some(Skip),   None),
            ("seed gone",               Seed,   Some("a"), None,      Some("a"), None,      false, None,         None),
        ];
        for c in cases {
            check_case(c);
        }
    }

    #[test]
    #[rustfmt::skip]
    fn more_cells() {
        use Action::*;
        use Class::*;
        let cases: [Row; 12] = [
            // `--force` recreates a deleted owned file and replaces a user-made one.
            ("owned deleted, --force",  Owned,  Some("a"), Some("b"), None,      Some("a"), true,  Some(Create), Some("b")),
            ("owned new, user, --force",Owned,  None,      Some("b"), Some("u"), None,      true,  Some(Update), Some("b")),
            // A user file that already equals the render needs nothing.
            ("owned new, same file",    Owned,  None,      Some("b"), Some("b"), None,      false, None,         None),
            ("owned edited to theirs",  Owned,  Some("a"), Some("b"), Some("b"), Some("a"), false, None,         None),
            // Edited owned files drift even when the template did not change them (spec §8.4).
            ("owned drift",             Owned,  Some("a"), Some("a"), Some("x"), Some("a"), false, None,         None),
            // `--force` never touches shared files.
            ("shared deleted, --force", Shared, Some("a\n"), Some("b\n"), None,  Some("a\n"), true, Some(Skip),  None),
            ("shared new, absent",      Shared, None,      Some("t\n"), None,    None,      false, Some(Create), Some("t\n")),
            ("shared new, same file",   Shared, None,      Some("t\n"), Some("t\n"), None,  false, None,         None),
            ("shared gone, unedited",   Shared, Some("a"), None,      Some("a"), Some("a"), false, Some(Delete), None),
            ("shared gone, deleted",    Shared, Some("a"), None,      None,      Some("a"), false, None,         None),
            ("owned gone, deleted",     Owned,  Some("a"), None,      None,      Some("a"), false, None,         None),
            ("seed new, user file",     Seed,   None,      Some("b"), Some("u"), None,      false, None,         None),
        ];
        for c in cases {
            check_case(c);
        }
    }

    #[test]
    fn edited_gone_files_say_they_are_kept() {
        let p = one(Class::Owned, Some("a"), None, Some("x"), Some("a"), true);
        assert_eq!(
            p.changes[0].action,
            Action::Skip,
            "force never deletes edits"
        );
        let note = p.changes[0].note.as_deref().unwrap();
        assert!(note.contains(P) && note.contains("kept"), "{note}");
    }

    #[test]
    fn shared_merge_uses_the_strategy() {
        let json = |s: &str| {
            let mut f = file("p.json", s.as_bytes(), Class::Shared);
            f.strategy = MergeStrategy::Json;
            f
        };
        let base = "{\n  \"a\": 1\n}\n";
        let ours = "{\n  \"a\": 1,\n  \"mine\": true\n}\n";
        let theirs = "{\n  \"a\": 2\n}\n";
        let p = run(&Case {
            base: vec![json(base)],
            theirs: vec![json(theirs)],
            disk: vec![("p.json", Some(ours.as_bytes()))],
            lock: vec![("p.json", Class::Shared, base.as_bytes())],
            removes: Vec::new(),
            force: false,
        });
        assert_eq!(p.changes[0].action, Action::Merge);
        let v: serde_json::Value =
            serde_json::from_slice(p.changes[0].bytes.as_ref().unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"a": 2, "mine": true}));
    }

    #[test]
    fn shared_non_utf8_on_disk_is_a_conflict_that_keeps_the_file() {
        let p = run(&Case {
            base: vec![file(P, b"a\n", Class::Shared)],
            theirs: vec![file(P, b"b\n", Class::Shared)],
            disk: vec![(P, Some(b"\xff\xfe"))],
            lock: vec![(P, Class::Shared, b"a\n")],
            removes: Vec::new(),
            force: false,
        });
        assert_eq!(p.changes[0].action, Action::Conflict);
        assert!(
            p.changes[0].bytes.is_none(),
            "the user's bytes are never replaced"
        );
        assert!(p.changes[0].note.as_deref().unwrap().contains("UTF-8"));
        assert_eq!(p.conflicts, vec![P.to_string()]);
    }

    #[test]
    fn remove_list_deletes_unedited_and_ignores_still_rendered() {
        let p = run(&Case {
            base: vec![
                file("old.txt", b"o", Class::Owned),
                file("kept.txt", b"k", Class::Owned),
                file("seed.txt", b"s", Class::Seed),
            ],
            theirs: vec![
                file("kept.txt", b"k", Class::Owned),
                file("seed.txt", b"s", Class::Seed),
            ],
            disk: vec![
                ("old.txt", Some(b"o")),
                ("kept.txt", Some(b"k")),
                ("seed.txt", Some(b"s")),
                ("legacy.txt", Some(b"l")),
                ("legacy-edited.txt", Some(b"mine")),
                ("untracked.txt", Some(b"u")),
                ("absent.txt", None),
            ],
            lock: vec![
                ("old.txt", Class::Owned, b"o"),
                ("kept.txt", Class::Owned, b"k"),
                ("legacy.txt", Class::Owned, b"l"),
                ("legacy-edited.txt", Class::Shared, b"l"),
            ],
            removes: [
                "old.txt",
                "kept.txt",
                "seed.txt",
                "legacy.txt",
                "legacy-edited.txt",
                "untracked.txt",
                "absent.txt",
            ]
            .map(String::from)
            .to_vec(),
            force: false,
        });
        let got: Vec<(&str, Action, Class)> = p
            .changes
            .iter()
            .map(|c| (c.path.as_str(), c.action, c.class))
            .collect();
        assert_eq!(
            got,
            vec![
                ("kept.txt", Action::Skip, Class::Owned),
                ("legacy-edited.txt", Action::Skip, Class::Shared),
                ("legacy.txt", Action::Delete, Class::Owned),
                ("old.txt", Action::Delete, Class::Owned),
                ("seed.txt", Action::Skip, Class::Seed),
                ("untracked.txt", Action::Skip, Class::Owned),
            ]
        );
        let note = |path: &str| {
            p.changes
                .iter()
                .find(|c| c.path == path)
                .and_then(|c| c.note.clone())
                .unwrap()
        };
        assert!(
            note("kept.txt").contains("still renders"),
            "{}",
            note("kept.txt")
        );
        assert!(note("legacy-edited.txt").contains("kept"));
        assert!(note("untracked.txt").contains("kept"));
        assert!(p.drift.is_empty() && p.conflicts.is_empty());
    }

    #[test]
    fn remove_of_a_still_rendered_changed_file_keeps_the_change_and_warns() {
        let p = run(&Case {
            base: vec![file(P, b"a", Class::Owned)],
            theirs: vec![file(P, b"b", Class::Owned)],
            disk: vec![(P, Some(b"a"))],
            lock: vec![(P, Class::Owned, b"a")],
            removes: vec![P.to_string()],
            force: false,
        });
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes[0].action, Action::Update);
        assert!(
            p.changes[0]
                .note
                .as_deref()
                .unwrap()
                .contains("still renders")
        );
    }

    #[test]
    fn owned_to_seed_leaves_the_file_and_says_so() {
        for on_disk in [b"a".as_slice(), b"edited"] {
            let p = run(&Case {
                base: vec![file(P, b"a", Class::Owned)],
                theirs: vec![file(P, b"b", Class::Seed)],
                disk: vec![(P, Some(on_disk))],
                lock: vec![(P, Class::Owned, b"a")],
                removes: Vec::new(),
                force: false,
            });
            assert_eq!(p.changes.len(), 1);
            let ch = &p.changes[0];
            assert_eq!((ch.action, ch.class), (Action::Skip, Class::Seed));
            assert!(ch.bytes.is_none());
            assert!(ch.note.as_deref().unwrap().contains("seed"));
            assert!(p.drift.is_empty() && p.conflicts.is_empty());
        }
        // Deleted locally: stays deleted, nothing to say.
        let p = run(&Case {
            base: vec![file(P, b"a", Class::Shared)],
            theirs: vec![file(P, b"b", Class::Seed)],
            disk: vec![(P, None)],
            lock: vec![(P, Class::Shared, b"a")],
            removes: Vec::new(),
            force: false,
        });
        assert_eq!(p, Plan::default());
    }

    #[test]
    fn seed_to_owned_creates_or_conflicts() {
        let case = |on_disk: Option<&'static [u8]>, to: Class| {
            run(&Case {
                base: vec![file(P, b"a\n", Class::Seed)],
                theirs: vec![file(P, b"b\n", to)],
                disk: vec![(P, on_disk)],
                lock: Vec::new(),
                removes: Vec::new(),
                force: true,
            })
        };
        let created = case(None, Class::Owned);
        assert_eq!(created.changes[0].action, Action::Create);
        assert_eq!(created.changes[0].bytes.as_deref(), Some(b"b\n".as_slice()));
        assert_eq!(case(Some(b"b\n"), Class::Owned), Plan::default());
        // Present and different: a conflict with markers, even with `--force`; never drift.
        for to in [Class::Owned, Class::Shared] {
            let p = case(Some(b"mine\n"), to);
            assert!(p.drift.is_empty());
            assert_eq!(p.conflicts, vec![P.to_string()]);
            let ch = &p.changes[0];
            assert_eq!((ch.action, ch.class), (Action::Conflict, to));
            let text = String::from_utf8(ch.bytes.clone().unwrap()).unwrap();
            assert!(
                text.contains("mine\n") && text.contains("<<<<<<< ours\n"),
                "{text}"
            );
        }
    }

    #[test]
    fn owned_to_shared_uses_the_shared_rules() {
        let p = run(&Case {
            base: vec![file(P, b"1\n2\n3\n", Class::Owned)],
            theirs: vec![file(P, b"1\n2\nT\n", Class::Shared)],
            disk: vec![(P, Some(b"U\n2\n3\n"))],
            lock: vec![(P, Class::Owned, b"1\n2\n3\n")],
            removes: Vec::new(),
            force: false,
        });
        assert!(p.drift.is_empty());
        assert_eq!(p.changes[0].action, Action::Merge);
        assert_eq!(p.changes[0].bytes.as_deref(), Some(b"U\n2\nT\n".as_slice()));
    }

    #[test]
    fn changes_are_sorted_and_drift_lists_every_edited_owned_file() {
        let p = run(&Case {
            base: vec![file("b", b"1", Class::Owned), file("a", b"1", Class::Owned)],
            theirs: vec![
                file("b", b"2", Class::Owned),
                file("a", b"2", Class::Owned),
                file("c", b"3", Class::Owned),
            ],
            disk: vec![("a", Some(b"x")), ("b", Some(b"y")), ("c", None)],
            lock: vec![("a", Class::Owned, b"1"), ("b", Class::Owned, b"1")],
            removes: Vec::new(),
            force: false,
        });
        assert_eq!(p.drift, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes[0].path, "c");
    }

    #[test]
    fn snapshot_reads_files_and_absent_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("d")).unwrap();
        std::fs::write(dir.path().join("d/a.txt"), "A").unwrap();
        let d = Disk::snapshot(
            dir.path(),
            ["d/a.txt", "missing.txt", "d/a.txt"].into_iter(),
        )
        .unwrap();
        assert_eq!(d, disk(&[("d/a.txt", Some(b"A")), ("missing.txt", None)]));
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_refuses_symlinks_and_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        for p in ["link/secret", "../secret"] {
            assert!(Disk::snapshot(dir.path(), [p].into_iter()).is_err(), "{p}");
        }
    }

    fn v2() -> Template {
        Template::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/template/v2"))
            .unwrap()
    }

    fn inst(id: &str, name: &str) -> PieceInstance {
        PieceInstance {
            id: id.into(),
            name: name.into(),
            vars: IndexMap::from([("name".to_string(), name.to_string())]),
        }
    }

    fn remove(path: &str, piece: Option<&str>) -> Remove {
        Remove {
            path: path.into(),
            piece: piece.map(String::from),
        }
    }

    #[test]
    fn render_removes_per_instance_and_with_template_vars() {
        let t = v2();
        let instances = [
            inst("base", "base"),
            inst("app", "web"),
            inst("app", "admin"),
        ];
        let vars = IndexMap::from([("project".to_string(), "demo".to_string())]);
        let rs = [
            remove("apps/{{ name }}/.eslintrc.json", Some("app")),
            remove("{{ project }}.old", None),
            remove("tool.config.mts", Some("base")),
            remove("lib.txt", Some("lib")), // no instance of `lib`: nothing
            remove("{{ project }}.old", None), // duplicates collapse
        ];
        let refs: Vec<&Remove> = rs.iter().collect();
        assert_eq!(
            render_removes(&t, &refs, &instances, &vars).unwrap(),
            vec![
                "apps/web/.eslintrc.json",
                "apps/admin/.eslintrc.json",
                "demo.old",
                "tool.config.mts",
            ]
        );
    }

    #[test]
    fn render_removes_rejects_unsafe_paths_and_unknown_pieces() {
        let t = v2();
        let instances = [inst("base", "base")];
        let vars = IndexMap::from([("project".to_string(), "..".to_string())]);
        for r in [
            remove("../x", None),
            remove("/abs", None),
            remove("{{ project }}/x", None),
            remove(".splice/lock.toml", None),
            remove("{{ name }}", None), // only template vars without `piece`
            remove("x", Some("nope")),
        ] {
            let e = render_removes(&t, &[&r], &instances, &vars).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{}", r.path);
        }
    }
}
