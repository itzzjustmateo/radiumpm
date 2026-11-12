use anyhow::Result;

use crate::cache::Cache;
use crate::cli::CacheCommand;
use crate::installer::validate_package_name;
use crate::ui;

/// `radium cache clear [package]`
pub fn run(command: CacheCommand) -> Result<u8> {
    let cache = Cache::new()?;

    match command {
        CacheCommand::Clear { package } => clear(&cache, package.as_deref()),
    }
}

fn clear(cache: &Cache, package: Option<&str>) -> Result<u8> {
    match package {
        Some(name) => {
            validate_package_name(name)?;

            if cache.clear_package(name)? {
                ui::success(format!("Cleared cached files for {name}."));
            } else {
                ui::info(format!("No cached files found for {name}."));
            }
        }
        None => {
            cache.clear()?;
            ui::success(format!(
                "Cleared the Radium cache at {}.",
                cache.root().display()
            ));
        }
    }

    Ok(0)
}
