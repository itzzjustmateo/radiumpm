mod add;
mod cache;
mod doctor;
pub(crate) mod exec;
mod init;
mod install;
mod migrate;
mod run;
mod upgrade;

use anyhow::Result;

use crate::cli::{Cli, Command};

/// Execute a parsed CLI command, returning the process exit code.
pub fn dispatch(cli: Cli) -> Result<u8> {
    match cli.command {
        Command::Init { template } => match template {
            Some(template) => init::initialize_from_template(&template),
            None => init::initialize(),
        },
        Command::New { dir_name, author } => init::create_new(dir_name, author),
        Command::Create { dir_name, template } => init::create_from_template(&dir_name, &template),
        Command::Add { package_name } => add::add(&package_name),
        Command::Install => install::install(),
        Command::Upgrade => upgrade::upgrade(),
        Command::Migrate { source, force } => migrate::run(source, force),
        Command::Run { script, args } => run::run(&script, &args),
        Command::X { package, args } => exec::run(&package, &args),
        Command::Doctor => doctor::run(),
        Command::Cache { command } => cache::run(command),
    }
}
