use anyhow::{Context, Result};
use std::fmt::Display;
use std::io::{self, Write};

/// Print a normal progress message.
pub fn info(message: impl Display) {
    println!("{message}");
}

/// Print a success message.
pub fn success(message: impl Display) {
    println!("{message}");
}

/// Print a warning to stderr.
pub fn warn(message: impl Display) {
    eprintln!("warning: {message}");
}

/// Print an error, including its anyhow context chain, to stderr.
pub fn error(error: &anyhow::Error) {
    eprintln!("error: {error:#}");
}

/// Prompt for a single line of input from stdin.
pub fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush().context("Failed to flush stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("Failed to read user input")?;

    Ok(input.trim().to_string())
}
