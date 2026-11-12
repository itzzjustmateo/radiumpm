mod cli;
mod commands;
mod installer;
mod lockfile;
mod migrate;
mod project;
mod registry;
mod runtime;
mod ui;

use clap::Parser;
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = cli::Cli::parse();

    match commands::dispatch(cli) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            ui::error(&error);
            ExitCode::FAILURE
        }
    }
}
