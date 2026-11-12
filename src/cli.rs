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
    Init {
        /// Template to scaffold with, for example `next` or `vite`
        template: Option<String>,
    },

    /// Create a new project directory
    New {
        /// Directory to create (prompted for when omitted)
        dir_name: Option<String>,

        /// Project author
        #[arg(short, long)]
        author: Option<String>,
    },

    /// Create a project from a template
    Create {
        /// Directory to create
        dir_name: String,

        /// Template to use, for example `next` or `vite`
        #[arg(short, long)]
        template: String,
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

    /// Run a package executable without adding it to the project (npx-style)
    #[command(visible_alias = "exec")]
    X {
        /// Package spec, for example `typescript` or `next-app@latest`
        package: String,

        /// Arguments forwarded to the executable
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

/// Command-line interface for `radix`, the one-off executable runner.
#[derive(Debug, Parser)]
#[command(
    name = "radix",
    version,
    about = "Run an npm package executable without installing it into a project",
    long_about = None
)]
pub struct RadixCli {
    /// Package spec, for example `typescript` or `next-app@latest`
    pub package: String,

    /// Arguments forwarded to the executable
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}
