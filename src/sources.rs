//! The sources of a repo (main template and channels) and the one-owner-per-path claim check.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use crate::error::SpliceError;
use crate::lock::{ChannelLock, Lock, SectionLock};
use crate::output::Class;
use crate::render::RenderedTree;

type Result<T> = std::result::Result<T, SpliceError>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceId {
    Main,
    Channel(String),
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceId::Main => write!(f, "main"),
            SourceId::Channel(n) => write!(f, "channel {n}"),
        }
    }
}

impl SourceId {
    /// Attribution tag for report messages.
    pub fn tag(&self) -> String {
        match self {
            SourceId::Main => "source=main".into(),
            SourceId::Channel(n) => format!("source=channel:{n}"),
        }
    }
}

pub struct Sources {
    pub main: Option<Lock>,
    pub channels: Vec<ChannelLock>,
}

/// What one source owns: files it owns or shares, and files it only hosts sections in.
#[derive(Debug, Clone)]
pub struct Claim {
    pub source: SourceId,
    pub files: BTreeSet<String>,
    pub hosts: BTreeSet<String>,
}

impl Sources {
    /// The main lock and every channel lock (sorted by name).
    pub fn load(repo: &Path) -> Result<Sources> {
        Ok(Sources {
            main: Lock::read(repo)?,
            channels: ChannelLock::read_all(repo)?,
        })
    }

    pub fn channel(&self, name: &str) -> Result<&ChannelLock> {
        self.channels
            .iter()
            .find(|c| c.meta.name == name)
            .ok_or_else(|| {
                let known: Vec<_> = self.channels.iter().map(|c| c.meta.name.as_str()).collect();
                let known = if known.is_empty() {
                    "none".to_string()
                } else {
                    known.join(", ")
                };
                SpliceError::Usage(format!("unknown channel `{name}`; known channels: {known}"))
            })
    }

    /// Current claims, main first, then channels by name.
    pub fn claims(&self) -> Vec<Claim> {
        let none = BTreeMap::new();
        self.main
            .iter()
            .map(|l| Claim::from_lock(SourceId::Main, l, &none))
            .chain(self.channels.iter().map(|c| {
                Claim::from_lock(SourceId::Channel(c.meta.name.clone()), &c.lock, &c.sections)
            }))
            .collect()
    }
}

impl Claim {
    pub fn from_lock(
        source: SourceId,
        lock: &Lock,
        sections: &BTreeMap<String, SectionLock>,
    ) -> Claim {
        Claim {
            source,
            files: lock.files.keys().cloned().collect(),
            hosts: sections.keys().cloned().collect(),
        }
    }

    /// The claim of a planned tree. Seeds are never claimed (G10).
    pub fn from_plan(
        source: SourceId,
        theirs: &RenderedTree,
        hosts: impl IntoIterator<Item = String>,
    ) -> Claim {
        Claim {
            source,
            files: theirs
                .values()
                .filter(|f| f.class != Class::Seed)
                .map(|f| f.path.clone())
                .collect(),
            hosts: hosts.into_iter().collect(),
        }
    }
}

/// Exit 2 when a path has two owners, or a section host is also somebody's file. Claims are
/// checked in order; the earlier holder is named. One message line per path, sorted.
pub fn check_claims(claims: &[Claim]) -> Result<()> {
    let mut owner: BTreeMap<&str, &SourceId> = BTreeMap::new();
    let mut conflicts: BTreeMap<&str, &SourceId> = BTreeMap::new();
    for c in claims {
        for f in &c.files {
            match owner.get(f.as_str()) {
                Some(first) => {
                    conflicts.entry(f).or_insert(first);
                }
                None => {
                    owner.insert(f, &c.source);
                }
            }
        }
    }
    for c in claims {
        for h in &c.hosts {
            if let Some(o) = owner.get(h.as_str()) {
                conflicts.entry(h).or_insert(o);
            }
        }
    }
    if conflicts.is_empty() {
        return Ok(());
    }
    let lines: Vec<_> = conflicts
        .iter()
        .map(|(p, o)| {
            format!(
                "path {p} is owned by {o}; exclude it with --exclude {p} or remove it from one source"
            )
        })
        .collect();
    Err(SpliceError::Usage(lines.join("\n")))
}

/// Swap the source's current claim for its planned one (appended when it had none).
pub fn replace_claim(current: Vec<Claim>, new: Claim) -> Vec<Claim> {
    let mut found = false;
    let mut out: Vec<Claim> = current
        .into_iter()
        .map(|c| {
            if c.source == new.source {
                found = true;
                new.clone()
            } else {
                c
            }
        })
        .collect();
    if !found {
        out.push(new);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lock::{ChannelMeta, LockTemplate};
    use crate::manifest::MergeStrategy;
    use crate::render::RenderedFile;
    use SourceId::Main;

    fn ch(n: &str) -> SourceId {
        SourceId::Channel(n.into())
    }

    fn claim(source: SourceId, files: &[&str], hosts: &[&str]) -> Claim {
        Claim {
            source,
            files: files.iter().map(|s| s.to_string()).collect(),
            hosts: hosts.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn chan_lock(name: &str) -> ChannelLock {
        ChannelLock {
            meta: ChannelMeta {
                name: name.into(),
                ..Default::default()
            },
            lock: Lock {
                format: 1,
                splice: semver::Version::new(0, 1, 0),
                template: LockTemplate {
                    source: "gh:o/r".into(),
                    version: semver::Version::new(1, 0, 0),
                    commit: "3f2c9d1e5a7b4c6d8e0f1a2b3c4d5e6f708192a3".into(),
                },
                vars: Default::default(),
                pieces: vec![],
                files: Default::default(),
            },
            sections: Default::default(),
        }
    }

    #[test]
    fn disjoint_claims_pass() {
        check_claims(&[
            claim(Main, &["a"], &[]),
            claim(ch("x"), &["b"], &["AGENTS.md"]),
        ])
        .unwrap();
    }

    #[test]
    fn same_path_two_sources() {
        let e = check_claims(&[
            claim(Main, &[".editorconfig"], &[]),
            claim(ch("rig"), &[".editorconfig"], &[]),
        ])
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert_eq!(
            e.to_string(),
            "path .editorconfig is owned by main; exclude it with --exclude .editorconfig or remove it from one source"
        );
    }

    #[test]
    fn conflicts_are_sorted_one_line_per_path() {
        let e = check_claims(&[
            claim(Main, &["b", "a"], &[]),
            claim(ch("x"), &["a", "b"], &[]),
            claim(ch("y"), &["a"], &[]),
        ])
        .unwrap_err();
        let m = e.to_string();
        let lines: Vec<_> = m.lines().collect();
        assert_eq!(lines.len(), 2, "{m}");
        assert!(lines[0].starts_with("path a is owned by main"), "{m}");
        assert!(lines[1].starts_with("path b is owned by main"), "{m}");
    }

    #[test]
    fn host_owned_by_other_source() {
        let e = check_claims(&[
            claim(Main, &["CLAUDE.md"], &[]),
            claim(ch("rig"), &[], &["CLAUDE.md"]),
        ])
        .unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert!(
            e.to_string().contains("path CLAUDE.md is owned by main"),
            "{e}"
        );
    }

    #[test]
    fn host_in_own_files_is_conflict() {
        let e = check_claims(&[claim(ch("rig"), &["AGENTS.md"], &["AGENTS.md"])]).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert!(
            e.to_string()
                .contains("path AGENTS.md is owned by channel rig"),
            "{e}"
        );
    }

    #[test]
    fn two_channels_one_host_is_fine() {
        check_claims(&[
            claim(ch("rig"), &[], &["AGENTS.md"]),
            claim(ch("decisions"), &[], &["AGENTS.md"]),
        ])
        .unwrap();
    }

    #[test]
    fn seeds_not_claimed() {
        let mut tree = RenderedTree::new();
        for (p, class) in [
            ("README.md", Class::Seed),
            ("a.cfg", Class::Owned),
            ("b.json", Class::Shared),
        ] {
            tree.insert(
                p.into(),
                RenderedFile {
                    path: p.into(),
                    bytes: vec![],
                    class,
                    strategy: MergeStrategy::Text,
                    piece_id: "p".into(),
                    piece_name: "p".into(),
                    glob: None,
                },
            );
        }
        let c = Claim::from_plan(ch("x"), &tree, ["AGENTS.md".to_string()]);
        assert_eq!(c.files.iter().collect::<Vec<_>>(), ["a.cfg", "b.json"]);
        assert_eq!(c.hosts.iter().collect::<Vec<_>>(), ["AGENTS.md"]);
    }

    #[test]
    fn load_reads_main_and_channels_sorted() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Sources::load(dir.path()).unwrap().main.is_none());
        chan_lock("b").write_atomic(dir.path()).unwrap();
        chan_lock("a").write_atomic(dir.path()).unwrap();
        let s = Sources::load(dir.path()).unwrap();
        let names: Vec<_> = s.channels.iter().map(|c| c.meta.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(s.claims().len(), 2);
        assert_eq!(s.claims()[0].source, ch("a"));
    }

    #[test]
    fn unknown_channel_lists_known() {
        let dir = tempfile::tempdir().unwrap();
        chan_lock("b").write_atomic(dir.path()).unwrap();
        chan_lock("a").write_atomic(dir.path()).unwrap();
        let s = Sources::load(dir.path()).unwrap();
        let e = s.channel("zz").unwrap_err();
        assert_eq!(e.exit_code(), 2);
        assert!(e.to_string().contains("a, b"), "{e}");
        assert_eq!(s.channel("a").unwrap().meta.name, "a");
    }

    #[test]
    fn display_and_tag() {
        assert_eq!(Main.to_string(), "main");
        assert_eq!(ch("rig").to_string(), "channel rig");
        assert_eq!(Main.tag(), "source=main");
        assert_eq!(ch("rig").tag(), "source=channel:rig");
    }

    #[test]
    fn replace_claim_swaps_in_place_or_appends() {
        let cur = vec![claim(Main, &["a"], &[]), claim(ch("x"), &["b"], &[])];
        let out = replace_claim(cur.clone(), claim(ch("x"), &["c"], &[]));
        assert_eq!(out.len(), 2);
        assert!(out[1].files.contains("c") && !out[1].files.contains("b"));
        let out = replace_claim(cur, claim(ch("y"), &["d"], &[]));
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].source, ch("y"));
    }
}
