use anyhow::{Context, Result, bail};
use std::env;
use std::fs;

use crate::installer::Installer;
use crate::lockfile::Lockfile;
use crate::project::PackageJson;
use crate::registry::Registry;
use crate::ui;

/// `radium upgrade`
pub fn upgrade() -> Result<u8> {
    let root = env::current_dir().context("Failed to get the current directory")?;

    if !PackageJson::path(&root).exists() {
        bail!("No package.json found here. Run `radium init` first.");
    }

    let mut package = PackageJson::load(&root)?;
    let dependencies: Vec<String> = package.dependencies.keys().cloned().collect();

    if dependencies.is_empty() {
        ui::info("No dependencies to upgrade.");
        Lockfile::from_package(&package).save(&root)?;
        return Ok(0);
    }

    ui::info(format!("Upgrading {} package(s)...", dependencies.len()));

    let node_modules = root.join("node_modules");
    if node_modules.exists() {
        fs::remove_dir_all(&node_modules)
            .with_context(|| format!("Failed to remove {}", node_modules.display()))?;
    }

    let registry = Registry::new()?;
    let mut installer = Installer::new(&registry, &root);

    for name in &dependencies {
        ui::info(format!("Upgrading {name}..."));
        let version = installer.install(name, "latest")?;
        package.set_dependency(name, &version);
        ui::info(format!("Updated {name} to {version}"));
    }

    package.mark_as_radium_project();
    package.save(&root)?;
    Lockfile::from_versions(package.name.clone(), installer.resolved()).save(&root)?;

    ui::success("Successfully upgraded all packages!");
    Ok(0)
}
