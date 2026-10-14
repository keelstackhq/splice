//! Three-way merges. One module per strategy.

pub mod text;

/// Result of a merge: the merged text and a description of each conflict
/// (a key path, or `line N` for text conflicts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    pub text: String,
    pub conflicts: Vec<String>,
}
