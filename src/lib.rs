//! splice: bootstrap a repo from versioned template pieces and keep it current.

pub mod apply;
pub mod classify;
pub mod cli;
pub mod error;
pub mod fetch;
pub mod filter;
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
pub mod sections;
pub mod source;
pub mod sources;
pub mod subset;
pub mod vars;

use cli::{Cli, Command};
use error::SpliceError;
use output::Report;

/// Run one command. Commands are filled in by later tasks; until then each is a usage error.
pub fn run(cli: Cli) -> Result<Report, SpliceError> {
    match &cli.command {
        Command::New(args) => ops::new::run(&cli.global, args),
        Command::Add(args) => ops::add::run(&cli.global, args),
        Command::Explain(args) => ops::explain::run(&cli.global, args),
        Command::Update(args) => ops::update::run(&cli.global, args),
        Command::Check(args) => ops::check::run(&cli.global, args),
        Command::Diff(args) => ops::diff::run(&cli.global, args),
    }
}
