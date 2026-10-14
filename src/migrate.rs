//! Migrations `migrations/<semver>.toml` (contract §7).

use crate::error::SpliceError;
use crate::manifest::{Extra, bad, read_toml, reject_symlink, warn_extra};
use crate::{paths, subset};
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
    #[serde(flatten)]
    extra: Extra,
}

#[derive(serde::Deserialize)]
struct RawRemove {
    path: String,
    piece: Option<String>,
    #[serde(flatten)]
    extra: Extra,
}

#[derive(serde::Deserialize)]
struct RawMigration {
    #[serde(default)]
    step: Vec<RawStep>,
    #[serde(default)]
    remove: Vec<RawRemove>,
    #[serde(flatten)]
    extra: Extra,
}

/// A `[[remove]].path` is a template path: `{{ expr }}` allowed, otherwise the rules of
/// rendered paths (relative, safe segments, not the lock file).
fn check_remove_path(path: &str, label: &str) -> Result<()> {
    let fail = |msg: &str| bad(label, format!("remove.path {path:?}: {msg}"));
    if path.contains('\\') {
        return Err(fail("backslash in path"));
    }
    if path.as_bytes().get(1) == Some(&b':') {
        return Err(fail("drive paths are not allowed"));
    }
    if path.eq_ignore_ascii_case(".splice/lock.toml") {
        return Err(fail("`.splice/lock.toml` is reserved for the lock file"));
    }
    for seg in path.split('/') {
        subset::validate_path_segment(seg, label)?;
        if !seg.contains("{{") {
            paths::validate_segment(seg).map_err(|e| fail(&e.to_string()))?;
        }
    }
    Ok(())
}

/// Load `migrations/<semver>.toml`, sorted by version. A missing directory is no migrations;
/// symlinks, bad file names and bad contents are template errors (exit 3).
pub fn load_migrations(root: &Path, warnings: &mut Vec<String>) -> Result<Vec<Migration>> {
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
            .filter(|v| v.build.is_empty())
            .ok_or_else(|| {
                bad(
                    &label,
                    "migration file names must be <semver>.toml without build metadata",
                )
            })?;
        let raw: RawMigration = read_toml(&e.path(), &label)?;
        warn_extra(warnings, &label, "", &raw.extra);
        for st in &raw.step {
            warn_extra(warnings, &label, "step.", &st.extra);
        }
        for r in &raw.remove {
            warn_extra(warnings, &label, "remove.", &r.extra);
            check_remove_path(&r.path, &label)?;
        }
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

/// Migrations with `from < version <= to`, in semver order.
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
        let e = load_migrations(d.path(), &mut vec![]).unwrap_err();
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
        let m = load_migrations(d.path(), &mut vec![]).unwrap();
        assert_eq!(names(m.iter().collect()), ["1.9.0", "1.10.0"]);
        assert_eq!(m[0].steps[1].description.as_deref(), Some("d"));
        assert_eq!(m[0].steps[0].description, None);
        assert_eq!(m[0].removes[1].piece.as_deref(), Some("p"));
        assert_eq!(m[0].removes[0].piece, None);
    }

    fn rm(path: &str) -> String {
        format!("[[remove]]\npath = {path:?}\n")
    }

    #[test]
    fn unsafe_remove_paths_are_exit_3() {
        for bad in [
            "../x",
            "/etc/x",
            "C:/x",
            "a\\b",
            "a//b",
            "./a",
            "",
            ".splice/lock.toml",
            "a/.git/hooks",
        ] {
            let d = write_dir(&[("1.0.0.toml", &rm(bad))]);
            let e = load_migrations(d.path(), &mut vec![]).unwrap_err();
            assert_eq!(e.exit_code(), 3, "{bad}");
            assert!(
                e.to_string().contains("migrations/1.0.0.toml"),
                "{bad}: {e}"
            );
        }
        let d = write_dir(&[("1.0.0.toml", &rm("apps/{{ name }}/.eslintrc.json"))]);
        assert!(load_migrations(d.path(), &mut vec![]).is_ok());
        let d = write_dir(&[("1.0.0.toml", &rm("a/{% if x %}b{% endif %}"))]);
        assert!(load_migrations(d.path(), &mut vec![]).is_err());
    }

    #[test]
    fn unknown_keys_warn() {
        let d = write_dir(&[(
            "1.0.0.toml",
            "colour = 1\n[[step]]\nrun = \"x\"\nwhen = 1\n[[remove]]\npath = \"a\"\nwhy = 1\n",
        )]);
        let mut w = vec![];
        load_migrations(d.path(), &mut w).unwrap();
        assert_eq!(w.len(), 3, "{w:?}");
        assert!(
            w.iter().all(|m| m.contains("migrations/1.0.0.toml")),
            "{w:?}"
        );
        assert!(w.iter().any(|m| m.contains("colour")));
        assert!(w.iter().any(|m| m.contains("when")));
        assert!(w.iter().any(|m| m.contains("why")));
    }

    #[test]
    fn build_metadata_name_is_rejected() {
        let d = write_dir(&[("1.0.0+x.toml", "")]);
        let e = load_migrations(d.path(), &mut vec![]).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("1.0.0+x.toml"), "{e}");
    }

    #[test]
    fn prerelease_orders_before_release() {
        let all = migs(&["1.0.0", "1.0.0-rc.1", "0.9.0"]);
        let v = |s| Version::parse(s).unwrap();
        assert_eq!(
            names(select(&all, &v("0.9.0"), &v("1.0.0"))),
            ["1.0.0-rc.1", "1.0.0"]
        );
        assert!(select(&all, &v("1.0.0"), &v("1.0.0")).is_empty());
    }

    #[test]
    fn missing_dir_is_empty_and_bad_content_is_exit_3() {
        let d = tempfile::tempdir().unwrap();
        assert!(load_migrations(d.path(), &mut vec![]).unwrap().is_empty());
        let d = write_dir(&[("1.0.0.toml", "[[step]]\ndescription = \"no run\"\n")]);
        let e = load_migrations(d.path(), &mut vec![]).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("1.0.0.toml"), "{e}");
    }
}
