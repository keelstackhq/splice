//! `SpliceError` and its exit codes (contract §8).

use crate::output::Report;

#[derive(Debug, thiserror::Error)]
pub enum SpliceError {
    /// Bad flags, missing value, dirty tree, ...: exit 2.
    #[error("{0}")]
    Usage(String),
    /// Bad manifest, render error, unsafe path, ...: exit 3.
    #[error("{0}")]
    Template(String),
    /// Git failure, failed post command, ...: exit 3.
    #[error("{0}")]
    Env(String),
    /// Conflicts written or update refused: exit 1.
    #[error("conflicts in {} file(s)", .0.changes.len())]
    Conflict(Report),
    /// Drift, markers or an available update: exit 1.
    #[error("drift detected")]
    Drift(Report),
    /// A write, migration step, post command or the lock write failed after files were written:
    /// exit 3. The report lists the changes already applied; its last message is the error.
    #[error("{}", .0.messages.last().map_or("", String::as_str))]
    Applied(Report),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl SpliceError {
    pub fn exit_code(&self) -> i32 {
        match self {
            SpliceError::Usage(_) => 2,
            SpliceError::Template(_)
            | SpliceError::Env(_)
            | SpliceError::Applied(_)
            | SpliceError::Io(_) => 3,
            SpliceError::Conflict(_) | SpliceError::Drift(_) => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        assert_eq!(SpliceError::Usage("x".into()).exit_code(), 2);
        assert_eq!(SpliceError::Template("x".into()).exit_code(), 3);
        assert_eq!(SpliceError::Conflict(Report::new("update")).exit_code(), 1);
    }
}
