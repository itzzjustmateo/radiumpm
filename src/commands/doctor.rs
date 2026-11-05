use anyhow::{Context, Result};
use std::env;

use crate::lockfile::Lockfile;
use crate::project::{self, PackageJson};
use crate::runtime::Runtime;
use crate::ui;

/// `radium doctor`
pub fn run() -> Result<u8> {
    ui::info(format!("radium {}", env!("CARGO_PKG_VERSION")));

    let mut healthy = true;

    match Runtime::detect().and_then(|runtime| {
        let version = runtime.version()?;
        Ok((runtime, version))
    }) {
        Ok((runtime, version)) => {
            ui::info(format!("JavaScript runtime: {} {version}", runtime.name()));
        }
        Err(error) => {
            healthy = false;
            ui::warn(format!("JavaScript runtime unavailable: {error:#}"));
        }
    }

    let root = env::current_dir().context("Failed to get the current directory")?;

    if PackageJson::path(&root).exists() {
        match PackageJson::load(&root) {
            Ok(package) => ui::info(format!(
                "Project: {}",
                package.name.as_deref().unwrap_or("<unnamed>")
            )),
            Err(error) => {
                ui::warn(format!("package.json could not be parsed: {error:#}"));
                healthy = false;
            }
        }
    } else {
        ui::warn("No package.json found in the current directory");
    }

    if project::lockfile_path(&root).exists() {
        match Lockfile::load(&root) {
            Ok(lockfile) => ui::info(format!(
                "Lockfile: present ({} package(s))",
                lockfile.packages.len()
            )),
            Err(error) => {
                healthy = false;
                ui::warn(format!("radium.lock could not be parsed: {error:#}"));
            }
        }
    } else {
        ui::info("Lockfile: missing");
    }

    Ok(u8::from(!healthy))
}
