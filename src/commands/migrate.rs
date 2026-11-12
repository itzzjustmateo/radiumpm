use anyhow::{Context, Result, bail};
use std::env;

use crate::cli::MigrationSource;
use crate::lockfile::Lockfile;
use crate::migrate;
use crate::project::{self, PackageJson};
use crate::ui;

/// `radium migrate <source>`
pub fn run(source: MigrationSource, force: bool) -> Result<u8> {
    let root = env::current_dir().context("Failed to get the current directory")?;
    let target = project::lockfile_path(&root);

    if target.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite it",
            target.display()
        );
    }

    let mut package = PackageJson::load(&root)?;
    let migration = migrate::read(&root, source)?;

    Lockfile::from_versions(package.name.clone(), &migration.versions).save(&root)?;

    package.mark_as_radium_project();
    package.save(&root)?;

    ui::success(format!(
        "Migrated {} package(s) from {} to radium.lock",
        migration.versions.len(),
        migration.source.display()
    ));

    Ok(0)
}
