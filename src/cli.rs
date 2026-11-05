use clap::{Parser, Subcommand};

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

    /// Reinstall every dependency at its latest version
    Upgrade,

    /// Run a script from package.json using the configured JavaScript runtime
    Run {
        /// Script name from the `scripts` section of package.json
        script: String,

        /// Arguments appended to the script command
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Show environment and project diagnostics
    Doctor,
}
