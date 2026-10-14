//! Commands (spec §8) and the helpers they share.

use crate::error::SpliceError;
use crate::paths::validate_segment;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

pub mod add;
pub mod new;

type Result<T> = std::result::Result<T, SpliceError>;

pub(crate) fn usage(msg: impl Into<String>) -> SpliceError {
    SpliceError::Usage(msg.into())
}

/// `K=V` flags into a map; the first `=` splits, later flags win.
pub(crate) fn parse_data(raw: &[String]) -> Result<IndexMap<String, String>> {
    let mut m = IndexMap::new();
    for item in raw {
        let (k, v) = item
            .split_once('=')
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| usage(format!("invalid --data `{item}`: expected K=V")))?;
        m.insert(k.to_string(), v.to_string());
    }
    Ok(m)
}

/// A name that becomes a path segment must be safe: a bad value is a usage error (exit 2).
pub(crate) fn safe_segment(what: &str, v: &str) -> Result<()> {
    validate_segment(v).map_err(|e| usage(format!("{what} {v:?} is not a safe name: {e}")))
}

/// Refuse to write through a symlink or over a directory below `dest` (checked before any write).
fn unsafe_path(msg: String) -> SpliceError {
    SpliceError::Template(format!("unsafe path: {msg}"))
}

pub(crate) fn check_target(dest: &Path, rel: &str) -> Result<PathBuf> {
    let mut p = dest.to_path_buf();
    let segs: Vec<&str> = rel.split('/').collect();
    for (i, seg) in segs.iter().enumerate() {
        p.push(seg);
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(unsafe_path(format!(
                    "refusing to write through symlink {}",
                    p.display()
                )));
            }
            Ok(m) if i + 1 == segs.len() && m.is_dir() => {
                return Err(unsafe_path(format!("{rel} exists and is a directory")));
            }
            Ok(m) if i + 1 < segs.len() && !m.is_dir() => {
                return Err(unsafe_path(format!(
                    "{} exists and is not a directory",
                    p.display()
                )));
            }
            _ => {}
        }
    }
    Ok(p)
}
