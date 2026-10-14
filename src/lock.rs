//! `.splice/lock.toml` (contract §6, spec §8.6).

use crate::error::SpliceError;
use crate::fetch::ResolvedTemplate;
use crate::fsio::{read_opt, sha256_hex};
use crate::output::Class;
use crate::paths::{validate_rel_path, validate_segment};
use crate::render::{PieceInstance, RenderedTree};
use crate::source::TemplateSource;
use indexmap::IndexMap;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value, value};

type Result<T> = std::result::Result<T, SpliceError>;

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

const LOCK_REL: &str = ".splice/lock.toml";
const LOCK_TMP_REL: &str = ".splice/lock.toml.tmp";

fn bad(msg: impl std::fmt::Display) -> SpliceError {
    SpliceError::Template(format!("{LOCK_REL}: {msg}"))
}

/// Re-label any error from a validator so the message names the lock and the exit code is 3.
fn in_lock(e: SpliceError) -> SpliceError {
    bad(e)
}

#[derive(Deserialize)]
struct RawLock {
    format: u32,
    splice: String,
    template: RawTemplate,
    #[serde(default)]
    vars: IndexMap<String, String>,
    #[serde(default)]
    pieces: Vec<RawPiece>,
    #[serde(default)]
    files: BTreeMap<String, RawFile>,
    #[serde(flatten)]
    extra: IndexMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct RawTemplate {
    source: String,
    version: String,
    commit: String,
    #[serde(flatten)]
    extra: IndexMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct RawPiece {
    id: String,
    name: String,
    #[serde(default)]
    vars: IndexMap<String, String>,
    #[serde(flatten)]
    extra: IndexMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct RawFile {
    class: Class,
    sha256: String,
    #[serde(flatten)]
    extra: IndexMap<String, toml::Value>,
}

fn is_hex(s: &str, lens: &[usize]) -> bool {
    lens.contains(&s.len()) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn check_var_names<'a>(names: impl Iterator<Item = &'a String>, ctx: &str) -> Result<()> {
    for n in names {
        let mut chars = n.chars();
        let ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if !ok {
            return Err(bad(format!(
                "{ctx} var name {n:?} must match ^[a-z_][a-z0-9_]*$"
            )));
        }
    }
    Ok(())
}

fn parse_version(what: &str, v: &str) -> Result<semver::Version> {
    semver::Version::parse(v).map_err(|e| bad(format!("invalid {what} {v:?}: {e}")))
}

impl Lock {
    pub fn from_tree(
        t: &ResolvedTemplate,
        vars: &IndexMap<String, String>,
        instances: &[PieceInstance],
        tree: &RenderedTree,
    ) -> Lock {
        let files = tree
            .iter()
            .filter(|(_, f)| f.class != Class::Seed)
            .map(|(path, f)| {
                (
                    path.clone(),
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
                .expect("crate version is semver"),
            template: LockTemplate {
                source: t.source.spec.clone(),
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

    /// Read and fully validate `repo/.splice/lock.toml`; `None` when absent. The lock is
    /// committed and therefore untrusted: nothing in it may reach git or the filesystem
    /// unchecked.
    pub fn read(repo: &Path) -> Result<Option<Lock>> {
        Ok(Self::read_with_warnings(repo)?.map(|(l, _)| l))
    }

    /// Like [`Lock::read`], also returning one warning per unknown key (ignored).
    pub fn read_with_warnings(repo: &Path) -> Result<Option<(Lock, Vec<String>)>> {
        for rel in [".splice", LOCK_REL] {
            match std::fs::symlink_metadata(repo.join(rel)) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(bad(format!(
                        "{rel} is a symlink; refusing to read through it"
                    )));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
        let Some(bytes) = read_opt(repo, LOCK_REL)? else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes).map_err(|_| bad("not valid UTF-8"))?;
        let raw: RawLock = toml::from_str(&text).map_err(|e| bad(e.to_string().trim_end()))?;
        if raw.format != 1 {
            return Err(bad(format!(
                "unsupported lock format {} (this splice reads format 1); upgrade splice",
                raw.format
            )));
        }
        let mut warnings = Vec::new();
        let mut warn = |ctx: &str, extra: &IndexMap<String, toml::Value>| {
            for k in extra.keys() {
                warnings.push(format!("{LOCK_REL}: unknown key `{ctx}{k}` ignored"));
            }
        };
        warn("", &raw.extra);
        warn("template.", &raw.template.extra);
        for p in &raw.pieces {
            warn("pieces.", &p.extra);
        }
        for (path, f) in &raw.files {
            warn(&format!("files.{path}."), &f.extra);
        }
        let source = TemplateSource::parse(&raw.template.source).map_err(in_lock)?;
        if !is_hex(&raw.template.commit, &[40, 64]) {
            return Err(bad(format!(
                "template.commit {:?} is not a full lowercase hex sha",
                raw.template.commit
            )));
        }
        check_var_names(raw.vars.keys(), "[vars]")?;
        let mut pieces = Vec::new();
        for p in raw.pieces {
            validate_segment(&p.id).map_err(in_lock)?;
            validate_segment(&p.name).map_err(in_lock)?;
            if pieces
                .iter()
                .any(|q: &LockPiece| q.id == p.id && q.name == p.name)
            {
                return Err(bad(format!(
                    "duplicate piece instance `{}:{}` in [[pieces]]",
                    p.id, p.name
                )));
            }
            check_var_names(p.vars.keys(), &format!("piece `{}`", p.id))?;
            pieces.push(LockPiece {
                id: p.id,
                name: p.name,
                vars: p.vars,
            });
        }
        let mut files = BTreeMap::new();
        for (path, f) in raw.files {
            validate_rel_path(&path).map_err(in_lock)?;
            if f.class == Class::Seed {
                return Err(bad(format!(
                    "file {path:?} has class seed; seeds are never listed"
                )));
            }
            if !is_hex(&f.sha256, &[64]) {
                return Err(bad(format!("file {path:?} has an invalid sha256")));
            }
            files.insert(
                path,
                LockFile {
                    class: f.class,
                    sha256: f.sha256,
                },
            );
        }
        let lock = Lock {
            format: 1,
            splice: parse_version("splice version", &raw.splice)?,
            template: LockTemplate {
                source: source.spec,
                version: parse_version("template.version", &raw.template.version)?,
                commit: raw.template.commit,
            },
            vars: raw.vars,
            pieces,
            files,
        };
        Ok(Some((lock, warnings)))
    }

    fn to_toml(&self) -> String {
        fn inline(m: &IndexMap<String, String>) -> InlineTable {
            let mut t = InlineTable::new();
            for (k, v) in m {
                t.insert(k, Value::from(v.as_str()));
            }
            t
        }
        let mut doc = DocumentMut::new();
        doc["format"] = value(i64::from(self.format));
        doc["splice"] = value(self.splice.to_string());

        let mut template = Table::new();
        template["source"] = value(self.template.source.as_str());
        template["version"] = value(self.template.version.to_string());
        template["commit"] = value(self.template.commit.as_str());
        doc["template"] = Item::Table(template);

        let mut vars = Table::new();
        for (k, v) in &self.vars {
            vars[k.as_str()] = value(v.as_str());
        }
        doc["vars"] = Item::Table(vars);

        let mut pieces = ArrayOfTables::new();
        for p in &self.pieces {
            let mut t = Table::new();
            t["id"] = value(p.id.as_str());
            t["name"] = value(p.name.as_str());
            t["vars"] = value(inline(&p.vars));
            pieces.push(t);
        }
        doc["pieces"] = Item::ArrayOfTables(pieces);

        let mut files = Table::new();
        for (path, f) in &self.files {
            let mut e = InlineTable::new();
            e.insert("class", Value::from(class_str(f.class)));
            e.insert("sha256", Value::from(f.sha256.as_str()));
            files[path.as_str()] = value(e);
        }
        doc["files"] = Item::Table(files);
        doc.to_string()
    }

    /// Write `.splice/lock.toml.tmp`, fsync it, rename it over the lock. The temp file is
    /// removed on any failure, so a crash or error leaves the old lock in place.
    pub fn write_atomic(&self, repo: &Path) -> Result<()> {
        let tmp = repo.join(LOCK_TMP_REL);
        let dest = repo.join(LOCK_REL);
        let dir = repo.join(".splice");
        // A committed `.splice` symlink must not redirect the delete/write below.
        match std::fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                return Err(bad(
                    ".splice is not a real directory (symlink or file); refusing to write",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::create_dir_all(&dir)?,
            Err(e) => return Err(e.into()),
        }
        // Never write through a pre-existing file or symlink at the temp path.
        match std::fs::remove_file(&tmp) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let result = (|| -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)?;
            f.write_all(self.to_toml().as_bytes())?;
            f.sync_all()?;
            drop(f);
            std::fs::rename(&tmp, &dest)
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
        #[cfg(unix)]
        // Best effort: the rename already succeeded, and some filesystems cannot fsync a
        // directory, so a failure here must not report the lock as unwritten.
        if let Ok(d) = std::fs::File::open(&dir) {
            let _ = d.sync_all();
        }
        Ok(())
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

fn class_str(c: Class) -> &'static str {
    match c {
        Class::Owned => "owned",
        Class::Shared => "shared",
        Class::Seed => "seed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsio::sha256_hex;
    use crate::manifest::MergeStrategy;
    use crate::render::RenderedFile;
    use crate::source::TemplateSource;

    const COMMIT: &str = "3f2c0a1b3f2c0a1b3f2c0a1b3f2c0a1b3f2c0a1b";

    fn resolved() -> ResolvedTemplate {
        ResolvedTemplate {
            source: TemplateSource::parse("gh:keelstack/hull//template").unwrap(),
            version: semver::Version::new(1, 2, 0),
            commit: COMMIT.into(),
            root: "/unused".into(),
        }
    }

    fn vars(kv: &[(&str, &str)]) -> IndexMap<String, String> {
        kv.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn file(path: &str, class: Class, bytes: &[u8]) -> (String, RenderedFile) {
        (
            path.to_string(),
            RenderedFile {
                path: path.into(),
                bytes: bytes.to_vec(),
                class,
                strategy: MergeStrategy::Text,
                piece_id: "preact-app".into(),
                piece_name: "web".into(),
                glob: None,
            },
        )
    }

    fn sample_lock() -> Lock {
        let tree: RenderedTree = [
            file("apps/web/vite.config.ts", Class::Owned, b"owned\n"),
            file("apps/web/package.json", Class::Shared, b"{}\n"),
            file("README.md", Class::Seed, b"seed\n"),
        ]
        .into_iter()
        .collect();
        let inst = [PieceInstance {
            id: "preact-app".into(),
            name: "web".into(),
            vars: vars(&[("name", "web")]),
        }];
        Lock::from_tree(
            &resolved(),
            &vars(&[("scope", "acme"), ("env", "prod")]),
            &inst,
            &tree,
        )
    }

    fn write_raw(dir: &Path, text: &str) {
        std::fs::create_dir_all(dir.join(".splice")).unwrap();
        std::fs::write(dir.join(".splice/lock.toml"), text).unwrap();
    }

    fn valid_text() -> String {
        let d = tempfile::tempdir().unwrap();
        sample_lock().write_atomic(d.path()).unwrap();
        std::fs::read_to_string(d.path().join(".splice/lock.toml")).unwrap()
    }

    fn read_err(text: &str) -> SpliceError {
        let d = tempfile::tempdir().unwrap();
        write_raw(d.path(), text);
        Lock::read(d.path()).unwrap_err()
    }

    #[test]
    fn roundtrip_matches_contract_shape() {
        let lock = sample_lock();
        let dir = tempfile::tempdir().unwrap();
        lock.write_atomic(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(".splice/lock.toml")).unwrap();
        insta::assert_snapshot!(text);
        assert_eq!(Lock::read(dir.path()).unwrap().unwrap(), lock);
        assert!(!text.contains("README.md"));
        assert!(!dir.path().join(".splice/lock.toml.tmp").exists());
        assert_eq!(
            lock.files["apps/web/vite.config.ts"].sha256,
            sha256_hex(b"owned\n")
        );
    }

    #[test]
    fn missing_lock_is_none() {
        let d = tempfile::tempdir().unwrap();
        assert!(Lock::read(d.path()).unwrap().is_none());
    }

    #[test]
    fn instances_roundtrip() {
        let inst = sample_lock().instances();
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].id, "preact-app");
        assert_eq!(inst[0].name, "web");
        assert_eq!(inst[0].vars, vars(&[("name", "web")]));
    }

    #[test]
    fn unknown_format_is_template_error() {
        let e = read_err(&valid_text().replace("format = 1", "format = 9"));
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("lock.toml"), "{e}");
    }

    #[test]
    fn malformed_toml_is_template_error() {
        let e = read_err("format = [");
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("lock.toml"), "{e}");
    }

    #[test]
    fn hostile_lock_is_rejected_naming_the_lock() {
        let good = valid_text();
        let cases = [
            (
                "source = \"gh:keelstack/hull//template\"",
                "source = \"--upload-pack=evil\"",
            ),
            (
                "source = \"gh:keelstack/hull//template\"",
                "source = \"ext::sh -c evil\"",
            ),
            ("source = \"gh:keelstack/hull//template\"", "source = \"\""),
            (COMMIT, "--output=x"),
            (COMMIT, "abc"),
            ("\"apps/web/vite.config.ts\"", "\"../../etc/passwd\""),
            ("\"apps/web/vite.config.ts\"", "\"/abs\""),
            ("\"apps/web/vite.config.ts\"", "\"a//b\""),
            ("\"apps/web/vite.config.ts\"", "\"a\\\\b\""),
            ("\"apps/web/vite.config.ts\"", "\"C:/x\""),
            ("\"apps/web/vite.config.ts\"", "\".splice/lock.toml\""),
            ("\"apps/web/vite.config.ts\"", "\"nul\""),
            ("class = \"owned\"", "class = \"seed\""),
            ("class = \"owned\"", "class = \"bogus\""),
            ("scope = \"acme\"", "\"Bad Name\" = \"acme\""),
            ("id = \"preact-app\"", "id = \"../x\""),
            ("name = \"web\"\nvars", "name = \"a/b\"\nvars"),
            ("vars = { name = \"web\" }", "vars = { \"x y\" = \"web\" }"),
            ("version = \"1.2.0\"", "version = \"nope\""),
            ("splice = \"0.1.0\"", "splice = \"nope\""),
        ];
        for (from, to) in cases {
            assert!(good.contains(from), "fixture lacks {from}");
            let e = read_err(&good.replace(from, to));
            assert_eq!(e.exit_code(), 3, "{to}: {e}");
            assert!(e.to_string().contains("lock.toml"), "{to}: {e}");
        }
        let bad_sha = good.replace(&sha256_hex(b"owned\n"), "zz");
        assert_eq!(read_err(&bad_sha).exit_code(), 3);
    }

    #[test]
    fn duplicate_piece_instance_is_rejected() {
        let good = valid_text();
        let start = good.find("[[pieces]]").expect("fixture has a piece");
        let end = good[start..]
            .find("\n\n")
            .map_or(good.len(), |i| start + i + 1);
        let block = &good[start..end];
        let dup = good.replacen(block, &format!("{block}\n{block}"), 1);
        let e = read_err(&dup);
        assert_eq!(e.exit_code(), 3, "{e}");
        let m = e.to_string();
        assert!(m.contains("lock.toml") && m.contains("duplicate"), "{m}");
    }

    #[test]
    fn failed_write_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        // A directory where the lock should go makes the final rename fail.
        std::fs::create_dir_all(dir.path().join(".splice/lock.toml")).unwrap();
        assert!(sample_lock().write_atomic(dir.path()).is_err());
        assert!(!dir.path().join(".splice/lock.toml.tmp").exists());
    }

    #[test]
    fn overwrite_replaces_existing_lock() {
        let dir = tempfile::tempdir().unwrap();
        let mut lock = sample_lock();
        lock.write_atomic(dir.path()).unwrap();
        lock.template.version = semver::Version::new(2, 0, 0);
        lock.write_atomic(dir.path()).unwrap();
        assert_eq!(Lock::read(dir.path()).unwrap().unwrap(), lock);
    }

    #[test]
    fn var_order_is_preserved_on_read() {
        let d = tempfile::tempdir().unwrap();
        let mut lock = sample_lock();
        lock.pieces[0].vars = vars(&[("name", "web"), ("kind", "x"), ("a", "y")]);
        lock.write_atomic(d.path()).unwrap();
        let back = Lock::read(d.path()).unwrap().unwrap();
        assert!(back.vars.keys().eq(["scope", "env"]));
        assert!(back.pieces[0].vars.keys().eq(["name", "kind", "a"]));
    }

    #[test]
    fn unknown_keys_produce_warnings() {
        let text = valid_text()
            .replace("format = 1", "format = 1\nextra = 1")
            .replace("[vars]", "[template.more]\nx = 1\n\n[vars]");
        let d = tempfile::tempdir().unwrap();
        write_raw(d.path(), &text);
        let (_, w) = Lock::read_with_warnings(d.path()).unwrap().unwrap();
        assert!(w.iter().any(|m| m.contains("extra")), "{w:?}");
        assert!(w.iter().any(|m| m.contains("template.more")), "{w:?}");
        assert!(Lock::read(d.path()).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_splice_dir_is_refused() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join(".splice")).unwrap();
        std::fs::write(outside.path().join("lock.toml.tmp"), "precious").unwrap();
        let e = sample_lock().write_atomic(repo.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains(".splice"), "{e}");
        assert_eq!(
            std::fs::read_to_string(outside.path().join("lock.toml.tmp")).unwrap(),
            "precious"
        );
        assert!(!outside.path().join("lock.toml").exists());

        std::fs::write(outside.path().join("lock.toml"), valid_text()).unwrap();
        let e = Lock::read(repo.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_lock_file_is_refused() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("real.toml"), valid_text()).unwrap();
        std::fs::create_dir(repo.path().join(".splice")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("real.toml"),
            repo.path().join(".splice/lock.toml"),
        )
        .unwrap();
        let e = Lock::read(repo.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("symlink"), "{e}");
    }
}
