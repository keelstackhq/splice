//! Small filesystem helpers.

use std::io::{self, Write};
use std::path::{Component, Path};

use crate::error::SpliceError;

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

type Result<T> = std::result::Result<T, SpliceError>;

fn refuse(rel: &str, why: &str) -> SpliceError {
    SpliceError::Template(format!("refusing to write `{rel}`: {why}"))
}

/// Check that writing `rel` below `root` stays inside `root`: `rel` is relative without `..`, and
/// neither the target nor any existing directory between `root` and it is a symlink.
fn check_target(root: &Path, rel: &str) -> Result<()> {
    let rel_path = Path::new(rel);
    if rel.is_empty() || rel_path.is_absolute() {
        return Err(refuse(rel, "the path must be relative"));
    }
    if rel_path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(refuse(rel, "the path must not contain `..` or a prefix"));
    }
    let mut cur = root.to_path_buf();
    for c in rel_path.components() {
        if let Component::Normal(part) = c {
            cur.push(part);
        } else {
            continue;
        }
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(refuse(
                    rel,
                    &format!(
                        "`{}` is a symlink",
                        cur.strip_prefix(root).unwrap_or(&cur).display()
                    ),
                ));
            }
            Ok(_) => {}
            // Nothing below a missing component exists yet.
            Err(e) if e.kind() == io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Write `rel` below `root`, creating parent directories. Refuses (exit 3, nothing written) an
/// absolute or `..` path and any symlink on the way.
pub fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
    check_target(root, rel)?;
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Like [`write_file`] but atomic: write `<rel>.tmp`, fsync, rename over `rel`. The same checks
/// apply to both paths.
pub fn write_atomic(root: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
    let tmp_rel = format!("{rel}.tmp");
    check_target(root, rel)?;
    check_target(root, &tmp_rel)?;
    let path = root.join(rel);
    let tmp = root.join(&tmp_rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let written = (|| -> io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Read `rel` below `root`; `None` when it does not exist.
pub fn read_opt(root: &Path, rel: &str) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(root.join(rel)) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_empty_and_abc() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn write_creates_parents_and_read_opt_reports_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_opt(dir.path(), "a/b/c.txt").unwrap(), None);
        write_file(dir.path(), "a/b/c.txt", b"hi\n").unwrap();
        assert_eq!(read_opt(dir.path(), "a/b/c.txt").unwrap().unwrap(), b"hi\n");
        write_file(dir.path(), "a/b/c.txt", b"new").unwrap();
        assert_eq!(read_opt(dir.path(), "a/b/c.txt").unwrap().unwrap(), b"new");
    }

    #[cfg(unix)]
    mod hardening {
        use super::*;
        use std::os::unix::fs::symlink;

        #[test]
        fn rejects_absolute_and_dotdot() {
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("repo");
            std::fs::create_dir(&root).unwrap();
            let dir = &root;
            let outside = tempfile::tempdir().unwrap();
            let abs = outside.path().join("x.txt");
            for rel in [abs.to_str().unwrap(), "../x.txt", "a/../../x.txt", ""] {
                let err = write_file(dir, rel, b"x").unwrap_err();
                assert_eq!(err.exit_code(), 3, "{rel}");
                let err = write_atomic(dir, rel, b"x").unwrap_err();
                assert_eq!(err.exit_code(), 3, "{rel}");
            }
            assert!(!abs.exists());
            assert!(!parent.path().join("x.txt").exists());
        }

        #[test]
        fn refuses_symlinked_ancestor_and_target() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            symlink(outside.path(), dir.path().join("link")).unwrap();
            let err = write_file(dir.path(), "link/sub/f.txt", b"x").unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());

            let victim = outside.path().join("victim.txt");
            std::fs::write(&victim, b"keep").unwrap();
            symlink(&victim, dir.path().join("f.txt")).unwrap();
            let err = write_file(dir.path(), "f.txt", b"x").unwrap_err();
            assert_eq!(err.exit_code(), 3);
            let err = write_atomic(dir.path(), "f.txt", b"x").unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
        }

        #[test]
        fn atomic_refuses_symlinked_tmp_and_dir() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let victim = outside.path().join("victim.txt");
            std::fs::write(&victim, b"keep").unwrap();
            symlink(&victim, dir.path().join("f.txt.tmp")).unwrap();
            let err = write_atomic(dir.path(), "f.txt", b"x").unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
            assert!(!dir.path().join("f.txt").exists());

            symlink(outside.path(), dir.path().join(".splice")).unwrap();
            let err = write_atomic(dir.path(), ".splice/lock.toml", b"x").unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert!(!outside.path().join("lock.toml").exists());
            assert!(!outside.path().join("lock.toml.tmp").exists());
        }
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        write_atomic(dir.path(), "d/f.txt", b"one").unwrap();
        write_atomic(dir.path(), "d/f.txt", b"two").unwrap();
        assert_eq!(read_opt(dir.path(), "d/f.txt").unwrap().unwrap(), b"two");
        assert!(!dir.path().join("d/f.txt.tmp").exists());
    }
}
