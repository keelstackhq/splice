//! `.splice/lock.toml` (contract §6): read, build from a rendered tree, atomic write.

use std::collections::BTreeMap;
use std::path::Path;

use indexmap::IndexMap;

use crate::error::SpliceError;
use crate::fetch::{ResolvedTemplate, validate_commit};
use crate::fsio::{self, sha256_hex};
use crate::output::Class;
use crate::render::{PieceInstance, RenderedTree};

type Result<T> = std::result::Result<T, SpliceError>;

pub const LOCK_PATH: &str = ".splice/lock.toml";

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

fn bad_lock(why: impl std::fmt::Display) -> SpliceError {
    SpliceError::Template(format!(
        "{LOCK_PATH}: {why}; restore it from git or re-run `splice new`"
    ))
}

fn parse_version(what: &str, v: &str) -> Result<semver::Version> {
    semver::Version::parse(v).map_err(|e| bad_lock(format!("`{what}` = \"{v}\": {e}")))
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

    /// `None` when the repo has no lock. An unknown `format`, a malformed lock or a commit that is
    /// not a full lowercase sha is a template/environment error (exit 3).
    pub fn read(repo: &Path) -> Result<Option<Lock>> {
        let Some(bytes) = fsio::read_opt(repo, LOCK_PATH)? else {
            return Ok(None);
        };
        let text = String::from_utf8(bytes).map_err(|_| bad_lock("not valid UTF-8"))?;
        let table: toml::Table = text.parse().map_err(bad_lock)?;
        match table.get("format").and_then(toml::Value::as_integer) {
            Some(1) => {}
            Some(n) => {
                return Err(SpliceError::Template(format!(
                    "{LOCK_PATH}: format {n} is not supported by this splice (supports 1); \
                     upgrade splice"
                )));
            }
            None => return Err(bad_lock("missing integer `format`")),
        }
        let raw: RawLock = table.try_into().map_err(bad_lock)?;
        validate_commit(&raw.template.commit)?;
        let mut files = BTreeMap::new();
        for (path, f) in raw.files {
            let class = match f.class.as_str() {
                "owned" => Class::Owned,
                "shared" => Class::Shared,
                other => {
                    return Err(bad_lock(format!(
                        "file `{path}`: class `{other}` is not allowed (owned or shared)"
                    )));
                }
            };
            files.insert(
                path,
                LockFile {
                    class,
                    sha256: f.sha256,
                },
            );
        }
        Ok(Some(Lock {
            format: 1,
            splice: parse_version("splice", &raw.splice)?,
            template: LockTemplate {
                source: raw.template.source,
                version: parse_version("template.version", &raw.template.version)?,
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
        }))
    }

    fn to_toml(&self) -> String {
        use toml_edit::{DocumentMut, InlineTable, Table, value};

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
        doc.to_string()
    }

    /// Write `.splice/lock.toml` via `.splice/lock.toml.tmp`, fsync and rename. The destination is
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
