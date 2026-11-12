use anyhow::{Context, Result, bail};
use std::env;

use crate::project::PackageJson;
use crate::runtime::{Runtime, is_runnable_file};

/// `radium run <script> [args...]`
pub fn run(script: &str, args: &[String]) -> Result<u8> {
    let root = env::current_dir().context("Failed to get the current directory")?;

    if !PackageJson::path(&root).exists() {
        bail!("No package.json found here. Run `radium init` first.");
    }

    let package = PackageJson::load(&root)?;
    let runtime = Runtime::detect()?;

    if is_runnable_file(script) {
        runtime.run_file(&root, &package, script, args)
    } else {
        runtime.run_script(&root, &package, script, args)
    }
}
