use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Args, Clone)]
pub struct GlobalArgs {
    /// Print exactly one JSON object on stdout
    #[arg(long, global = true)]
    pub json: bool,
    /// No prompts; use defaults
    #[arg(long, global = true)]
    pub yes: bool,
    /// Repo root
    #[arg(short = 'C', global = true, default_value = ".")]
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

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Render base + pieces into a destination and write the lock
    New(NewArgs),
    /// Add a piece instance at the lock's template commit
    Add(AddArgs),
    /// Move to a newer template version
    Update(UpdateArgs),
    /// Exit 1 on owned drift, conflict markers, or an available update
    Check(CheckArgs),
    /// Show what update would change, without writing
    Diff(DiffArgs),
    /// Print class, owning piece and matching glob of a path
    Explain(ExplainArgs),
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::New(_) => "new",
            Command::Add(_) => "add",
            Command::Update(_) => "update",
            Command::Check(_) => "check",
            Command::Diff(_) => "diff",
            Command::Explain(_) => "explain",
        }
    }
}

pub const SUBCOMMANDS: &[&str] = &["new", "add", "update", "check", "diff", "explain"];

#[derive(Debug, Args)]
pub struct NewArgs {
    pub source: String,
    #[arg(default_value = ".")]
    pub dest: PathBuf,
    #[arg(long = "ref")]
    pub git_ref: Option<String>,
    #[arg(long)]
    pub pre: bool,
    #[arg(long = "piece", value_name = "ID[:NAME]")]
    pub pieces: Vec<String>,
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    #[arg(long)]
    pub no_post: bool,
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct AddArgs {
    pub piece_id: String,
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    #[arg(long)]
    pub no_post: bool,
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct UpdateArgs {
    #[arg(long, value_name = "VERSION")]
    pub to: Option<String>,
    #[arg(long = "data", value_name = "K=V")]
    pub data: Vec<String>,
    #[arg(long)]
    pub allow_dirty: bool,
    #[arg(long)]
    pub force: bool,
    #[arg(long)]
    pub no_post: bool,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    #[arg(long)]
    pub offline: bool,
}

#[derive(Debug, Args)]
pub struct DiffArgs {
    #[arg(long, value_name = "VERSION")]
    pub to: Option<String>,
}

#[derive(Debug, Args)]
pub struct ExplainArgs {
    pub path: String,
}
