pub mod cli;
pub mod error;
pub mod fetch;
pub mod git;
pub mod manifest;
pub mod output;
pub mod source;

use cli::{Cli, Command};
use error::SpliceError;
use output::Report;

/// Run a parsed command line and return its report.
pub fn run(cli: Cli) -> Result<Report, SpliceError> {
    match cli.command {
        Command::New(_) => Err(SpliceError::Usage("not implemented".into())),
        Command::Add(_) => Err(SpliceError::Usage("not implemented".into())),
        Command::Explain(_) => Err(SpliceError::Usage("not implemented".into())),
        Command::Update(_) | Command::Check(_) | Command::Diff(_) => {
            Err(SpliceError::Usage("not implemented".into()))
        }
    }
}
