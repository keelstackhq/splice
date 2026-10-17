//! Command line grammar (spec §3).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Clone, Args)]
pub struct GlobalArgs {
    /// Print exactly one JSON object on stdout.
    #[arg(long, global = true)]
    pub json: bool,
    /// No prompts; use defaults.
    #[arg(long, global = true)]
    pub yes: bool,
    /// Repo root.
    #[arg(short = 'C', global = true, value_name = "DIR", default_value = ".")]
    pub dir: PathBuf,
}

#[derive(Debug, Parser)]
#[command(name = "splice", version, about)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Render `base` plus the listed pieces into a directory and write the lock.
    New(NewArgs),
    /// Add a piece instance at the lock's template commit.
    Add(AddArgs),
    /// Move to a newer template version.
    Update(UpdateArgs),
    /// Exit 1 on owned drift, leftover conflict markers, or an available update.
    Check(CheckArgs),
    /// Show what `update` would change, without writing.
    Diff(DiffArgs),
    /// Print class, owning piece instance and matching glob of a path.
    Explain(ExplainArgs),
    /// Add or remove a channel: an extra template source kept current beside the main one.
    Channel(ChannelArgs),
}

impl Command {
    /// Subcommand names, as typed on the command line.
    pub const NAMES: [&'static str; 7] = [
        "new", "add", "update", "check", "diff", "explain", "channel",
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Command::New(_) => "new",
            Command::Add(_) => "add",
            Command::Update(_) => "update",
            Command::Check(_) => "check",
            Command::Diff(_) => "diff",
            Command::Explain(_) => "explain",
            Command::Channel(a) => match a.command {
                ChannelCommand::Add(_) => "channel add",
                ChannelCommand::Remove(_) => "channel remove",
            },
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct NewArgs {
    /// Template source (`gh:owner/repo//subdir`, a git URL or a local path).
    pub source: String,
    /// Destination directory.
    #[arg(default_value = ".")]
    pub dest: PathBuf,
    /// A tag, branch or sha.
    #[arg(long = "ref", value_name = "R")]
    pub git_ref: Option<String>,
    /// Allow pre-release versions.
    #[arg(long)]
    pub pre: bool,
    /// Piece to render, `ID` or `ID:NAME`.
    #[arg(long = "piece", value_name = "ID[:NAME]")]
    pub pieces: Vec<String>,
    /// Template or piece variable, `K=V`.
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    /// Skip post commands.
    #[arg(long)]
    pub no_post: bool,
    /// Write into a non-empty destination.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Piece id.
    pub piece_id: String,
    /// Instance name for a `multiple` piece.
    #[arg(long)]
    pub name: Option<String>,
    /// Add the piece from this channel instead of the main template.
    #[arg(long, value_name = "NAME")]
    pub channel: Option<String>,
    /// Template or piece variable, `K=V`.
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    /// Add even when the working tree is dirty.
    #[arg(long)]
    pub allow_dirty: bool,
    /// Skip post commands.
    #[arg(long)]
    pub no_post: bool,
    /// Replace owned files that drifted; keep conflicting shared files as they are.
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    /// Target version (default: latest stable tag).
    #[arg(long, value_name = "VERSION")]
    pub to: Option<String>,
    /// Template or piece variable, `K=V`.
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    /// Update even when the working tree is dirty.
    #[arg(long)]
    pub allow_dirty: bool,
    /// Overwrite owned files that drifted.
    #[arg(long)]
    pub force: bool,
    /// Skip post commands.
    #[arg(long)]
    pub no_post: bool,
    /// Update only this channel.
    #[arg(long, value_name = "NAME")]
    pub channel: Option<String>,
    /// Update the main template and every channel.
    #[arg(
        long,
        conflicts_with_all = ["channel", "to", "data", "only", "exclude"]
    )]
    pub all: bool,
    /// Only paths matching this glob (channel updates).
    #[arg(long, value_name = "GLOB", requires = "channel")]
    pub only: Vec<String>,
    /// Skip paths matching this glob (channel updates).
    #[arg(long, value_name = "GLOB", requires = "channel")]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Args)]
pub struct CheckArgs {
    /// Do not look for newer template versions.
    #[arg(long)]
    pub offline: bool,
    /// Check only this channel.
    #[arg(long, value_name = "NAME")]
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct DiffArgs {
    /// Target version (default: latest stable tag).
    #[arg(long, value_name = "VERSION")]
    pub to: Option<String>,
    /// Show the update of this channel.
    #[arg(long, value_name = "NAME")]
    pub channel: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ExplainArgs {
    /// Path to explain, relative to the repo root.
    pub path: String,
}

#[derive(Debug, Clone, Args)]
pub struct ChannelArgs {
    #[command(subcommand)]
    pub command: ChannelCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ChannelCommand {
    /// Render a template source into the repo as a named channel.
    Add(ChannelAddArgs),
    /// Stop tracking a channel and delete its lock.
    Remove(ChannelRemoveArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ChannelAddArgs {
    /// Template source (`gh:owner/repo//subdir`, a git URL or a local path).
    pub source: String,
    /// Channel name (default: derived from the source).
    #[arg(long = "as", value_name = "NAME")]
    pub as_name: Option<String>,
    /// A tag, branch or sha.
    #[arg(long = "ref", value_name = "R", conflicts_with = "track")]
    pub git_ref: Option<String>,
    /// Follow this branch instead of version tags.
    #[arg(long, value_name = "BRANCH")]
    pub track: Option<String>,
    /// Allow pre-release versions.
    #[arg(long)]
    pub pre: bool,
    /// Piece to render, `ID` or `ID:NAME`.
    #[arg(long = "piece", value_name = "ID[:NAME]")]
    pub pieces: Vec<String>,
    /// Template or piece variable, `K=V`.
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    /// Only paths matching this glob.
    #[arg(long, value_name = "GLOB")]
    pub only: Vec<String>,
    /// Skip paths matching this glob.
    #[arg(long, value_name = "GLOB")]
    pub exclude: Vec<String>,
    /// Add even when the working tree is dirty.
    #[arg(long)]
    pub allow_dirty: bool,
    /// Replace owned files that drifted.
    #[arg(long)]
    pub force: bool,
    /// Skip post commands.
    #[arg(long)]
    pub no_post: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ChannelRemoveArgs {
    /// Channel name.
    pub name: String,
    /// Remove even when the working tree is dirty.
    #[arg(long)]
    pub allow_dirty: bool,
    /// Delete owned files that drifted.
    #[arg(long)]
    pub force: bool,
}
