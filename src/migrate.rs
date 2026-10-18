//! Migrations: `migrations/<semver>.toml` (contract §7).

use std::path::Path;

use semver::Version;
use serde::Deserialize;

use crate::error::SpliceError;

type Result<T> = std::result::Result<T, SpliceError>;

/// A command run after the files are written (`[[step]]`).
#[derive(Debug, Clone, Deserialize)]
pub struct Step {
    pub run: String,
    pub description: Option<String>,
}

/// A file the new version no longer ships (`[[remove]]`); `path` may use template variables.
#[derive(Debug, Clone, Deserialize)]
pub struct Remove {
    pub path: String,
    pub piece: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Migration {
    pub version: Version,
    pub steps: Vec<Step>,
    pub removes: Vec<Remove>,
}

#[derive(Deserialize)]
struct RawMigration {
    #[serde(default, rename = "step")]
    steps: Vec<Step>,
    #[serde(default, rename = "remove")]
    removes: Vec<Remove>,
}

/// Load `<root>/migrations/<semver>.toml`, sorted by version. No directory means no migrations;
/// a file not named `<semver>.toml` or not valid is a template error (exit 3).
pub fn load_migrations(root: &Path) -> Result<Vec<Migration>> {
    let dir = root.join("migrations");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut all = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let bad_name = || {
            SpliceError::Template(format!(
                "{}: migration files must be named <semver>.toml, for example 1.2.0.toml",
                path.display()
            ))
        };
        let stem = name.strip_suffix(".toml").ok_or_else(bad_name)?;
        let version = Version::parse(stem).map_err(|_| bad_name())?;
        let text = std::fs::read_to_string(&path)
            .map_err(|e| SpliceError::Template(format!("cannot read {}: {e}", path.display())))?;
        let raw: RawMigration = toml::from_str(&text)
            .map_err(|e| SpliceError::Template(format!("{}: {e}", path.display())))?;
        all.push(Migration {
            version,
            steps: raw.steps,
            removes: raw.removes,
        });
    }
    all.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(all)
}

/// Migrations with `from < version <= to`, in semver order (`all` must be sorted).
pub fn select<'a>(all: &'a [Migration], from: &Version, to: &Version) -> Vec<&'a Migration> {
    all.iter()
        .filter(|m| &m.version > from && &m.version <= to)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn migs(versions: &[&str]) -> Vec<Migration> {
        let mut all: Vec<Migration> = versions
            .iter()
            .map(|v| Migration {
                version: Version::parse(v).unwrap(),
                steps: vec![],
                removes: vec![],
            })
            .collect();
        all.sort_by(|a, b| a.version.cmp(&b.version));
        all
    }

    fn names(sel: Vec<&Migration>) -> Vec<String> {
        sel.iter().map(|m| m.version.to_string()).collect()
    }

    fn write(root: &Path, name: &str, text: &str) {
        let dir = root.join("migrations");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
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

    #[test]
    fn bad_migration_name() {
        for bad in ["latest.toml", "1.0.toml", "1.0.0.txt"] {
            let dir = tempfile::tempdir().unwrap();
            write(dir.path(), bad, "");
            let e = load_migrations(dir.path()).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{bad}");
            assert!(e.to_string().contains(bad), "{e}");
        }
    }

    #[test]
    fn no_migrations_dir_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_migrations(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn loads_steps_and_removes_sorted() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "1.10.0.toml",
            "[[step]]\nrun = \"echo a\"\ndescription = \"A\"\n\n[[step]]\nrun = \"echo b\"\n\n\
             [[remove]]\npath = \"x/{{ name }}.json\"\npiece = \"app\"\n\n[[remove]]\npath = \"y\"\n",
        );
        write(dir.path(), "1.9.0.toml", "");
        let all = load_migrations(dir.path()).unwrap();
        assert_eq!(
            all.iter()
                .map(|m| m.version.to_string())
                .collect::<Vec<_>>(),
            ["1.9.0", "1.10.0"]
        );
        let m = &all[1];
        assert_eq!(m.steps[0].run, "echo a");
        assert_eq!(m.steps[0].description.as_deref(), Some("A"));
        assert_eq!(m.steps[1].description, None);
        assert_eq!(m.removes[0].path, "x/{{ name }}.json");
        assert_eq!(m.removes[0].piece.as_deref(), Some("app"));
        assert_eq!(m.removes[1].piece, None);
    }

    #[test]
    fn invalid_content_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "1.0.0.toml",
            "[[step]]\ndescription = \"no run\"\n",
        );
        let e = load_migrations(dir.path()).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("1.0.0.toml"), "{e}");
    }
}
