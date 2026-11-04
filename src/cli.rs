use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "claude-statusline-rs",
    about = "Custom statusline for Claude Code"
)]
pub struct Cli {
    /// Print version
    #[arg(short, long)]
    pub version: bool,

    /// Path to config file
    #[arg(short, long)]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List dev servers of this worktree (and others), or kill them
    Servers {
        /// Kill this worktree's servers (whole process tree)
        #[arg(long)]
        kill: bool,
        /// With --kill: also kill other repos' dev servers
        #[arg(long, requires = "kill")]
        all: bool,
        /// Worktree to inspect (default: current directory)
        #[arg(long)]
        cwd: Option<String>,
        /// Read the worktree from a Claude Code hook's stdin JSON (`cwd`)
        #[arg(long, conflicts_with = "cwd")]
        hook: bool,
    },
}

impl Cli {
    pub fn parse_args() -> Self {
        Cli::parse()
    }
}
