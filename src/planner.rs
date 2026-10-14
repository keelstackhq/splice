//! The pure update planner (spec §8.4): base, theirs, disk and lock in, a list of changes out.

use crate::error::SpliceError;
use crate::fsio::{read_opt, sha256_hex};
use crate::lock::Lock;
use crate::manifest::{MergeStrategy, Template};
use crate::merge::merge3;
use crate::migrate::Remove;
use crate::output::{Action, Class};
use crate::paths::render_repo_path;
use crate::render::{PieceInstance, RenderedTree, Renderer};
use indexmap::IndexMap;
use minijinja::Value;
use std::collections::BTreeMap;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

/// One step of the plan: what to do with `path`; `bytes` is `Some` only for a write.
#[derive(Debug, Clone)]
pub struct Planned {
    pub path: String,
    pub action: Action,
    pub class: Class,
    pub bytes: Option<Vec<u8>>,
    pub note: Option<String>,
}

/// The on-disk state of every path the planner may look at (`None` = absent).
#[derive(Debug, Clone, Default)]
pub struct Disk {
    pub files: BTreeMap<String, Option<Vec<u8>>>,
}

impl Disk {
    /// Read every path under `repo` (`None` when absent). Never follows a symlink: a symlink at
    /// a path or on its way (or a directory where a file belongs) is an error, so no content
    /// from outside the repo can enter a plan.
    pub fn snapshot<'a>(repo: &Path, paths: impl Iterator<Item = &'a str>) -> Result<Disk> {
        let mut files = BTreeMap::new();
        for p in paths {
            crate::ops::check_target(repo, p)?;
            files.insert(p.to_string(), read_opt(repo, p)?);
        }
        Ok(Disk { files })
    }

    fn get(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).and_then(|o| o.as_deref())
    }
}

pub struct PlanInput<'a> {
    pub base: &'a RenderedTree,
    pub theirs: &'a RenderedTree,
    pub disk: &'a Disk,
    pub lock: &'a Lock,
    /// Rendered `[[remove]]` paths.
    pub removes: Vec<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub changes: Vec<Planned>,
    pub drift: Vec<String>,
    pub conflicts: Vec<String>,
    /// Lock-listed shared files that would be merged but still hold conflict markers on disk
    /// (spec §10 item 26): the update is refused, nothing is written.
    pub marked: Vec<String>,
}

impl Plan {
    fn push(&mut self, path: &str, action: Action, class: Class, bytes: Option<Vec<u8>>) {
        self.changes.push(Planned {
            path: path.to_string(),
            action,
            class,
            bytes,
            note: None,
        });
    }

    fn skip(&mut self, path: &str, class: Class, note: String) {
        self.changes.push(Planned {
            path: path.to_string(),
            action: Action::Skip,
            class,
            bytes: None,
            note: Some(note),
        });
    }
}

/// Three-way merge of a shared file; `base` is empty for a file the user made first.
fn merge_shared(
    plan: &mut Plan,
    path: &str,
    strategy: MergeStrategy,
    base: &[u8],
    ours: &[u8],
    theirs: &[u8],
) {
    let (Ok(b), Ok(o), Ok(t)) = (
        std::str::from_utf8(base),
        std::str::from_utf8(ours),
        std::str::from_utf8(theirs),
    ) else {
        plan.skip(
            path,
            Class::Shared,
            "not valid UTF-8, cannot merge; kept as is".into(),
        );
        return;
    };
    let m = merge3(strategy, b, o, t);
    if m.conflicts.is_empty() {
        if m.text != o {
            plan.push(
                path,
                Action::Merge,
                Class::Shared,
                Some(m.text.into_bytes()),
            );
        }
    } else {
        plan.conflicts.push(path.to_string());
        plan.changes.push(Planned {
            path: path.to_string(),
            action: Action::Conflict,
            class: Class::Shared,
            bytes: Some(m.text.into_bytes()),
            note: Some(format!("conflicts: {}", m.conflicts.join(", "))),
        });
    }
}

/// Leftover conflict markers in a UTF-8 file.
fn has_markers(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|t| !crate::markers::find_markers(t).is_empty())
}

const STILL_RENDERED: &str = "listed in [[remove]] but still rendered by the target; ignored";

pub fn plan(input: &PlanInput) -> Plan {
    let PlanInput {
        base,
        theirs,
        disk,
        lock,
        force,
        ..
    } = input;
    let force = *force;
    let mut out = Plan::default();
    let mut paths: Vec<&String> = base.keys().chain(theirs.keys()).collect();
    paths.sort();
    paths.dedup();
    for path in paths {
        let cur = disk.get(path);
        let unedited = |c: &[u8]| {
            lock.files
                .get(path.as_str())
                .is_some_and(|l| l.sha256 == sha256_hex(c))
        };
        match (base.get(path), theirs.get(path)) {
            (None, Some(t)) => match (t.class, cur) {
                (_, None) => out.push(path, Action::Create, t.class, Some(t.bytes.clone())),
                (Class::Seed, Some(_)) => {}
                (_, Some(c)) if c == t.bytes => {}
                (Class::Owned, Some(_)) if force => {
                    out.push(path, Action::Update, t.class, Some(t.bytes.clone()))
                }
                (Class::Owned, Some(_)) => out.drift.push(path.clone()),
                (_, Some(c)) => merge_shared(&mut out, path, t.strategy, b"", c, &t.bytes),
            },
            (Some(b), Some(t)) => {
                if t.class == Class::Seed {
                    continue;
                }
                if t.class == Class::Owned {
                    // contract §5: owned drift refuses unless --force, whether or not the
                    // template changed the file
                    match cur {
                        None if force => {
                            out.push(path, Action::Create, t.class, Some(t.bytes.clone()))
                        }
                        None => out.drift.push(path.clone()),
                        Some(c) if c == t.bytes => {}
                        Some(c) if unedited(c) && b.bytes == t.bytes => {}
                        Some(c) if force || unedited(c) => {
                            out.push(path, Action::Update, t.class, Some(t.bytes.clone()))
                        }
                        Some(_) => out.drift.push(path.clone()),
                    }
                    continue;
                }
                if b.bytes == t.bytes {
                    continue;
                }
                match (t.class, cur) {
                    (_, None) => out.skip(
                        path,
                        t.class,
                        "deleted locally; kept deleted (the lock records the target version)"
                            .into(),
                    ),
                    (_, Some(c)) if c == b.bytes => {
                        out.push(path, Action::Update, t.class, Some(t.bytes.clone()))
                    }
                    (_, Some(c)) if lock.files.contains_key(path.as_str()) && has_markers(c) => {
                        out.marked.push(path.clone())
                    }
                    (_, Some(c)) => merge_shared(&mut out, path, t.strategy, &b.bytes, c, &t.bytes),
                }
            }
            (Some(b), None) => {
                let Some(c) = cur else { continue };
                let deletable = match b.class {
                    Class::Seed => continue,
                    Class::Owned => unedited(c),
                    Class::Shared => c == b.bytes,
                };
                if deletable {
                    out.push(path, Action::Delete, b.class, None);
                } else {
                    // kept, and unmanaged from now on: the new lock no longer lists it
                    out.skip(
                        path,
                        b.class,
                        "edited locally and no longer rendered; kept, but no longer managed by splice"
                            .into(),
                    );
                }
            }
            (None, None) => unreachable!("path comes from base or theirs"),
        }
    }

    // `[[remove]]` paths. Those in base are handled above; the rest can only be deleted if the
    // lock proves they are managed and unedited.
    let mut seen = std::collections::BTreeSet::new();
    for path in &input.removes {
        if !seen.insert(path.as_str()) {
            continue;
        }
        if let Some(t) = theirs.get(path) {
            let note = STILL_RENDERED.to_string();
            match out.changes.iter_mut().find(|c| &c.path == path) {
                Some(c) => {
                    c.note = Some(match c.note.take() {
                        Some(n) => format!("{n}; {note}"),
                        None => note,
                    })
                }
                None => out.skip(path, t.class, note),
            }
            continue;
        }
        if base.contains_key(path) {
            continue;
        }
        let (Some(entry), Some(c)) = (lock.files.get(path.as_str()), disk.get(path)) else {
            continue;
        };
        if entry.class == Class::Seed {
            continue;
        }
        if entry.sha256 == sha256_hex(c) {
            out.push(path, Action::Delete, entry.class, None);
        } else {
            out.skip(
                path,
                entry.class,
                "listed in [[remove]] but edited locally; kept, but no longer managed by splice"
                    .into(),
            );
        }
    }
    out
}

/// Render the `[[remove]]` paths of the selected migrations. A path with `piece` is rendered
/// once per instance of that piece (an id naming no piece in the template is exit 3); without
/// `piece` only the template vars, `pieces` and `template` are in scope. `version` is the
/// base (lock) version, like `render_tree` uses for `template.version`. Every result is re-checked with the rendered
/// path rules of `paths.rs`.
pub fn render_removes(
    t: &Template,
    removes: &[&Remove],
    instances: &[PieceInstance],
    vars: &IndexMap<String, String>,
    version: &semver::Version,
) -> Result<Vec<String>> {
    let renderer = Renderer::new();
    let mut ids: Vec<&str> = Vec::new();
    for i in instances {
        if !ids.contains(&i.id.as_str()) {
            ids.push(&i.id);
        }
    }
    let ctx = |extra: &IndexMap<String, String>, inst: Option<&PieceInstance>| {
        let mut m = serde_json::Map::new();
        for (k, v) in vars.iter().chain(extra.iter()) {
            m.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        m.insert("pieces".into(), serde_json::json!(ids));
        if let Some(i) = inst {
            m.insert(
                "piece".into(),
                serde_json::json!({ "id": i.id, "name": i.name }),
            );
        }
        m.insert(
            "template".into(),
            serde_json::json!({ "name": t.manifest.name, "version": version.to_string() }),
        );
        Value::from_serialize(serde_json::Value::Object(m))
    };
    let mut out: Vec<String> = Vec::new();
    let mut add = |p: String| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    for r in removes {
        match &r.piece {
            None => add(render_repo_path(
                &r.path,
                &renderer,
                &ctx(&IndexMap::new(), None),
            )?),
            Some(id) => {
                if !t.pieces.contains_key(id) {
                    return Err(SpliceError::Template(format!(
                        "migration [[remove]] path {:?} names piece `{id}`, which the template does not have",
                        r.path
                    )));
                }
                for i in instances.iter().filter(|i| &i.id == id) {
                    add(render_repo_path(
                        &r.path,
                        &renderer,
                        &ctx(&i.vars, Some(i)),
                    )?);
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsio::sha256_hex;
    use crate::lock::{LockFile, LockTemplate};
    use crate::manifest::{MergeStrategy, PieceManifest, TemplateManifest};
    use crate::render::RenderedFile;

    fn file(path: &str, bytes: &str, class: Class) -> RenderedFile {
        RenderedFile {
            path: path.into(),
            bytes: bytes.as_bytes().to_vec(),
            class,
            strategy: MergeStrategy::Text,
            piece_id: "base".into(),
            piece_name: "base".into(),
            glob: None,
        }
    }

    fn lock_with(files: &[(&str, Class, &str)]) -> Lock {
        Lock {
            format: 1,
            splice: semver::Version::new(0, 1, 0),
            template: LockTemplate {
                source: "x".into(),
                version: semver::Version::new(1, 0, 0),
                commit: "0".repeat(40),
            },
            vars: IndexMap::new(),
            pieces: vec![],
            files: files
                .iter()
                .map(|(p, class, content)| {
                    (
                        p.to_string(),
                        LockFile {
                            class: *class,
                            sha256: sha256_hex(content.as_bytes()),
                        },
                    )
                })
                .collect(),
        }
    }

    fn disk(files: &[(&str, Option<&str>)]) -> Disk {
        Disk {
            files: files
                .iter()
                .map(|(p, c)| (p.to_string(), c.map(|c| c.as_bytes().to_vec())))
                .collect(),
        }
    }

    type Case = (
        &'static str,
        Class,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
        bool,
        Option<Action>,
        Option<&'static str>,
        bool, // drift expected
    );

    fn check_case(c: Case) {
        let (name, class, base, theirs, on_disk, lock_sha_of, force, action, bytes, drift_expected) =
            c;
        let p = "f.txt";
        let base_t: RenderedTree = base.iter().map(|b| (p.into(), file(p, b, class))).collect();
        let theirs_t: RenderedTree = theirs
            .iter()
            .map(|t| (p.into(), file(p, t, class)))
            .collect();
        let lock_files: Vec<(&str, Class, &str)> =
            lock_sha_of.iter().map(|s| (p, class, *s)).collect();
        let lock = lock_with(&lock_files);
        let d = disk(&[(p, on_disk)]);
        let plan = plan(&PlanInput {
            base: &base_t,
            theirs: &theirs_t,
            disk: &d,
            lock: &lock,
            removes: vec![],
            force,
        });
        assert_eq!(
            plan.drift.contains(&p.to_string()),
            drift_expected,
            "{name}: drift"
        );
        match action {
            None => assert!(plan.changes.is_empty(), "{name}: {:?}", plan.changes),
            Some(a) => {
                assert_eq!(plan.changes.len(), 1, "{name}: {:?}", plan.changes);
                let ch = &plan.changes[0];
                assert_eq!(ch.action, a, "{name}");
                assert_eq!(ch.class, class, "{name}");
                match bytes {
                    Some(b) => assert_eq!(ch.bytes.as_deref(), Some(b.as_bytes()), "{name}"),
                    None if a == Action::Conflict => {
                        let text = String::from_utf8(ch.bytes.clone().unwrap()).unwrap();
                        assert!(text.contains("<<<<<<< ours"), "{name}: {text}");
                        assert!(text.contains(">>>>>>> template"), "{name}: {text}");
                        assert_eq!(plan.conflicts, [p], "{name}");
                    }
                    None => assert!(ch.bytes.is_none(), "{name}"),
                }
                if a == Action::Skip {
                    assert!(ch.note.is_some(), "{name}: skip needs a note");
                }
            }
        }
    }

    #[test]
    fn table() {
        use Action::*;
        use Class::*;
        let cases: [Case; 19] = [
            (
                "owned unedited changed",
                Owned,
                Some("a"),
                Some("b"),
                Some("a"),
                Some("a"),
                false,
                Some(Update),
                Some("b"),
                false,
            ),
            (
                "owned unchanged",
                Owned,
                Some("a"),
                Some("a"),
                Some("a"),
                Some("a"),
                false,
                None,
                None,
                false,
            ),
            (
                "owned drift",
                Owned,
                Some("a"),
                Some("b"),
                Some("x"),
                Some("a"),
                false,
                None,
                None,
                true,
            ),
            (
                "owned drift --force",
                Owned,
                Some("a"),
                Some("b"),
                Some("x"),
                Some("a"),
                true,
                Some(Update),
                Some("b"),
                false,
            ),
            (
                "owned deleted by user",
                Owned,
                Some("a"),
                Some("b"),
                None,
                Some("a"),
                false,
                None,
                None,
                true,
            ),
            (
                "owned new, absent",
                Owned,
                None,
                Some("b"),
                None,
                None,
                false,
                Some(Create),
                Some("b"),
                false,
            ),
            (
                "owned new, user file",
                Owned,
                None,
                Some("b"),
                Some("u"),
                None,
                false,
                None,
                None,
                true,
            ),
            (
                "shared ours==base",
                Shared,
                Some("a\n"),
                Some("b\n"),
                Some("a\n"),
                Some("a\n"),
                false,
                Some(Update),
                Some("b\n"),
                false,
            ),
            (
                "shared theirs==base",
                Shared,
                Some("a\n"),
                Some("a\n"),
                Some("u\n"),
                Some("a\n"),
                false,
                None,
                None,
                false,
            ),
            (
                "shared merge",
                Shared,
                Some("1\n2\n3\n"),
                Some("1\n2\nT\n"),
                Some("U\n2\n3\n"),
                Some("1\n2\n3\n"),
                false,
                Some(Merge),
                Some("U\n2\nT\n"),
                false,
            ),
            (
                "shared conflict",
                Shared,
                Some("x\n"),
                Some("t\n"),
                Some("u\n"),
                Some("x\n"),
                false,
                Some(Conflict),
                None,
                false,
            ),
            (
                "shared deleted by user",
                Shared,
                Some("a\n"),
                Some("b\n"),
                None,
                Some("a\n"),
                false,
                Some(Skip),
                None,
                false,
            ),
            (
                "shared new, user file",
                Shared,
                None,
                Some("t\n"),
                Some("u\n"),
                None,
                false,
                Some(Conflict),
                None,
                false,
            ),
            (
                "seed exists",
                Seed,
                Some("a"),
                Some("b"),
                Some("u"),
                None,
                false,
                None,
                None,
                false,
            ),
            (
                "seed new, absent",
                Seed,
                None,
                Some("b"),
                None,
                None,
                false,
                Some(Create),
                Some("b"),
                false,
            ),
            (
                "owned gone, unedited",
                Owned,
                Some("a"),
                None,
                Some("a"),
                Some("a"),
                false,
                Some(Delete),
                None,
                false,
            ),
            (
                "owned gone, edited",
                Owned,
                Some("a"),
                None,
                Some("x"),
                Some("a"),
                false,
                Some(Skip),
                None,
                false,
            ),
            (
                "shared gone, edited",
                Shared,
                Some("a"),
                None,
                Some("x"),
                Some("a"),
                false,
                Some(Skip),
                None,
                false,
            ),
            (
                "seed gone",
                Seed,
                Some("a"),
                None,
                Some("a"),
                None,
                false,
                None,
                None,
                false,
            ),
        ];
        for c in cases {
            check_case(c);
        }
    }

    #[test]
    fn more_cells_of_the_table() {
        use Action::*;
        use Class::*;
        let more: [Case; 13] = [
            (
                "owned deleted by user --force",
                Owned,
                Some("a"),
                Some("b"),
                None,
                Some("a"),
                true,
                Some(Create),
                Some("b"),
                false,
            ),
            (
                "owned new, user file equals theirs",
                Owned,
                None,
                Some("b"),
                Some("b"),
                None,
                false,
                None,
                None,
                false,
            ),
            (
                "owned new, user file --force",
                Owned,
                None,
                Some("b"),
                Some("u"),
                None,
                true,
                Some(Update),
                Some("b"),
                false,
            ),
            (
                "owned gone, edited --force",
                Owned,
                Some("a"),
                None,
                Some("x"),
                Some("a"),
                true,
                Some(Skip),
                None,
                false,
            ),
            (
                "shared gone, unedited",
                Shared,
                Some("a"),
                None,
                Some("a"),
                Some("a"),
                false,
                Some(Delete),
                None,
                false,
            ),
            (
                "shared new, absent",
                Shared,
                None,
                Some("t\n"),
                None,
                None,
                false,
                Some(Create),
                Some("t\n"),
                false,
            ),
            (
                "shared new, user file equals theirs",
                Shared,
                None,
                Some("t\n"),
                Some("t\n"),
                None,
                false,
                None,
                None,
                false,
            ),
            (
                "seed deleted by user",
                Seed,
                Some("a"),
                Some("b"),
                None,
                None,
                false,
                None,
                None,
                false,
            ),
            (
                "owned gone, already deleted",
                Owned,
                Some("a"),
                None,
                None,
                Some("a"),
                false,
                None,
                None,
                false,
            ),
            (
                "owned edited, T=B",
                Owned,
                Some("a"),
                Some("a"),
                Some("x"),
                Some("a"),
                false,
                None,
                None,
                true,
            ),
            (
                "owned edited, T=B --force",
                Owned,
                Some("a"),
                Some("a"),
                Some("x"),
                Some("a"),
                true,
                Some(Update),
                Some("a"),
                false,
            ),
            (
                "owned deleted, T=B",
                Owned,
                Some("a"),
                Some("a"),
                None,
                Some("a"),
                false,
                None,
                None,
                true,
            ),
            (
                "owned deleted, T=B --force",
                Owned,
                Some("a"),
                Some("a"),
                None,
                Some("a"),
                true,
                Some(Create),
                Some("a"),
                false,
            ),
        ];
        for c in more {
            check_case(c);
        }
    }

    fn run(
        base: &[RenderedFile],
        theirs: &[RenderedFile],
        d: &Disk,
        lock: &Lock,
        removes: &[&str],
    ) -> Plan {
        let t = |f: &[RenderedFile]| -> RenderedTree {
            f.iter().map(|f| (f.path.clone(), f.clone())).collect()
        };
        plan(&PlanInput {
            base: &t(base),
            theirs: &t(theirs),
            disk: d,
            lock,
            removes: removes.iter().map(|s| s.to_string()).collect(),
            force: false,
        })
    }

    #[test]
    fn remove_list_deletes_unedited() {
        let lock = lock_with(&[("old/x.txt", Class::Owned, "a")]);
        let p = run(
            &[],
            &[],
            &disk(&[("old/x.txt", Some("a"))]),
            &lock,
            &["old/x.txt"],
        );
        assert_eq!(p.changes.len(), 1);
        assert_eq!(p.changes[0].action, Action::Delete);
        assert_eq!(p.changes[0].class, Class::Owned);
        assert!(p.changes[0].bytes.is_none());
        assert!(p.drift.is_empty());
    }

    #[test]
    fn remove_list_keeps_edited_with_a_note_even_with_force() {
        let lock = lock_with(&[("old/x.txt", Class::Owned, "a")]);
        let d = disk(&[("old/x.txt", Some("mine"))]);
        let t = BTreeMap::new();
        for force in [false, true] {
            let p = plan(&PlanInput {
                base: &t,
                theirs: &t,
                disk: &d,
                lock: &lock,
                removes: vec!["old/x.txt".into()],
                force,
            });
            assert_eq!(p.changes.len(), 1);
            assert_eq!(p.changes[0].action, Action::Skip);
            assert!(p.changes[0].note.as_deref().unwrap().contains("edited"));
            assert!(p.drift.is_empty());
        }
    }

    #[test]
    fn remove_list_ignores_absent_unmanaged_and_seed() {
        let lock = lock_with(&[("s.txt", Class::Seed, "a")]);
        let d = disk(&[
            ("gone.txt", None),
            ("s.txt", Some("a")),
            ("user.txt", Some("u")),
        ]);
        // gone.txt: absent; s.txt: seeds are never deleted; user.txt: not in the lock
        let p = run(&[], &[], &d, &lock, &["gone.txt", "s.txt", "user.txt"]);
        assert!(p.changes.is_empty(), "{:?}", p.changes);
    }

    #[test]
    fn remove_list_ignores_still_rendered_with_a_warning() {
        let lock = lock_with(&[("a.txt", Class::Owned, "a")]);
        let f = file("a.txt", "a", Class::Owned);
        let d = disk(&[("a.txt", Some("a"))]);
        let p = run(
            std::slice::from_ref(&f),
            std::slice::from_ref(&f),
            &d,
            &lock,
            &["a.txt"],
        );
        assert_eq!(p.changes.len(), 1, "{:?}", p.changes);
        assert_eq!(p.changes[0].action, Action::Skip);
        assert!(p.changes[0].bytes.is_none());
        assert!(
            p.changes[0]
                .note
                .as_deref()
                .unwrap()
                .contains("still rendered"),
            "{:?}",
            p.changes[0].note
        );
    }

    #[test]
    fn remove_of_a_path_in_base_is_not_planned_twice() {
        let lock = lock_with(&[("a.txt", Class::Owned, "a")]);
        let b = file("a.txt", "a", Class::Owned);
        let p = run(&[b], &[], &disk(&[("a.txt", Some("a"))]), &lock, &["a.txt"]);
        assert_eq!(p.changes.len(), 1, "{:?}", p.changes);
        assert_eq!(p.changes[0].action, Action::Delete);
    }

    #[test]
    fn still_rendered_note_is_attached_to_the_existing_change() {
        let lock = lock_with(&[("a.txt", Class::Owned, "a")]);
        let b = file("a.txt", "a", Class::Owned);
        let t = file("a.txt", "b", Class::Owned);
        let p = run(
            &[b],
            &[t],
            &disk(&[("a.txt", Some("a"))]),
            &lock,
            &["a.txt"],
        );
        assert_eq!(p.changes.len(), 1, "{:?}", p.changes);
        assert_eq!(p.changes[0].action, Action::Update);
        assert!(
            p.changes[0]
                .note
                .as_deref()
                .unwrap()
                .contains("still rendered")
        );
    }

    // ---- Disk::snapshot

    #[test]
    fn snapshot_reads_files_and_marks_absent() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/f.txt"), "hi").unwrap();
        let s = Disk::snapshot(d.path(), ["a/f.txt", "a/none", "nodir/x"].into_iter()).unwrap();
        assert_eq!(s.files["a/f.txt"].as_deref(), Some(&b"hi"[..]));
        assert_eq!(s.files["a/none"], None);
        assert_eq!(s.files["nodir/x"], None);
    }

    #[cfg(unix)]
    #[test]
    fn snapshot_never_follows_symlinks_out_of_the_repo() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "S").unwrap();
        std::fs::create_dir(outside.path().join("dir")).unwrap();
        std::fs::write(outside.path().join("dir/f"), "S").unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), repo.path().join("link"))
            .unwrap();
        std::os::unix::fs::symlink(outside.path().join("dir"), repo.path().join("dirlink"))
            .unwrap();
        std::os::unix::fs::symlink(repo.path().join("missing"), repo.path().join("dangling"))
            .unwrap();
        for p in ["link", "dirlink/f", "dangling"] {
            let e = Disk::snapshot(repo.path(), [p].into_iter()).unwrap_err();
            assert!(e.to_string().contains("symlink"), "{p}: {e}");
            assert_eq!(e.exit_code(), 3);
        }
    }

    #[test]
    fn snapshot_rejects_a_directory_at_a_file_path() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        assert!(Disk::snapshot(d.path(), ["sub"].into_iter()).is_err());
    }

    // ---- render_removes

    fn piece(id: &str) -> PieceManifest {
        PieceManifest {
            id: id.into(),
            description: String::new(),
            multiple: true,
            requires: vec![],
            target: None,
            vars: IndexMap::new(),
            owned: vec![],
            shared: vec![],
            seed: vec![],
            merge: IndexMap::new(),
            post: vec![],
            files_dir: "unused".into(),
        }
    }

    fn template() -> Template {
        Template {
            root: ".".into(),
            manifest: TemplateManifest {
                format: 1,
                name: "t".into(),
                min_splice: semver::Version::new(0, 1, 0),
                base: "base".into(),
                vars: IndexMap::new(),
            },
            pieces: [("app".to_string(), piece("app"))].into_iter().collect(),
            migrations: vec![],
            warnings: vec![],
        }
    }

    fn inst(id: &str, name: &str) -> PieceInstance {
        PieceInstance {
            id: id.into(),
            name: name.into(),
            vars: [("name".to_string(), name.to_string())]
                .into_iter()
                .collect(),
        }
    }

    fn rm(path: &str, piece: Option<&str>) -> Remove {
        Remove {
            path: path.into(),
            piece: piece.map(str::to_string),
        }
    }

    fn v1() -> semver::Version {
        semver::Version::new(1, 2, 3)
    }

    fn vars(kv: &[(&str, &str)]) -> IndexMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn render_removes_per_instance_and_template_vars() {
        let t = template();
        let instances = [inst("app", "web"), inst("app", "api"), inst("other", "x")];
        let r1 = rm("apps/{{ name }}/old.json", Some("app"));
        let r2 = rm("{{ scope }}/old.txt", None);
        let r3 = rm("plain.txt", None);
        let out = render_removes(
            &t,
            &[&r1, &r2, &r3],
            &instances,
            &vars(&[("scope", "pkg")]),
            &v1(),
        )
        .unwrap();
        assert_eq!(
            out,
            [
                "apps/web/old.json",
                "apps/api/old.json",
                "pkg/old.txt",
                "plain.txt"
            ]
        );
    }

    #[test]
    fn render_removes_unknown_piece_is_exit_3() {
        let r = rm("a.txt", Some("nope"));
        let e = render_removes(&template(), &[&r], &[], &vars(&[]), &v1()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("nope"), "{e}");
    }

    #[test]
    fn render_removes_rechecks_rendered_paths() {
        let t = template();
        let instances = [inst("app", "..")];
        for (path, v) in [
            ("{{ name }}/x", ".."),
            ("{{ name }}", "a/b"),
            ("{{ name }}", ".git"),
            ("{{ name }}", "CON"),
            ("{{ name }}", ""),
        ] {
            let r = rm(path, None);
            let e = render_removes(&t, &[&r], &[], &vars(&[("name", v)]), &v1()).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{path} {v:?}");
        }
        let r = rm("{{ name }}/x", Some("app"));
        assert_eq!(
            render_removes(&t, &[&r], &instances, &vars(&[]), &v1())
                .unwrap_err()
                .exit_code(),
            3
        );
        let r = rm("{{ name }}", None);
        let e = render_removes(&t, &[&r], &[], &vars(&[("name", ".splice")]), &v1()).unwrap();
        assert_eq!(e, [".splice"]);
        let r = rm(".splice/{{ name }}", None);
        let e = render_removes(&t, &[&r], &[], &vars(&[("name", "lock.toml")]), &v1()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
    }

    #[test]
    fn render_removes_undefined_var_is_exit_3() {
        let r = rm("{{ missing }}/x", None);
        let e = render_removes(&template(), &[&r], &[], &vars(&[]), &v1()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
    }

    #[test]
    fn render_removes_has_template_name_version_and_pieces() {
        let t = template();
        let r1 = rm("{{ template.name }}-{{ template.version }}/x", None);
        let r3 = rm("{{ template.version }}/{{ piece.id }}", Some("app"));
        let out =
            render_removes(&t, &[&r1, &r3], &[inst("app", "web")], &vars(&[]), &v1()).unwrap();
        assert_eq!(out, ["t-1.2.3/x", "1.2.3/app"]);
    }
}
