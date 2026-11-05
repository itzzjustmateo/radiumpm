mod add;
mod doctor;
mod init;
mod run;
mod upgrade;

use anyhow::Result;

use crate::cli::{Cli, Command};

/// Execute a parsed CLI command, returning the process exit code.
pub fn dispatch(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Init => init::initialize(),
        Command::New { dir_name, author } => init::create_new(dir_name, author),
        Command::Add { package_name } => add::add(&package_name),
        Command::Upgrade => upgrade::upgrade(),
        Command::Run { script, args } => run::run(&script, &args),
        Command::Doctor => doctor::run(),
    }
}
