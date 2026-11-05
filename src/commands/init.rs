use anyhow::{Context, Result, bail};
use std::env;
use std::path::Path;

use crate::lockfile::Lockfile;
use crate::project::{self, PackageJson};
use crate::ui;

/// `radium init`
pub fn initialize() -> Result<u8> {
    let root = env::current_dir().context("Failed to get the current directory")?;
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("my-project")
        .to_string();

    let author = ui::prompt("Author name: ")?;
    scaffold(&root, &name, &author)?;

    Ok(0)
}

/// `radium new`
pub fn create_new(dir_name: Option<String>, author: Option<String>) -> Result<u8> {
    let name = match dir_name {
        Some(name) => name,
        None => ui::prompt("Project name: ")?,
    };

    if name.trim().is_empty() {
        bail!("Project name cannot be empty");
    }

    let author = match author {
        Some(author) => author,
        None => ui::prompt("Author name: ")?,
    };

    scaffold(Path::new(&name), &name, &author)?;

    Ok(0)
}

/// Create package.json, radium.lock, and a starter source file.
pub(crate) fn scaffold(root: &Path, name: &str, author: &str) -> Result<()> {
    if PackageJson::path(root).exists() {
        bail!(
            "A package.json already exists in {}; refusing to overwrite it",
            root.display()
        );
    }

    project::create(root, name, author)?;

    let package = PackageJson::load(root)?;
    Lockfile::from_package(&package).save(root)?;

    ui::success(format!("Initialized project `{name}`"));
    Ok(())
}
