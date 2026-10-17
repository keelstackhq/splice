//! splice: bootstrap a repo from versioned template pieces and keep it current.

pub mod cli;
pub mod error;
pub mod fetch;
pub mod git;
pub mod manifest;
pub mod output;
pub mod render;
pub mod source;
pub mod subset;

use cli::{Cli, Command};
use error::SpliceError;
use output::Report;

/// Run one command. Commands are filled in by later tasks; until then each is a usage error.
pub fn run(cli: Cli) -> Result<Report, SpliceError> {
    match cli.command {
        Command::New(_)
        | Command::Add(_)
        | Command::Update(_)
        | Command::Check(_)
        | Command::Diff(_)
        | Command::Explain(_) => Err(SpliceError::Usage("not implemented".into())),
    }
}
