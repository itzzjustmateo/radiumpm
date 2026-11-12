use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::env;
use std::path::Path;

use crate::installer::{Installer, validate_package_name};
use crate::lockfile::Lockfile;
use crate::project::{self, PackageJson};
use crate::registry::Registry;
use crate::ui;

use super::init;

/// `radium add <package>`
pub fn add(package_name: &str) -> Result<u8> {
    validate_package_name(package_name)?;

    let root = env::current_dir().context("Failed to get the current directory")?;

    if !ensure_project(&root)? {
        ui::info("Installation aborted. Run `radium init` first.");
        return Ok(0);
    }

    ui::info(format!("Installing {package_name}..."));

    let registry = Registry::new()?;
    let mut installer = Installer::new(&registry, &root);
    let version = installer.install(package_name, "latest")?;

    let mut package = PackageJson::load(&root)?;
    package.set_dependency(package_name, &version);
    package.mark_as_radium_project();
    package.save(&root)?;

    // Preserve packages that were locked before this command, then overlay
    // everything installed during this run (including transitive packages).
    let mut versions = Lockfile::load(&root)
        .map(|lockfile| {
            lockfile
                .packages
                .into_iter()
                .map(|(name, package)| (name, package.version))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    versions.extend(
        installer
            .resolved()
            .iter()
            .map(|(name, version)| (name.clone(), version.clone())),
    );

    Lockfile::from_versions(package.name.clone(), &versions).save(&root)?;

    ui::success(format!("Successfully installed {package_name}@{version}!"));

    Ok(0)
}

/// Make sure `root` has a package.json and lockfile, prompting the user when
/// it does not. Returns `false` when the user declines.
fn ensure_project(root: &Path) -> Result<bool> {
    if project::is_project(root) {
        return Ok(true);
    }

    if PackageJson::path(root).exists() {
        let answer =
            ui::prompt("package.json found but radium.lock is missing. Create it? (y/N): ")?;
        if !is_yes(&answer) {
            return Ok(false);
        }

        let package = PackageJson::load(root)?;
        Lockfile::from_package(&package).save(root)?;
        return Ok(true);
    }

    let answer = ui::prompt("No Radium project found here. Initialize one now? (y/N): ")?;
    if !is_yes(&answer) {
        return Ok(false);
    }

    let name = ui::prompt("Project name: ")?;
    let author = ui::prompt("Author name: ")?;
    init::scaffold(root, &name, &author)?;

    Ok(true)
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}
