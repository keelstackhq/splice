//! One module per command.

pub mod add;
pub mod adopt;
pub mod channel;
pub mod check;
pub mod diff;
pub mod explain;
pub mod new;
pub mod pieces;
pub mod update;

/// Exit 2: the command needs a lock and the directory has none.
pub(crate) fn no_lock(repo: &std::path::Path) -> crate::error::SpliceError {
    crate::error::SpliceError::Usage(format!(
        "no {} in {}; run `splice new <source>` for a new repo or `splice adopt <source>` for an \
         existing one (or pass -C <dir>)",
        crate::lock::LOCK_PATH,
        repo.canonicalize()
            .unwrap_or_else(|_| repo.to_path_buf())
            .display()
    ))
}
