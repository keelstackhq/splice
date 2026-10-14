pub mod apply;
pub mod classify;
pub mod cli;
pub mod error;
pub mod fetch;
pub mod fsio;
pub mod git;
pub mod lock;
pub mod manifest;
pub mod markers;
pub mod merge;
pub mod migrate;
pub mod ops;
pub mod output;
pub mod paths;
pub mod planner;
pub mod post;
pub mod render;
pub mod source;
pub mod subset;
pub mod vars;

use cli::{Cli, Command};
use error::SpliceError;
use output::Report;

/// Run a parsed command line and return its report.
pub fn run(cli: Cli) -> Result<Report, SpliceError> {
    match cli.command {
        Command::New(a) => ops::new::run(&cli.global, &a),
        Command::Add(a) => ops::add::run(&cli.global, &a),
        Command::Explain(a) => ops::explain::run(&cli.global, &a),
        Command::Update(a) => ops::update::run(&cli.global, &a),
        Command::Check(_) | Command::Diff(_) => Err(SpliceError::Usage("not implemented".into())),
    }
}
