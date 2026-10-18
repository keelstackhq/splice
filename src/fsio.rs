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
pub fn check_target(root: &Path, rel: &str) -> Result<()> {
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

/// Remove `rel` below `root`, with the same checks as [`write_file`].
pub fn remove_file(root: &Path, rel: &str) -> Result<()> {
    check_target(root, rel)?;
    std::fs::remove_file(root.join(rel))?;
    Ok(())
}

/// Like [`write_file`] but atomic: write an exclusively created, randomly named temp file in the
/// target's directory, fsync it, rename it over `rel` and fsync the directory. A user's own
/// `<rel>.tmp` is never touched. The same checks as [`write_file`] apply.
pub fn write_atomic(root: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
    check_target(root, rel)?;
    let path = root.join(rel);
    let parent = path.parent().unwrap_or(root);
    std::fs::create_dir_all(parent)?;
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    set_mode(tmp.path(), &path)?;
    tmp.persist(&path).map_err(|e| e.error)?;
    sync_dir(parent)?;
    Ok(())
}

/// Temp files are created `0600`; give the result the old file's mode, else `0644`.
#[cfg(unix)]
fn set_mode(tmp: &Path, target: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = match std::fs::metadata(target) {
        Ok(m) => m.permissions().mode() & 0o7777,
        Err(_) => 0o644,
    };
    std::fs::set_permissions(tmp, std::fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_tmp: &Path, _target: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
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
        fn atomic_refuses_symlinked_dir_and_target() {
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            symlink(outside.path(), dir.path().join(".splice")).unwrap();
            let err = write_atomic(dir.path(), ".splice/lock.toml", b"x").unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert!(std::fs::read_dir(outside.path()).unwrap().next().is_none());
        }

        #[test]
        fn atomic_does_not_follow_a_user_tmp_symlink() {
            // The temp name is random: a `<rel>.tmp` symlink is neither used nor touched.
            let dir = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            let victim = outside.path().join("victim.txt");
            std::fs::write(&victim, b"keep").unwrap();
            symlink(&victim, dir.path().join("f.txt.tmp")).unwrap();
            write_atomic(dir.path(), "f.txt", b"x").unwrap();
            assert_eq!(std::fs::read(&victim).unwrap(), b"keep");
            assert_eq!(std::fs::read(dir.path().join("f.txt")).unwrap(), b"x");
        }
    }

    #[test]
    fn atomic_keeps_a_user_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("f.txt.tmp"), b"mine").unwrap();
        write_atomic(dir.path(), "f.txt", b"one").unwrap();
        write_atomic(dir.path(), "f.txt", b"two").unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("f.txt.tmp")).unwrap(),
            b"mine"
        );
        assert_eq!(std::fs::read(dir.path().join("f.txt")).unwrap(), b"two");
        // Nothing else is left behind.
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
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
