use anyhow::{Context, Result, bail};
use std::env;
use std::fs;

use crate::installer::Installer;
use crate::lockfile::Lockfile;
use crate::project::{self, PackageJson};
use crate::registry::Registry;
use crate::ui;

/// `radium install`
///
/// Restore `node_modules` from the exact versions recorded in `radium.lock`.
/// The lockfile is never modified; run `radium add` or `radium upgrade` to
/// update it.
pub fn install() -> Result<u8> {
    let root = env::current_dir().context("Failed to get the current directory")?;

    if !PackageJson::path(&root).exists() {
        bail!("No package.json found here. Run `radium init` first.");
    }

    if !project::lockfile_path(&root).exists() {
        bail!(
            "No radium.lock found here. Run `radium add <package>` or `radium migrate <source>` to create one."
        );
    }

    let mut package = PackageJson::load(&root)?;
    let lockfile = Lockfile::load(&root)?;

    let missing: Vec<&String> = package
        .dependencies
        .keys()
        .filter(|name| !lockfile.packages.contains_key(*name))
        .collect();

    if !missing.is_empty() {
        bail!(
            "radium.lock is missing entries for: {}. Run `radium add` or `radium upgrade` to update the lockfile.",
            missing
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let dependencies: Vec<String> = package.dependencies.keys().cloned().collect();

    if dependencies.is_empty() {
        ui::info("No dependencies to install.");
    } else {
        ui::info(format!(
            "Installing {} package(s) from radium.lock...",
            dependencies.len()
        ));
    }

    let node_modules = root.join("node_modules");
    if node_modules.exists() {
        fs::remove_dir_all(&node_modules)
            .with_context(|| format!("Failed to remove {}", node_modules.display()))?;
    }

    let registry = Registry::new()?;
    let mut installer = Installer::new(&registry, &root);

    for name in &dependencies {
        installer.install_locked(name, &lockfile)?;
    }

    package.mark_as_radium_project();
    package.save(&root)?;

    if dependencies.is_empty() {
        ui::success("Successfully installed the locked dependency tree!");
    } else {
        ui::success(format!(
            "Successfully installed {} package(s) from radium.lock!",
            installer.resolved().len()
        ));
    }

    Ok(0)
}
