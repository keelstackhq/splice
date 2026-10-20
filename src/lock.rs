//! `.splice/lock.toml` (contract §6): read, build from a rendered tree, atomic write.

use std::collections::BTreeMap;
use std::path::Path;

use indexmap::IndexMap;

use crate::error::SpliceError;
use crate::fetch::{ResolvedTemplate, validate_commit};
use crate::fsio::{self, sha256_hex};
use crate::output::Class;
use crate::render::{PieceInstance, RenderedTree};
use crate::source::TemplateSource;

type Result<T> = std::result::Result<T, SpliceError>;

pub const LOCK_PATH: &str = ".splice/lock.toml";
pub const CHANNELS_DIR: &str = ".splice/channels";

/// Recovery hint appended to a corrupt main lock.
const MAIN_HINT: &str = "restore it from git or re-run `splice new`";
/// Recovery hint appended to a corrupt channel lock.
const CHANNEL_HINT: &str = "restore it from git or re-run `splice channel add`";

pub fn channel_lock_path(name: &str) -> String {
    format!("{CHANNELS_DIR}/{name}.toml")
}

/// Channel names match `^[a-z][a-z0-9-]*$`.
pub fn valid_channel_name(name: &str) -> bool {
    let mut b = name.bytes();
    matches!(b.next(), Some(b'a'..=b'z'))
        && b.all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockFile {
    pub class: Class,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockPiece {
    pub id: String,
    pub name: String,
    pub vars: IndexMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockTemplate {
    pub source: String,
    pub version: semver::Version,
    pub commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lock {
    pub format: u32,
    pub splice: semver::Version,
    pub template: LockTemplate,
    pub vars: IndexMap<String, String>,
    pub pieces: Vec<LockPiece>,
    pub files: BTreeMap<String, LockFile>,
}

#[derive(serde::Deserialize)]
struct RawLock {
    splice: String,
    template: RawTemplate,
    #[serde(default)]
    vars: IndexMap<String, String>,
    #[serde(default)]
    pieces: Vec<RawPiece>,
    #[serde(default)]
    files: BTreeMap<String, RawFile>,
}

#[derive(serde::Deserialize)]
struct RawTemplate {
    source: String,
    version: String,
    commit: String,
}

#[derive(serde::Deserialize)]
struct RawPiece {
    id: String,
    name: String,
    #[serde(default)]
    vars: IndexMap<String, String>,
}

#[derive(serde::Deserialize)]
struct RawFile {
    class: String,
    sha256: String,
}

fn bad_lock(path: &str, hint: &str, why: impl std::fmt::Display) -> SpliceError {
    SpliceError::Template(format!("{path}: {why}; {hint}"))
}

fn parse_version(path: &str, hint: &str, what: &str, v: &str) -> Result<semver::Version> {
    semver::Version::parse(v).map_err(|e| bad_lock(path, hint, format!("`{what}` = \"{v}\": {e}")))
}

/// Parse the contract §6 part of a lock. The table is returned so a caller can read the keys
/// only it knows (`[channel]`, `[sections]`).
fn parse_lock(text: &str, path: &str, hint: &str) -> Result<(Lock, toml::Table)> {
    let table: toml::Table = text.parse().map_err(|e| bad_lock(path, hint, e))?;
    match table.get("format").and_then(toml::Value::as_integer) {
        Some(1) => {}
        Some(n) => {
            return Err(SpliceError::Template(format!(
                "{path}: format {n} is not supported by this splice (supports 1); upgrade splice"
            )));
        }
        None => return Err(bad_lock(path, hint, "missing integer `format`")),
    }
    let raw: RawLock = table
        .clone()
        .try_into()
        .map_err(|e| bad_lock(path, hint, e))?;
    validate_commit(&raw.template.commit, path)?;
    let mut files = BTreeMap::new();
    for (file, f) in raw.files {
        let class = match f.class.as_str() {
            "owned" => Class::Owned,
            "shared" => Class::Shared,
            other => {
                return Err(bad_lock(
                    path,
                    hint,
                    format!("file `{file}`: class `{other}` is not allowed (owned or shared)"),
                ));
            }
        };
        files.insert(
            file,
            LockFile {
                class,
                sha256: f.sha256,
            },
        );
    }
    let lock = Lock {
        format: 1,
        splice: parse_version(path, hint, "splice", &raw.splice)?,
        template: LockTemplate {
            source: raw.template.source,
            version: parse_version(path, hint, "template.version", &raw.template.version)?,
            commit: raw.template.commit,
        },
        vars: raw.vars,
        pieces: raw
            .pieces
            .into_iter()
            .map(|p| LockPiece {
                id: p.id,
                name: p.name,
                vars: p.vars,
            })
            .collect(),
        files,
    };
    Ok((lock, table))
}

impl Lock {
    /// The lock for a rendered tree. Seed files are not listed (contract §6).
    pub fn from_tree(
        t: &ResolvedTemplate,
        vars: &IndexMap<String, String>,
        instances: &[PieceInstance],
        tree: &RenderedTree,
    ) -> Lock {
        let files = tree
            .values()
            .filter(|f| f.class != Class::Seed)
            .map(|f| {
                (
                    f.path.clone(),
                    LockFile {
                        class: f.class,
                        sha256: sha256_hex(&f.bytes),
                    },
                )
            })
            .collect();
        Lock {
            format: 1,
            splice: semver::Version::parse(env!("CARGO_PKG_VERSION"))
                .expect("CARGO_PKG_VERSION is semver"),
            template: LockTemplate {
                source: t.source.lock_spec(),
                version: t.version.clone(),
                commit: t.commit.clone(),
            },
            vars: vars.clone(),
            pieces: instances
                .iter()
                .map(|i| LockPiece {
                    id: i.id.clone(),
                    name: i.name.clone(),
                    vars: i.vars.clone(),
                })
                .collect(),
            files,
        }
    }

    /// Keep the `source` of the lock this one replaces when that was a relative local path:
    /// `from_tree` writes the resolved absolute path, but a hand-written `../tpl` should stay
    /// as it was (spec §10 item 18).
    #[must_use]
    pub fn keep_relative_source(mut self, old: &str) -> Lock {
        if TemplateSource::parse(old).is_ok_and(|s| s.is_relative_local()) {
            self.template.source = old.to_string();
        }
        self
    }

    /// `None` when the repo has no lock. An unknown `format`, a malformed lock, a commit that is
    /// not a full lowercase sha or a `[channel]`/`[sections]` table (only channel locks have
    /// them) is a template/environment error (exit 3).
    pub fn read(repo: &Path) -> Result<Option<Lock>> {
        let Some(bytes) = fsio::read_opt(repo, LOCK_PATH)? else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| bad_lock(LOCK_PATH, MAIN_HINT, "not valid UTF-8"))?;
        let (lock, table) = parse_lock(&text, LOCK_PATH, MAIN_HINT)?;
        for key in ["channel", "sections"] {
            if table.contains_key(key) {
                return Err(bad_lock(
                    LOCK_PATH,
                    MAIN_HINT,
                    format!("unexpected `[{key}]` (only channel locks have it)"),
                ));
            }
        }
        Ok(Some(lock))
    }

    fn to_toml(&self) -> String {
        self.to_document(None, None).to_string()
    }

    /// The lock document. A channel lock passes its `[channel]` (after `splice`) and `[sections]`
    /// (after `[files]`); tables are emitted in insertion order.
    fn to_document(
        &self,
        channel: Option<&ChannelMeta>,
        sections: Option<&BTreeMap<String, SectionLock>>,
    ) -> toml_edit::DocumentMut {
        use toml_edit::{Array, DocumentMut, InlineTable, Table, value};

        fn inline(pairs: &IndexMap<String, String>) -> InlineTable {
            let mut t = InlineTable::new();
            for (k, v) in pairs {
                t.insert(k, v.as_str().into());
            }
            t
        }

        let mut doc = DocumentMut::new();
        doc["format"] = value(i64::from(self.format));
        doc["splice"] = value(self.splice.to_string());
        if let Some(m) = channel {
            let strings = |v: &[String]| v.iter().map(String::as_str).collect::<Array>();
            let mut t = Table::new();
            t["name"] = value(m.name.as_str());
            if let Some(track) = &m.track {
                t["track"] = value(track.as_str());
            }
            t["only"] = value(strings(&m.only));
            t["exclude"] = value(strings(&m.exclude));
            doc["channel"] = toml_edit::Item::Table(t);
        }
        let mut template = Table::new();
        template["source"] = value(self.template.source.as_str());
        template["version"] = value(self.template.version.to_string());
        template["commit"] = value(self.template.commit.as_str());
        doc["template"] = toml_edit::Item::Table(template);
        let mut vars = Table::new();
        for (k, v) in &self.vars {
            vars[k.as_str()] = value(v.as_str());
        }
        doc["vars"] = toml_edit::Item::Table(vars);
        let mut pieces = toml_edit::ArrayOfTables::new();
        for p in &self.pieces {
            let mut t = Table::new();
            t["id"] = value(p.id.as_str());
            t["name"] = value(p.name.as_str());
            t["vars"] = value(inline(&p.vars));
            pieces.push(t);
        }
        doc["pieces"] = toml_edit::Item::ArrayOfTables(pieces);
        let mut files = Table::new();
        for (path, f) in &self.files {
            let mut e = InlineTable::new();
            e.insert("class", f.class.as_str().into());
            e.insert("sha256", f.sha256.as_str().into());
            files[path.as_str()] = value(e);
        }
        doc["files"] = toml_edit::Item::Table(files);
        if let Some(sections) = sections {
            let mut t = Table::new();
            for (path, sec) in sections {
                let mut e = InlineTable::new();
                e.insert("sha256", sec.sha256.as_str().into());
                e.insert("created", sec.created.into());
                t[path.as_str()] = value(e);
            }
            doc["sections"] = toml_edit::Item::Table(t);
        }
        doc
    }

    /// Write `.splice/lock.toml` via a temp file in `.splice/`, fsync and rename. The destination is
    /// checked like every write (no `..`, no symlinks); a failure leaves any old lock in place.
    pub fn write_atomic(&self, repo: &Path) -> Result<()> {
        fsio::write_atomic(repo, LOCK_PATH, self.to_toml().as_bytes())
    }

    pub fn instances(&self) -> Vec<PieceInstance> {
        self.pieces
            .iter()
            .map(|p| PieceInstance {
                id: p.id.clone(),
                name: p.name.clone(),
                vars: p.vars.clone(),
            })
            .collect()
    }
}

fn check_name(name: &str) -> Result<()> {
    if valid_channel_name(name) {
        Ok(())
    } else {
        Err(SpliceError::Template(format!(
            "invalid channel name `{name}`: expected `^[a-z][a-z0-9-]*$`"
        )))
    }
}

/// A channel's identity and filters, from its lock's `[channel]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChannelMeta {
    pub name: String,
    pub track: Option<String>,
    pub only: Vec<String>,
    pub exclude: Vec<String>,
}

/// Hash of the managed block of a section host and whether splice created the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionLock {
    pub sha256: String,
    pub created: bool,
}

/// `.splice/channels/<name>.toml`: a contract §6 lock plus `[channel]` and `[sections]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLock {
    pub meta: ChannelMeta,
    pub lock: Lock,
    pub sections: BTreeMap<String, SectionLock>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChannel {
    name: String,
    track: Option<String>,
    #[serde(default)]
    only: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSection {
    sha256: String,
    created: bool,
}

impl ChannelLock {
    pub fn path(&self) -> String {
        channel_lock_path(&self.meta.name)
    }

    /// `None` when the channel has no lock file. Corruption (bad TOML, missing `[channel]`, a name
    /// that differs from the file name, a bad commit) is exit 3 and names the file.
    pub fn read(repo: &Path, name: &str) -> Result<Option<ChannelLock>> {
        check_name(name)?;
        let path = channel_lock_path(name);
        let Some(bytes) = fsio::read_opt(repo, &path)? else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes)
            .map_err(|_| bad_lock(&path, CHANNEL_HINT, "not valid UTF-8"))?;
        let (lock, mut table) = parse_lock(&text, &path, CHANNEL_HINT)?;
        let Some(channel) = table.remove("channel") else {
            return Err(bad_lock(&path, CHANNEL_HINT, "missing `[channel]` table"));
        };
        let raw: RawChannel = channel
            .try_into()
            .map_err(|e| bad_lock(&path, CHANNEL_HINT, format!("`[channel]`: {e}")))?;
        if raw.name != name {
            return Err(bad_lock(
                &path,
                CHANNEL_HINT,
                format!(
                    "`[channel]` name = \"{}\" does not match the file name",
                    raw.name
                ),
            ));
        }
        let sections = match table.remove("sections") {
            None => BTreeMap::new(),
            Some(v) => {
                let raw: BTreeMap<String, RawSection> = v
                    .try_into()
                    .map_err(|e| bad_lock(&path, CHANNEL_HINT, format!("`[sections]`: {e}")))?;
                raw.into_iter()
                    .map(|(k, s)| {
                        (
                            k,
                            SectionLock {
                                sha256: s.sha256,
                                created: s.created,
                            },
                        )
                    })
                    .collect()
            }
        };
        Ok(Some(ChannelLock {
            meta: ChannelMeta {
                name: raw.name,
                track: raw.track,
                only: raw.only,
                exclude: raw.exclude,
            },
            lock,
            sections,
        }))
    }

    /// Every channel lock, sorted by name. Names starting with `.` (temp files of an interrupted
    /// write) are ignored; any other entry that is not `<valid name>.toml` is corrupt (exit 3).
    pub fn read_all(repo: &Path) -> Result<Vec<ChannelLock>> {
        let entries = match std::fs::read_dir(repo.join(CHANNELS_DIR)) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.starts_with('.') {
                continue;
            }
            match file.strip_suffix(".toml") {
                Some(name) if valid_channel_name(name) && entry.path().is_file() => {
                    names.push(name.to_string());
                }
                _ => {
                    return Err(bad_lock(
                        &format!("{CHANNELS_DIR}/{file}"),
                        CHANNEL_HINT,
                        "not a channel lock (expected `<name>.toml`, name matching \
                         `^[a-z][a-z0-9-]*$`)",
                    ));
                }
            }
        }
        names.sort();
        let mut out = Vec::new();
        for name in names {
            if let Some(cl) = ChannelLock::read(repo, &name)? {
                out.push(cl);
            }
        }
        Ok(out)
    }

    pub fn to_toml(&self) -> String {
        self.lock
            .to_document(Some(&self.meta), Some(&self.sections))
            .to_string()
    }

    /// Atomic write of `.splice/channels/<name>.toml` (same guarantees as `Lock::write_atomic`).
    pub fn write_atomic(&self, repo: &Path) -> Result<()> {
        check_name(&self.meta.name)?;
        fsio::write_atomic(repo, &self.path(), self.to_toml().as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::MergeStrategy;
    use crate::render::RenderedFile;
    use crate::source::TemplateSource;

    const SHA: &str = "3f2c9d1e5a7b4c6d8e0f1a2b3c4d5e6f708192a3";

    fn file(path: &str, body: &str, class: Class, id: &str, name: &str) -> RenderedFile {
        RenderedFile {
            path: path.into(),
            bytes: body.as_bytes().to_vec(),
            class,
            strategy: MergeStrategy::Text,
            piece_id: id.into(),
            piece_name: name.into(),
            glob: None,
        }
    }

    fn map(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn sample_lock() -> Lock {
        let t = ResolvedTemplate {
            source: TemplateSource::parse("gh:keelstackhq/hull//template").unwrap(),
            version: semver::Version::new(1, 2, 0),
            commit: SHA.into(),
            root: "/unused".into(),
        };
        let instances = vec![
            PieceInstance {
                id: "base".into(),
                name: "base".into(),
                vars: IndexMap::new(),
            },
            PieceInstance {
                id: "preact-app".into(),
                name: "web".into(),
                vars: map(&[("name", "web")]),
            },
        ];
        let mut tree = RenderedTree::new();
        for f in [
            file(
                "apps/web/vite.config.ts",
                "cfg\n",
                Class::Owned,
                "preact-app",
                "web",
            ),
            file(
                "apps/web/package.json",
                "{}\n",
                Class::Shared,
                "preact-app",
                "web",
            ),
            file("README.md", "# hi\n", Class::Seed, "base", "base"),
        ] {
            tree.insert(f.path.clone(), f);
        }
        Lock::from_tree(&t, &map(&[("scope", "acme")]), &instances, &tree)
    }

    /// Ruling C13: snapshots never pin the source, commit or splice version.
    fn redact(text: &str) -> String {
        text.lines()
            .map(|l| {
                for key in ["splice", "source", "commit"] {
                    if l.starts_with(&format!("{key} = ")) {
                        return format!("{key} = \"[redacted]\"");
                    }
                }
                l.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }

    fn write_raw(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir.join(".splice")).unwrap();
        std::fs::write(dir.join(LOCK_PATH), text).unwrap();
    }

    #[test]
    fn roundtrip_matches_contract_shape() {
        let lock = sample_lock();
        let dir = tempfile::tempdir().unwrap();
        lock.write_atomic(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(".splice/lock.toml")).unwrap();
        insta::assert_snapshot!(redact(&text));
        assert_eq!(Lock::read(dir.path()).unwrap().unwrap(), lock);
        assert!(!text.contains("README.md")); // seed not listed
        assert!(!dir.path().join(".splice/lock.toml.tmp").exists());
        assert_eq!(
            lock.files["apps/web/vite.config.ts"].sha256,
            sha256_hex(b"cfg\n")
        );
    }

    #[test]
    fn read_missing_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Lock::read(dir.path()).unwrap(), None);
    }

    #[test]
    fn instances_roundtrip() {
        let lock = sample_lock();
        let inst = lock.instances();
        assert_eq!(inst.len(), 2);
        assert_eq!(inst[1].id, "preact-app");
        assert_eq!(inst[1].name, "web");
        assert_eq!(inst[1].vars, map(&[("name", "web")]));
    }

    #[test]
    fn unknown_format_is_template_error() {
        let dir = tempfile::tempdir().unwrap();
        write_raw(dir.path(), "format = 9\n");
        let err = Lock::read(dir.path()).unwrap_err();
        assert!(matches!(err, SpliceError::Template(_)), "{err}");
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn malformed_lock_is_template_error() {
        let dir = tempfile::tempdir().unwrap();
        write_raw(dir.path(), "format = 1\nsplice = \"not semver\"\n");
        assert_eq!(Lock::read(dir.path()).unwrap_err().exit_code(), 3);
        write_raw(dir.path(), "this is not toml [");
        assert_eq!(Lock::read(dir.path()).unwrap_err().exit_code(), 3);
    }

    #[test]
    fn commit_must_be_40_lowercase_hex() {
        let good = {
            let dir = tempfile::tempdir().unwrap();
            sample_lock().write_atomic(dir.path()).unwrap();
            std::fs::read_to_string(dir.path().join(LOCK_PATH)).unwrap()
        };
        assert!(good.contains(SHA));
        for bad in [
            "../../x",
            "HEAD",
            &SHA[..39],
            &SHA.to_uppercase(),
            &format!("{SHA}0"),
            "",
        ] {
            let dir = tempfile::tempdir().unwrap();
            write_raw(dir.path(), &good.replace(SHA, bad));
            let err = Lock::read(dir.path()).unwrap_err();
            assert!(matches!(err, SpliceError::Template(_)), "{bad}: {err}");
            assert_eq!(err.exit_code(), 3);
        }
    }

    #[test]
    fn seed_entry_in_lock_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        sample_lock().write_atomic(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(LOCK_PATH)).unwrap();
        write_raw(
            dir.path(),
            &text.replace("class = \"owned\"", "class = \"seed\""),
        );
        assert_eq!(Lock::read(dir.path()).unwrap_err().exit_code(), 3);
    }

    fn lock_at(version: &str) -> Lock {
        let mut l = sample_lock();
        l.template.version = semver::Version::parse(version).unwrap();
        l
    }

    fn sample_channel_lock() -> ChannelLock {
        ChannelLock {
            meta: ChannelMeta {
                name: "decisions".into(),
                track: Some("main".into()),
                only: vec![],
                exclude: vec![".claude/skills/caveman/**".into()],
            },
            lock: lock_at("0.0.0+g3f2c1ab"),
            sections: [(
                "docs/agents/project.md".to_string(),
                SectionLock {
                    sha256: "ab".repeat(32),
                    created: false,
                },
            )]
            .into(),
        }
    }

    fn write_at(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn main_text() -> String {
        let dir = tempfile::tempdir().unwrap();
        sample_lock().write_atomic(dir.path()).unwrap();
        std::fs::read_to_string(dir.path().join(LOCK_PATH)).unwrap()
    }

    fn channel_text(name: &str) -> String {
        let dir = tempfile::tempdir().unwrap();
        let mut cl = sample_channel_lock();
        cl.meta.name = name.into();
        cl.write_atomic(dir.path()).unwrap();
        std::fs::read_to_string(dir.path().join(channel_lock_path(name))).unwrap()
    }

    #[test]
    fn channel_lock_round_trip() {
        let cl = sample_channel_lock();
        let dir = tempfile::tempdir().unwrap();
        cl.write_atomic(dir.path()).unwrap();
        assert!(dir.path().join(".splice/channels/decisions.toml").is_file());
        assert_eq!(cl.path(), ".splice/channels/decisions.toml");
        assert_eq!(
            ChannelLock::read(dir.path(), "decisions").unwrap().unwrap(),
            cl
        );
    }

    #[test]
    fn channel_lock_table_order() {
        let text = sample_channel_lock().to_toml();
        let order: Vec<_> = [
            "format =",
            "splice =",
            "[channel]",
            "[template]",
            "[vars]",
            "[[pieces]]",
            "[files]",
            "[sections]",
        ]
        .iter()
        .map(|k| text.find(k).unwrap_or_else(|| panic!("{k} in {text}")))
        .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
    }

    #[test]
    fn main_lock_has_no_channel_tables() {
        let text = main_text();
        assert!(
            !text.contains("[channel]") && !text.contains("[sections]"),
            "{text}"
        );
    }

    #[test]
    fn main_lock_with_channel_table_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("{}\n[channel]\nname = \"x\"\n", main_text());
        write_raw(dir.path(), &text);
        let err = Lock::read(dir.path()).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        let m = err.to_string();
        assert!(
            m.contains(".splice/lock.toml") && m.contains("splice new"),
            "{m}"
        );
    }

    #[test]
    fn main_lock_with_sections_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!("{}\n[sections]\n", main_text());
        write_raw(dir.path(), &text);
        let err = Lock::read(dir.path()).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains(".splice/lock.toml"), "{err}");
    }

    #[test]
    fn channel_lock_without_channel_table_is_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        write_at(dir.path(), ".splice/channels/x.toml", &main_text());
        let err = ChannelLock::read(dir.path(), "x").unwrap_err();
        assert_eq!(err.exit_code(), 3);
        let m = err.to_string();
        assert!(
            m.contains(".splice/channels/x.toml") && m.contains("splice channel add"),
            "{m}"
        );
    }

    #[test]
    fn channel_name_must_match_file() {
        let dir = tempfile::tempdir().unwrap();
        write_at(dir.path(), ".splice/channels/x.toml", &channel_text("y"));
        let err = ChannelLock::read(dir.path(), "x").unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains(".splice/channels/x.toml"), "{err}");
    }

    #[test]
    fn bad_commit_names_channel_lock() {
        let dir = tempfile::tempdir().unwrap();
        let text = channel_text("x").replace(SHA, "zz");
        write_at(dir.path(), ".splice/channels/x.toml", &text);
        let err = ChannelLock::read(dir.path(), "x").unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains(".splice/channels/x.toml"), "{err}");
    }

    #[test]
    fn stray_file_in_channels_dir_is_corrupt() {
        for stray in ["Bad.toml", "notes.txt"] {
            let dir = tempfile::tempdir().unwrap();
            write_at(dir.path(), ".splice/channels/a.toml", &channel_text("a"));
            write_at(dir.path(), &format!(".splice/channels/{stray}"), "x");
            let err = ChannelLock::read_all(dir.path()).unwrap_err();
            assert_eq!(err.exit_code(), 3, "{stray}");
            assert!(err.to_string().contains(stray), "{err}");
        }
    }

    #[test]
    fn dot_files_in_channels_dir_ignored() {
        let dir = tempfile::tempdir().unwrap();
        write_at(dir.path(), ".splice/channels/b.toml", &channel_text("b"));
        write_at(dir.path(), ".splice/channels/a.toml", &channel_text("a"));
        write_at(dir.path(), ".splice/channels/.tmpabc", "partial");
        let all = ChannelLock::read_all(dir.path()).unwrap();
        let names: Vec<_> = all.iter().map(|c| c.meta.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
    }

    #[test]
    fn read_all_without_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(ChannelLock::read_all(dir.path()).unwrap().is_empty());
        assert_eq!(ChannelLock::read(dir.path(), "a").unwrap(), None);
    }

    #[test]
    fn invalid_channel_name_is_rejected_on_read_and_write() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["../x", "A", ""] {
            let e = ChannelLock::read(dir.path(), bad).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{bad}");
            let mut cl = sample_channel_lock();
            cl.meta.name = bad.into();
            let e = cl.write_atomic(dir.path()).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{bad}");
        }
        assert!(!dir.path().join(".splice").exists());
    }

    #[test]
    fn channel_names() {
        for ok in ["a", "decisions", "rig-2"] {
            assert!(valid_channel_name(ok), "{ok}");
        }
        for bad in ["", "A", "1a", "-a", "a_b", "a/b", "a.b"] {
            assert!(!valid_channel_name(bad), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn write_refuses_symlinked_splice_dir() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join(".splice")).unwrap();
        let err = sample_lock().write_atomic(dir.path()).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
    }
}
