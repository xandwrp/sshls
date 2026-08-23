use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sshls",
    version,
    about = "A small, durable inventory for SSH destinations",
    arg_required_else_help = false,
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Show one managed destination
    Show { name: String },
    /// Search names, destinations, users, and tags
    Find { text: String },
    /// Show what the installed SSH client will actually use
    Resolve { name: String },
    /// Attempt a bounded, non-interactive SSH connection
    Test {
        name: String,
        /// Overall timeout in seconds
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=300))]
        timeout: u64,
    },
    /// Add a managed destination
    Add(AddArgs),
    /// Change a managed destination
    Edit(EditArgs),
    /// Duplicate a managed destination
    Clone { source: String, name: String },
    /// Rename a managed destination
    Rename { old: String, new: String },
    /// Forget a managed destination
    Rm { name: String },
    /// Manage tags
    Tag {
        #[command(subcommand)]
        command: TagCommand,
    },
    /// Check or regenerate OpenSSH integration
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Args)]
pub struct AddArgs {
    pub name: String,
    pub hostname: String,
    #[arg(long)]
    pub user: Option<String>,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(short = 'i', long = "identity", value_name = "PATH")]
    pub identity: Option<String>,
    #[arg(short = 't', long = "tag", value_name = "TAG")]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct EditArgs {
    pub name: String,
    #[arg(long = "host")]
    pub hostname: Option<String>,
    #[arg(long, conflicts_with = "clear_user")]
    pub user: Option<String>,
    #[arg(long, conflicts_with = "user")]
    pub clear_user: bool,
    #[arg(long, conflicts_with = "clear_port")]
    pub port: Option<u16>,
    #[arg(long, conflicts_with = "port")]
    pub clear_port: bool,
    #[arg(short = 'i', long = "identity", conflicts_with = "clear_identity")]
    pub identity: Option<String>,
    #[arg(long, conflicts_with = "identity")]
    pub clear_identity: bool,
}

impl EditArgs {
    pub fn has_changes(&self) -> bool {
        self.hostname.is_some()
            || self.user.is_some()
            || self.clear_user
            || self.port.is_some()
            || self.clear_port
            || self.identity.is_some()
            || self.clear_identity
    }
}

#[derive(Debug, Subcommand)]
pub enum TagCommand {
    /// Add one or more tags
    Add {
        name: String,
        #[arg(required = true)]
        tags: Vec<String>,
    },
    /// Remove one or more tags
    Rm {
        name: String,
        #[arg(required = true)]
        tags: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Validate the complete setup without connecting
    Check,
    /// Regenerate the managed OpenSSH fragment
    Sync,
}
