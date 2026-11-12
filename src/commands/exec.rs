use anyhow::{Context, Result, bail};
use std::env;
use std::path::{Path, PathBuf};

use crate::installer::Installer;
use crate::project::PackageJson;
use crate::registry::Registry;
use crate::runtime::Runtime;
use crate::spec::PackageSpec;
use crate::templates::Template;
use crate::ui;

/// `radium x <package> [args...]`
pub fn run(specification: &str, args: &[String]) -> Result<u8> {
    let spec = PackageSpec::parse(specification)?;
    execute(&spec, args)
}

/// Run a template initializer against `target`.
pub fn run_template(template: &Template, target: &str) -> Result<u8> {
    let mut args = template.args.clone();
    args.push(target.to_string());

    execute(&template.spec, &args)
}

fn execute(spec: &PackageSpec, args: &[String]) -> Result<u8> {
    let working_dir = env::current_dir().context("Failed to get the current directory")?;

    // Install into a scratch tree so one-off tools never modify the project's
    // node_modules or lockfile.
    let installation = tempfile::Builder::new()
        .prefix("radium-exec-")
        .tempdir()
        .context("Failed to create a temporary installation directory")?;
    let root = installation.path();

    let registry = Registry::new()?;
    let mut installer = Installer::new(&registry, root);
    let version = installer.install_cached(&spec.name, &spec.version)?;

    let package_dir = root.join("node_modules").join(&spec.name);
    let package = PackageJson::load(&package_dir)?;
    let (command, relative) = select_bin(&package, &spec.command)?;
    let bin = resolve_bin(&package_dir, &relative)?;

    ui::info(format!("Running {command} from {}@{version}...", spec.name));

    Runtime::detect()?.run_package_bin(&working_dir, root, &package, &bin, args)
}

/// Pick the executable to run from a package's `bin` entries.
///
/// The package's own name wins when present, which covers the common case.
/// A package with a single executable is unambiguous even when it is named
/// differently.
fn select_bin(package: &PackageJson, preferred: &str) -> Result<(String, String)> {
    let bins = package.bins();
    let package_name = package.name.as_deref().unwrap_or(preferred);

    if bins.is_empty() {
        bail!("`{package_name}` does not declare an executable (`bin` entry)");
    }

    if let Some(path) = bins.get(preferred) {
        return Ok((preferred.to_string(), path.clone()));
    }

    if bins.len() == 1 {
        let (name, path) = bins.into_iter().next().expect("one bin entry");
        return Ok((name, path));
    }

    let available = bins.keys().cloned().collect::<Vec<_>>().join(", ");
    bail!("`{package_name}` exposes multiple executables: {available}");
}

/// Resolve a `bin` path and make sure it stays inside the package directory.
fn resolve_bin(package_dir: &Path, relative: &str) -> Result<PathBuf> {
    let package_root = package_dir
        .canonicalize()
        .with_context(|| format!("Failed to resolve {}", package_dir.display()))?;

    let bin = package_dir.join(relative).canonicalize().with_context(|| {
        format!(
            "Executable `{relative}` is missing from {}",
            package_dir.display()
        )
    })?;

    if !bin.starts_with(&package_root) {
        bail!(
            "Executable `{relative}` points outside of {}",
            package_dir.display()
        );
    }

    if !bin.is_file() {
        bail!("Executable `{relative}` is not a file");
    }

    Ok(bin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, bin: &str) -> PackageJson {
        serde_json::from_str(&format!(
            r#"{{"name":"{name}","version":"1.0.0","bin":{bin}}}"#
        ))
        .expect("parse package.json")
    }

    #[test]
    fn selects_the_preferred_bin() {
        let package = package("tool", r##"{"tool":"cli.js","helper":"helper.js"}"##);

        let (name, path) = select_bin(&package, "tool").expect("select bin");
        assert_eq!(name, "tool");
        assert_eq!(path, "cli.js");
    }

    #[test]
    fn selects_the_only_bin() {
        let package = package("tool", r##"{"other":"cli.js"}"##);

        let (name, path) = select_bin(&package, "tool").expect("select bin");
        assert_eq!(name, "other");
        assert_eq!(path, "cli.js");
    }

    #[test]
    fn selects_the_package_name_for_a_string_bin() {
        let package = package("tool", r##""cli.js""##);

        let (name, path) = select_bin(&package, "tool").expect("select bin");
        assert_eq!(name, "tool");
        assert_eq!(path, "cli.js");
    }

    #[test]
    fn rejects_ambiguous_and_missing_bins() {
        let ambiguous = package("tool", r##"{"first":"a.js","second":"b.js"}"##);
        assert!(select_bin(&ambiguous, "tool").is_err());

        let missing = package("tool", r##"{}"##);
        assert!(select_bin(&missing, "tool").is_err());
    }

    #[test]
    fn rejects_bins_outside_the_package() {
        let directory = tempfile::tempdir().expect("temp dir");
        let package_dir = directory.path().join("package");
        std::fs::create_dir_all(&package_dir).expect("create package dir");
        std::fs::write(package_dir.join("cli.js"), "").expect("write cli");
        std::fs::write(directory.path().join("outside.js"), "").expect("write outside");

        assert!(resolve_bin(&package_dir, "cli.js").is_ok());
        assert!(resolve_bin(&package_dir, "../outside.js").is_err());
    }
}
