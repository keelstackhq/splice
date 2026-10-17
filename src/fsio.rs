//! Small filesystem helpers.

use std::io;
use std::path::Path;

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Write `rel` below `root`, creating parent directories.
pub fn write_file(root: &Path, rel: &str, bytes: &[u8]) -> io::Result<()> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)
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
}
