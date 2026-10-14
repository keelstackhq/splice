//! Migrations `migrations/<semver>.toml` (contract §7).

use crate::error::SpliceError;
use crate::manifest::{bad, read_toml, reject_symlink};
use semver::Version;
use std::path::Path;

type Result<T> = std::result::Result<T, SpliceError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub run: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remove {
    pub path: String,
    pub piece: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Migration {
    pub version: Version,
    pub steps: Vec<Step>,
    pub removes: Vec<Remove>,
}

#[derive(serde::Deserialize)]
struct RawStep {
    run: String,
    description: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawRemove {
    path: String,
    piece: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawMigration {
    #[serde(default)]
    step: Vec<RawStep>,
    #[serde(default)]
    remove: Vec<RawRemove>,
}

/// Load `migrations/<semver>.toml`, sorted by version. A missing directory is no migrations;
/// symlinks, bad file names and bad contents are template errors (exit 3).
pub fn load_migrations(root: &Path) -> Result<Vec<Migration>> {
    let dir = root.join("migrations");
    reject_symlink(&dir, "migrations")?;
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for e in std::fs::read_dir(&dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let label = format!("migrations/{name}");
        reject_symlink(&e.path(), &label)?;
        let version = name
            .strip_suffix(".toml")
            .and_then(|stem| Version::parse(stem).ok())
            .ok_or_else(|| bad(&label, "migration file names must be <semver>.toml"))?;
        let raw: RawMigration = read_toml(&e.path(), &label)?;
        out.push(Migration {
            version,
            steps: raw
                .step
                .into_iter()
                .map(|s| Step {
                    run: s.run,
                    description: s.description,
                })
                .collect(),
            removes: raw
                .remove
                .into_iter()
                .map(|r| Remove {
                    path: r.path,
                    piece: r.piece,
                })
                .collect(),
        });
    }
    out.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(out)
}

/// Migrations with `from < version <= to`, in semver order .
pub fn select<'a>(all: &'a [Migration], from: &Version, to: &Version) -> Vec<&'a Migration> {
    let mut v: Vec<&Migration> = all
        .iter()
        .filter(|m| &m.version > from && &m.version <= to)
        .collect();
    v.sort_by(|a, b| a.version.cmp(&b.version));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn migs(vs: &[&str]) -> Vec<Migration> {
        vs.iter()
            .map(|v| Migration {
                version: Version::parse(v).unwrap(),
                steps: vec![],
                removes: vec![],
            })
            .collect()
    }

    fn names(m: Vec<&Migration>) -> Vec<String> {
        m.iter().map(|m| m.version.to_string()).collect()
    }

    #[test]
    fn selects_range_in_semver_order() {
        let all = migs(&["1.2.0", "1.10.0", "2.0.0", "1.9.0"]);
        let v = |s| Version::parse(s).unwrap();
        assert_eq!(
            names(select(&all, &v("1.2.0"), &v("2.0.0"))),
            ["1.9.0", "1.10.0", "2.0.0"]
        );
        assert!(select(&all, &v("2.0.0"), &v("2.0.0")).is_empty());
    }

    fn write_dir(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("migrations")).unwrap();
        for (n, c) in files {
            std::fs::write(d.path().join("migrations").join(n), c).unwrap();
        }
        d
    }

    #[test]
    fn bad_migration_name() {
        let d = write_dir(&[("latest.toml", "")]);
        let e = load_migrations(d.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("latest.toml"), "{e}");
    }

    #[test]
    fn loads_sorted_and_parses_contents() {
        let d = write_dir(&[
            ("1.10.0.toml", ""),
            (
                "1.9.0.toml",
                "[[step]]\nrun = \"x\"\n[[step]]\nrun = \"y\"\ndescription = \"d\"\n[[remove]]\npath = \"a\"\n[[remove]]\npath = \"b\"\npiece = \"p\"\n",
            ),
        ]);
        let m = load_migrations(d.path()).unwrap();
        assert_eq!(names(m.iter().collect()), ["1.9.0", "1.10.0"]);
        assert_eq!(m[0].steps[1].description.as_deref(), Some("d"));
        assert_eq!(m[0].steps[0].description, None);
        assert_eq!(m[0].removes[1].piece.as_deref(), Some("p"));
        assert_eq!(m[0].removes[0].piece, None);
    }

    #[test]
    fn missing_dir_is_empty_and_bad_content_is_exit_3() {
        let d = tempfile::tempdir().unwrap();
        assert!(load_migrations(d.path()).unwrap().is_empty());
        let d = write_dir(&[("1.0.0.toml", "[[step]]\ndescription = \"no run\"\n")]);
        let e = load_migrations(d.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("1.0.0.toml"), "{e}");
    }
}
