use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const PACKAGE_JSON: &str = "package.json";
const LOCKFILE: &str = "radium.lock";
const PACKAGE_TEMPLATE: &str = include_str!("../resources/templates/package.json");
const INDEX_TEMPLATE: &str = include_str!("../resources/templates/index.ts");

/// A package.json file, preserving fields Radium does not understand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageJson {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,

    #[serde(default)]
    pub scripts: BTreeMap<String, String>,

    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,

    #[serde(default, rename = "devDependencies")]
    pub dev_dependencies: BTreeMap<String, String>,

    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl PackageJson {
    /// Path to the package.json for a project root.
    pub fn path(root: &Path) -> PathBuf {
        root.join(PACKAGE_JSON)
    }

    /// Read and parse `<root>/package.json`.
    pub fn load(root: &Path) -> Result<Self> {
        let path = Self::path(root);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Write a deterministic, pretty-printed `<root>/package.json`.
    pub fn save(&self, root: &Path) -> Result<()> {
        let path = Self::path(root);
        let mut content =
            serde_json::to_string_pretty(self).context("Failed to serialize package.json")?;
        content.push('\n');

        fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
    }

    /// Add or replace a runtime dependency.
    pub fn set_dependency(&mut self, name: &str, version: &str) {
        self.dependencies
            .insert(name.to_string(), version.to_string());
    }
}

/// Whether `root` already looks like a Radium project.
pub fn is_project(root: &Path) -> bool {
    PackageJson::path(root).is_file() && lockfile_path(root).is_file()
}

/// Path to the lockfile for a project root.
pub fn lockfile_path(root: &Path) -> PathBuf {
    root.join(LOCKFILE)
}

/// Scaffold a new project in `root`.
///
/// The caller is responsible for checking whether a package.json already
/// exists. The starter source file is only written when one is not present.
pub fn create(root: &Path, name: &str, author: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("Project name cannot be empty");
    }

    fs::create_dir_all(root)
        .with_context(|| format!("Failed to create project directory {}", root.display()))?;

    let package_json = PACKAGE_TEMPLATE
        .replace("{{NAME}}", name)
        .replace("{{AUTHOR}}", author);

    fs::write(PackageJson::path(root), package_json)
        .with_context(|| format!("Failed to create {}", PackageJson::path(root).display()))?;

    let index_path = root.join("index.ts");
    if !index_path.exists() {
        fs::write(&index_path, INDEX_TEMPLATE)
            .with_context(|| format!("Failed to create {}", index_path.display()))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_unknown_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let raw = r#"{
            "name": "demo",
            "version": "1.0.0",
            "main": "index.js",
            "scripts": { "test": "node test.js" },
            "dependencies": { "typescript": "5.0.0" }
        }"#;
        fs::write(PackageJson::path(dir.path()), raw).expect("write package.json");

        let mut package = PackageJson::load(dir.path()).expect("load package.json");
        package.set_dependency("zod", "3.0.0");
        package.save(dir.path()).expect("save package.json");

        let reloaded = PackageJson::load(dir.path()).expect("reload package.json");
        assert_eq!(reloaded.dependencies.get("zod"), Some(&"3.0.0".to_string()));
        assert_eq!(
            reloaded.other.get("main"),
            Some(&Value::String("index.js".to_string()))
        );
        assert_eq!(
            reloaded.scripts.get("test"),
            Some(&"node test.js".to_string())
        );
    }

    #[test]
    fn create_writes_template_without_overwriting_index() {
        let dir = tempfile::tempdir().expect("temp dir");
        let index = dir.path().join("index.ts");
        fs::write(&index, "// custom\n").expect("write index");

        create(dir.path(), "demo", "Jane").expect("create project");

        let package = PackageJson::load(dir.path()).expect("load package.json");
        assert_eq!(package.name.as_deref(), Some("demo"));
        assert_eq!(
            fs::read_to_string(index).expect("read index"),
            "// custom\n"
        );
    }
}
