use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::project::{self, PackageJson};

/// Current on-disk lockfile format version.
pub const LOCKFILE_VERSION: u32 = 1;

/// The `radium.lock` file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(rename = "lockfileVersion")]
    pub lockfile_version: u32,

    pub packages: BTreeMap<String, LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    pub version: String,
}

impl Lockfile {
    /// Build a lockfile from an explicit package -> version map.
    pub fn from_versions(name: Option<String>, versions: &BTreeMap<String, String>) -> Self {
        let packages = versions
            .iter()
            .map(|(name, version)| {
                (
                    name.clone(),
                    LockedPackage {
                        version: version.clone(),
                    },
                )
            })
            .collect();

        Self {
            name,
            lockfile_version: LOCKFILE_VERSION,
            packages,
        }
    }

    /// Build a lockfile containing only the direct dependencies in package.json.
    pub fn from_package(package: &PackageJson) -> Self {
        Self::from_versions(package.name.clone(), &package.dependencies)
    }

    /// Load `<root>/radium.lock`.
    pub fn load(root: &Path) -> Result<Self> {
        let path = project::lockfile_path(root);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Could not read {}", path.display()))?;

        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Write `<root>/radium.lock` in a deterministic, pretty-printed format.
    pub fn save(&self, root: &Path) -> Result<()> {
        let path = project::lockfile_path(root);
        let mut content =
            serde_json::to_string_pretty(self).context("Failed to serialize lockfile")?;
        content.push('\n');

        fs::write(&path, content).with_context(|| format!("Failed to write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_stable_sorted_output() {
        let dir = tempfile::tempdir().expect("temp dir");
        let versions = BTreeMap::from([
            ("zod".to_string(), "3.0.0".to_string()),
            ("esbuild".to_string(), "0.25.0".to_string()),
        ]);

        let lockfile = Lockfile::from_versions(Some("demo".to_string()), &versions);
        lockfile.save(dir.path()).expect("save lockfile");

        let raw = fs::read_to_string(project::lockfile_path(dir.path())).expect("read lockfile");
        let parsed: Lockfile = serde_json::from_str(&raw).expect("parse lockfile");

        assert_eq!(parsed.lockfile_version, LOCKFILE_VERSION);
        assert_eq!(parsed.name.as_deref(), Some("demo"));
        assert!(raw.find("esbuild") < raw.find("zod"));
    }
}
