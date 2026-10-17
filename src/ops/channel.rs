//! `splice channel add|remove` (channels spec §3). Stubs until Tasks 10 and 17.

use crate::cli::{ChannelAddArgs, ChannelRemoveArgs, GlobalArgs};
use crate::error::SpliceError;
use crate::output::Report;

type Result<T> = std::result::Result<T, SpliceError>;

/// The usage error of a flag or command whose behaviour lands in a later task.
pub fn not_implemented(what: &str) -> SpliceError {
    SpliceError::Usage(format!("splice {what}: not implemented yet"))
}

pub fn add(_g: &GlobalArgs, _a: &ChannelAddArgs) -> Result<Report> {
    Err(not_implemented("channel add"))
}

pub fn remove(_g: &GlobalArgs, _a: &ChannelRemoveArgs) -> Result<Report> {
    Err(not_implemented("channel remove"))
}
