pub mod cache;
pub mod cli;
pub mod commands;
pub mod hash;
pub mod installer;
pub mod lockfile;
pub mod migrate;
pub mod project;
pub mod registry;
pub mod runtime;
pub mod spec;
pub mod templates;
pub mod ui;

use clap::Parser;
use std::process::ExitCode;

/// Run the `radium` command-line interface.
pub fn run_cli() -> ExitCode {
    let cli = cli::Cli::parse();
    finish(commands::dispatch(cli))
}

/// Run the `radix` executable, a shortcut for `radium x`.
pub fn run_radix() -> ExitCode {
    let cli = cli::RadixCli::parse();
    finish(commands::exec::run(&cli.package, &cli.args))
}

fn finish(result: anyhow::Result<u8>) -> ExitCode {
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            ui::error(&error);
            ExitCode::FAILURE
        }
    }
}
