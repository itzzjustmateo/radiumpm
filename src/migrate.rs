use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::MigrationSource;
use crate::installer::validate_package_name;

/// A lockfile converted from another package manager.
///
/// Radium uses a flat dependency tree, so nested versions are collapsed into a
/// single package -> version map. For npm lockfiles the shallowest version of a
/// package wins; for Deno and Bun the highest resolved version wins.
#[derive(Debug, Clone)]
pub struct Migration {
    /// The lockfile that was read.
    pub source: PathBuf,

    /// Package name -> resolved version.
    pub versions: BTreeMap<String, String>,
}

impl Migration {
    fn new(source: PathBuf) -> Self {
        Self {
            source,
            versions: BTreeMap::new(),
        }
    }
}

/// Read the lockfile for `source` from `root` and convert it into a flat
/// package -> version map.
pub fn read(root: &Path, source: MigrationSource) -> Result<Migration> {
    match source {
        MigrationSource::Node => read_node(root),
        MigrationSource::Deno => read_deno(root),
        MigrationSource::Bun => read_bun(root),
    }
}

fn read_node(root: &Path) -> Result<Migration> {
    let path = root.join("package-lock.json");
    let content =
        fs::read_to_string(&path).with_context(|| format!("Could not read {}", path.display()))?;
    let document: Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    let mut migration = Migration::new(path.clone());
    let mut depths = BTreeMap::new();

    // npm v2/v3 lockfiles have a flat `packages` map keyed by install path.
    if let Some(packages) = document.get("packages").and_then(Value::as_object) {
        for (install_path, entry) in packages {
            if install_path.is_empty() {
                continue;
            }

            let Some(name) = npm_package_name(install_path) else {
                continue;
            };
            let Some(entry) = entry.as_object() else {
                continue;
            };
            if entry.get("link").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if entry
                .get("resolved")
                .and_then(Value::as_str)
                .is_some_and(is_non_registry_resolved)
            {
                continue;
            }
            let Some(version) = entry.get("version").and_then(Value::as_str) else {
                continue;
            };

            let depth = install_path.matches("node_modules/").count();
            insert(&mut migration.versions, &mut depths, name, version, depth);
        }
    }

    // npm v1 lockfiles (and the legacy section of v2 lockfiles) use a nested
    // `dependencies` tree. Only fall back to it when `packages` produced
    // nothing, because the flat map is authoritative when it exists.
    if migration.versions.is_empty()
        && let Some(dependencies) = document.get("dependencies").and_then(Value::as_object)
    {
        collect_npm_dependencies(dependencies, 0, &mut migration.versions, &mut depths);
    }

    if migration.versions.is_empty() {
        bail!("No npm packages found in {}", path.display());
    }

    Ok(migration)
}

/// Extract the package name from an npm install path such as
/// `node_modules/foo` or `node_modules/a/node_modules/@scope/b`.
fn npm_package_name(install_path: &str) -> Option<&str> {
    let index = install_path.rfind("node_modules/")?;
    let name = &install_path[index + "node_modules/".len()..];

    if name.is_empty() || name.ends_with('/') {
        None
    } else {
        Some(name)
    }
}

/// Whether a lockfile `resolved` URL points at something other than a package
/// registry, such as a local folder, workspace, or git repository.
fn is_non_registry_resolved(resolved: &str) -> bool {
    ["file:", "link:", "workspace:", "git+", "git:", "github:"]
        .iter()
        .any(|prefix| resolved.starts_with(prefix))
}

fn collect_npm_dependencies(
    dependencies: &Map<String, Value>,
    depth: usize,
    versions: &mut BTreeMap<String, String>,
    depths: &mut BTreeMap<String, usize>,
) {
    for (name, entry) in dependencies {
        let Some(entry) = entry.as_object() else {
            continue;
        };

        if entry
            .get("resolved")
            .and_then(Value::as_str)
            .is_some_and(is_non_registry_resolved)
        {
            continue;
        }

        if let Some(version) = entry.get("version").and_then(Value::as_str) {
            insert(versions, depths, name, version, depth);
        }

        if let Some(nested) = entry.get("dependencies").and_then(Value::as_object) {
            collect_npm_dependencies(nested, depth + 1, versions, depths);
        }
    }
}

fn read_deno(root: &Path) -> Result<Migration> {
    let path = root.join("deno.lock");
    let content =
        fs::read_to_string(&path).with_context(|| format!("Could not read {}", path.display()))?;
    let document: Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))?;

    let mut migration = Migration::new(path.clone());
    let mut depths = BTreeMap::new();

    if let Some(npm) = document.get("npm").and_then(Value::as_object) {
        // deno.lock v2/v3 nests `specifiers` and `packages` under `npm`.
        if let Some(packages) = npm.get("packages").and_then(Value::as_object) {
            collect_deno_packages(packages, &mut migration.versions, &mut depths);
        }
        if let Some(specifiers) = npm.get("specifiers").and_then(Value::as_object) {
            collect_deno_specifiers(specifiers, &mut migration.versions, &mut depths);
        }

        // deno.lock v4/v5 puts resolved `name@version` keys directly under `npm`.
        for (key, value) in npm {
            if key == "packages" || key == "specifiers" {
                continue;
            }

            if let Some((name, version)) = split_name_version(key) {
                insert(&mut migration.versions, &mut depths, name, version, 0);
            } else if let Some(version) = value.get("version").and_then(Value::as_str) {
                insert(&mut migration.versions, &mut depths, key, version, 0);
            }
        }
    }

    // deno.lock v4/v5 also records root specifiers at the top level.
    if let Some(specifiers) = document.get("specifiers").and_then(Value::as_object) {
        collect_deno_specifiers(specifiers, &mut migration.versions, &mut depths);
    }

    if migration.versions.is_empty() {
        bail!("No npm packages found in {}", path.display());
    }

    Ok(migration)
}

fn collect_deno_packages(
    packages: &Map<String, Value>,
    versions: &mut BTreeMap<String, String>,
    depths: &mut BTreeMap<String, usize>,
) {
    for (key, value) in packages {
        if let Some((name, version)) = split_name_version(key) {
            insert(versions, depths, name, version, 0);
        } else if let Some(version) = value.get("version").and_then(Value::as_str) {
            insert(versions, depths, key, version, 0);
        }
    }
}

fn collect_deno_specifiers(
    specifiers: &Map<String, Value>,
    versions: &mut BTreeMap<String, String>,
    depths: &mut BTreeMap<String, usize>,
) {
    for (key, value) in specifiers {
        let Some(value) = value.as_str() else {
            continue;
        };
        let Some(name) = deno_specifier_name(key) else {
            continue;
        };
        let Some(version) = deno_resolved_version(value) else {
            continue;
        };

        insert(versions, depths, name, version, 0);
    }
}

/// Extract the package name from a Deno specifier such as
/// `npm:chalk@^5.6.2` or `chalk@^5.3.0`.
fn deno_specifier_name(specifier: &str) -> Option<&str> {
    let specifier = specifier.strip_prefix("npm:").unwrap_or(specifier);
    split_name_version(specifier).map(|(name, _)| name)
}

/// Extract the resolved version from a Deno specifier value.
///
/// Deno v5 appends peer dependency information to resolved versions, for
/// example `1.1.11_marked@15.0.12`. Older lockfiles store a full
/// `name@version` string instead.
fn deno_resolved_version(value: &str) -> Option<&str> {
    let base = value.split('_').next().unwrap_or(value);
    if semver::Version::parse(base).is_ok() {
        return Some(base);
    }

    split_name_version(value).map(|(_, version)| version)
}

fn read_bun(root: &Path) -> Result<Migration> {
    let text_path = root.join("bun.lock");
    let binary_path = root.join("bun.lockb");

    if text_path.is_file() {
        parse_bun_lock(&text_path)
    } else if binary_path.is_file() {
        let mut migration = convert_bun_lockb(root, &text_path, &binary_path)?;
        migration.source = binary_path;
        Ok(migration)
    } else {
        bail!("Could not find bun.lock or bun.lockb in {}", root.display())
    }
}

fn parse_bun_lock(path: &Path) -> Result<Migration> {
    let content =
        fs::read_to_string(path).with_context(|| format!("Could not read {}", path.display()))?;
    let document: Value =
        json5::from_str(&content).with_context(|| format!("Failed to parse {}", path.display()))?;

    let Some(packages) = document.get("packages").and_then(Value::as_object) else {
        bail!("{} does not contain a `packages` map", path.display());
    };

    let mut migration = Migration::new(path.to_path_buf());
    let mut depths = BTreeMap::new();

    for (key, entry) in packages {
        let specifier = entry
            .as_array()
            .and_then(|values| values.first())
            .and_then(Value::as_str)
            .unwrap_or(key);

        let Some((name, version)) = split_name_version(specifier) else {
            continue;
        };

        insert(&mut migration.versions, &mut depths, name, version, 0);
    }

    if migration.versions.is_empty() {
        bail!("No registry packages found in {}", path.display());
    }

    Ok(migration)
}

/// Convert a binary `bun.lockb` into a temporary text lockfile, parse it, and
/// remove the generated file again.
fn convert_bun_lockb(root: &Path, text_path: &Path, binary_path: &Path) -> Result<Migration> {
    let bun = env::var_os("RADIUM_BUN").unwrap_or_else(|| OsString::from("bun"));
    convert_bun_lockb_with(root, text_path, binary_path, &bun)
}

fn convert_bun_lockb_with(
    root: &Path,
    text_path: &Path,
    binary_path: &Path,
    bun: &OsStr,
) -> Result<Migration> {
    let original = fs::read(binary_path)
        .with_context(|| format!("Could not read {}", binary_path.display()))?;

    let output = Command::new(bun)
        .args([
            "install",
            "--save-text-lockfile",
            "--frozen-lockfile",
            "--lockfile-only",
            "--ignore-scripts",
        ])
        .current_dir(root)
        .output();

    // Recent Bun versions delete the binary lockfile after writing the text
    // lockfile. Migration must not destroy the user's original lockfile, so
    // restore it before doing anything else.
    let unchanged = fs::read(binary_path)
        .map(|current| current == original)
        .unwrap_or(false);
    if !unchanged {
        fs::write(binary_path, &original)
            .with_context(|| format!("Failed to restore {}", binary_path.display()))?;
    }

    let output = output.with_context(|| {
        format!(
            "Bun is required to convert {}; install Bun or run \
             `bun install --save-text-lockfile --frozen-lockfile --lockfile-only` first",
            binary_path.display()
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Bun could not convert {}: {}",
            binary_path.display(),
            stderr.trim()
        );
    }

    let result = parse_bun_lock(text_path);
    let _ = fs::remove_file(text_path);
    result
}

/// Split a `name@version` specifier, handling scoped package names and Deno's
/// `version_peer@version` suffixes.
fn split_name_version(specifier: &str) -> Option<(&str, &str)> {
    let start = usize::from(specifier.starts_with('@'));
    let index = specifier[start..].find('@')? + start;

    let name = &specifier[..index];
    let version = specifier[index + 1..].split('_').next().unwrap_or_default();

    if name.is_empty() || version.is_empty() {
        None
    } else {
        Some((name, version))
    }
}

/// Record a resolved version, preferring shallower entries and then newer
/// versions. Non-registry packages and invalid names are ignored.
fn insert(
    versions: &mut BTreeMap<String, String>,
    depths: &mut BTreeMap<String, usize>,
    name: &str,
    version: &str,
    depth: usize,
) {
    if validate_package_name(name).is_err() {
        return;
    }

    let Ok(parsed_version) = semver::Version::parse(version) else {
        return;
    };

    if let Some(existing) = versions.get(name) {
        let existing_depth = depths.get(name).copied().unwrap_or(usize::MAX);
        let newer = semver::Version::parse(existing)
            .map(|existing_version| parsed_version > existing_version)
            .unwrap_or(true);

        if depth > existing_depth || (depth == existing_depth && !newer) {
            return;
        }
    }

    versions.insert(name.to_string(), version.to_string());
    depths.insert(name.to_string(), depth);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_lockfile(name: &str, content: &str) -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temp dir");
        fs::write(directory.path().join(name), content).expect("write lockfile");
        directory
    }

    #[test]
    fn migrates_npm_v1_with_shallow_versions() {
        let directory = write_lockfile(
            "package-lock.json",
            r#"{
                "name": "demo",
                "lockfileVersion": 1,
                "dependencies": {
                    "a": {
                        "version": "1.0.0",
                        "dependencies": {
                            "b": { "version": "1.0.0" }
                        }
                    },
                    "b": { "version": "2.0.0" }
                }
            }"#,
        );

        let migration = read_node(directory.path()).expect("read npm v1 lockfile");

        assert_eq!(migration.source, directory.path().join("package-lock.json"));
        assert_eq!(migration.versions.get("a"), Some(&"1.0.0".to_string()));
        assert_eq!(migration.versions.get("b"), Some(&"2.0.0".to_string()));
    }

    #[test]
    fn migrates_npm_v3_with_scoped_and_nested_packages() {
        let directory = write_lockfile(
            "package-lock.json",
            r#"{
                "name": "demo",
                "lockfileVersion": 3,
                "packages": {
                    "": { "name": "demo", "version": "1.0.0" },
                    "node_modules/@scope/a": { "version": "1.0.0" },
                    "node_modules/a": { "version": "2.0.0" },
                    "node_modules/a/node_modules/b": { "version": "3.0.0" },
                    "node_modules/b": { "version": "4.0.0" },
                    "node_modules/link": { "link": true, "resolved": "packages/link" },
                    "node_modules/workspace": { "version": "file:packages/workspace" },
                    "node_modules/gitdep": {
                        "version": "1.0.0",
                        "resolved": "git+https://github.com/example/gitdep.git#abc123"
                    }
                }
            }"#,
        );

        let migration = read_node(directory.path()).expect("read npm v3 lockfile");

        assert_eq!(
            migration.versions.get("@scope/a"),
            Some(&"1.0.0".to_string())
        );
        assert_eq!(migration.versions.get("a"), Some(&"2.0.0".to_string()));
        assert_eq!(migration.versions.get("b"), Some(&"4.0.0".to_string()));
        assert!(!migration.versions.contains_key("link"));
        assert!(!migration.versions.contains_key("workspace"));
        assert!(!migration.versions.contains_key("gitdep"));
    }

    #[test]
    fn migrates_deno_v5_lockfile() {
        let directory = write_lockfile(
            "deno.lock",
            r#"{
                "version": "5",
                "specifiers": {
                    "npm:chalk@^5.6.2": "5.6.2",
                    "npm:marked-mangle@^1.1.9": "1.1.11_marked@15.0.12",
                    "jsr:@std/path@^1.0.0": "1.0.0"
                },
                "npm": {
                    "chalk@5.6.2": { "integrity": "sha512-..." },
                    "@scope/pkg@1.2.3": { "integrity": "sha512-..." },
                    "marked-mangle@1.1.11_marked@15.0.12": { "integrity": "sha512-..." }
                }
            }"#,
        );

        let migration = read_deno(directory.path()).expect("read deno v5 lockfile");

        assert_eq!(migration.versions.get("chalk"), Some(&"5.6.2".to_string()));
        assert_eq!(
            migration.versions.get("@scope/pkg"),
            Some(&"1.2.3".to_string())
        );
        assert_eq!(
            migration.versions.get("marked-mangle"),
            Some(&"1.1.11".to_string())
        );
        assert!(!migration.versions.contains_key("1.1.11_marked"));
        assert_eq!(migration.versions.len(), 3);
    }

    #[test]
    fn migrates_deno_v3_lockfile() {
        let directory = write_lockfile(
            "deno.lock",
            r#"{
                "version": "3",
                "npm": {
                    "specifiers": {
                        "chalk@^5.3.0": "chalk@5.3.0"
                    },
                    "packages": {
                        "chalk@5.3.0": { "integrity": "sha512-..." },
                        "ansi-styles@4.3.0": { "integrity": "sha512-..." }
                    }
                }
            }"#,
        );

        let migration = read_deno(directory.path()).expect("read deno v3 lockfile");

        assert_eq!(migration.versions.get("chalk"), Some(&"5.3.0".to_string()));
        assert_eq!(
            migration.versions.get("ansi-styles"),
            Some(&"4.3.0".to_string())
        );
    }

    #[test]
    fn migrates_bun_text_lockfile() {
        let directory = write_lockfile(
            "bun.lock",
            r#"{
                "lockfileVersion": 1,
                // Bun lockfiles are JSONC and may contain trailing commas.
                "packages": {
                    "react": ["react@18.3.1", "", {}, "sha512-..."],
                    "@scope/pkg": ["@scope/pkg@1.2.3", "", {}, "sha512-..."],
                    "bun-types": ["bun-types@workspace:packages/bun-types"],
                    "local": ["local@file:../local"],
                },
            }"#,
        );

        let migration = read_bun(directory.path()).expect("read bun text lockfile");

        assert_eq!(migration.versions.get("react"), Some(&"18.3.1".to_string()));
        assert_eq!(
            migration.versions.get("@scope/pkg"),
            Some(&"1.2.3".to_string())
        );
        assert!(!migration.versions.contains_key("bun-types"));
        assert!(!migration.versions.contains_key("local"));
    }

    #[test]
    fn rejects_lockfiles_without_registry_packages() {
        let directory = write_lockfile(
            "bun.lock",
            r#"{
                "packages": {
                    "bun-types": ["bun-types@workspace:packages/bun-types"]
                }
            }"#,
        );

        let error = read_bun(directory.path()).expect_err("no registry packages");
        assert!(error.to_string().contains("No registry packages"));
    }

    #[test]
    fn splits_scoped_name_version_specifiers() {
        assert_eq!(
            split_name_version("@scope/pkg@1.2.3"),
            Some(("@scope/pkg", "1.2.3"))
        );
        assert_eq!(split_name_version("pkg@1.2.3"), Some(("pkg", "1.2.3")));
        assert_eq!(
            split_name_version("marked-mangle@1.1.11_marked@15.0.12"),
            Some(("marked-mangle", "1.1.11"))
        );
        assert_eq!(split_name_version("@scope/pkg"), None);
    }

    #[test]
    fn reads_deno_resolved_versions() {
        assert_eq!(deno_resolved_version("5.6.2"), Some("5.6.2"));
        assert_eq!(
            deno_resolved_version("1.1.11_marked@15.0.12"),
            Some("1.1.11")
        );
        assert_eq!(deno_resolved_version("chalk@5.3.0"), Some("5.3.0"));
        assert_eq!(deno_resolved_version("workspace:packages/foo"), None);
    }

    #[cfg(unix)]
    #[test]
    fn restores_binary_bun_lockfile_after_conversion() {
        use std::os::unix::fs::PermissionsExt;

        let directory = write_lockfile("bun.lockb", "binary lockfile");
        let script = directory.path().join("fake-bun.sh");
        fs::write(
            &script,
            "#!/bin/sh\nrm -f bun.lockb\ncat > bun.lock <<'EOF'\n\
             { \"packages\": { \"react\": [\"react@18.3.1\"] } }\nEOF\n",
        )
        .expect("write fake bun");

        let mut permissions = fs::metadata(&script)
            .expect("script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("make script executable");

        let text_path = directory.path().join("bun.lock");
        let binary_path = directory.path().join("bun.lockb");
        let migration = convert_bun_lockb_with(
            directory.path(),
            &text_path,
            &binary_path,
            script.as_os_str(),
        )
        .expect("convert bun.lockb");

        assert_eq!(migration.versions.get("react"), Some(&"18.3.1".to_string()));
        assert_eq!(
            fs::read_to_string(&binary_path).expect("read bun.lockb"),
            "binary lockfile"
        );
        assert!(!text_path.exists());
    }
}
