use clap::{Parser, Subcommand, ValueEnum};

/// Lockfile ecosystems supported by `radium migrate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MigrationSource {
    /// npm's package-lock.json
    #[value(alias = "npm")]
    Node,

    /// Deno's deno.lock
    Deno,

    /// Bun's bun.lock or bun.lockb
    Bun,
}

/// Command-line interface for Radium.
#[derive(Debug, Parser)]
#[command(
    name = "radium",
    version,
    about = "A small JavaScript package manager written in Rust",
    long_about = None
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize a Radium project in the current directory
    Init,

    /// Create a new project directory
    New {
        /// Directory to create (prompted for when omitted)
        dir_name: Option<String>,

        /// Project author
        #[arg(short, long)]
        author: Option<String>,
    },

    /// Add a dependency from the npm registry
    Add {
        /// Package name, for example `typescript` or `@scope/name`
        package_name: String,
    },

    /// Install every dependency at the exact versions in radium.lock
    Install,

    /// Reinstall every dependency at its latest version
    Upgrade,

    /// Convert another package manager's lockfile into radium.lock
    Migrate {
        /// Source lockfile ecosystem
        #[arg(value_enum)]
        source: MigrationSource,

        /// Overwrite an existing radium.lock
        #[arg(long)]
        force: bool,
    },

    /// Run a package.json script or a .ts/.js/.mjs file
    Run {
        /// Script name or path to a .ts/.js/.mjs file
        script: String,

        /// Arguments appended to the script or file command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show environment and project diagnostics
    Doctor,

    /// Manage the shared package cache
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

/// Subcommands for `radium cache`.
#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Delete the entire cache, or only one package's cached files
    Clear {
        /// Package name; omit to clear the entire cache
        package: Option<String>,
    },
}
