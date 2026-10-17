//! Merging of shared files: text here, structured formats in later tasks.

pub mod text;

pub use text::{merge_text, wrap_whole};

/// The result of a merge. `conflicts` holds one entry per conflict: a key path for structured
/// formats, `line N` (1-based, in `text`) for text. A conflicted file keeps diff3 markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    pub conflicts: Vec<String>,
}
