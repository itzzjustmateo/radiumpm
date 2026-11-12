use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::hash::sha256_hex;
use crate::registry::{Dist, PackageDocument, ResolvedPackage};

/// Environment variable that overrides the platform cache directory.
pub const CACHE_DIR_ENV: &str = "RADIUM_CACHE_DIR";

/// A package tarball cache keyed by the SHA-256 of its contents.
#[derive(Debug, Clone)]
pub struct Cache {
    root: PathBuf,
}

/// Per-package index of cached tarballs, keyed by resolved version.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageIndex {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub tarballs: BTreeMap<String, String>,
}

/// Cached metadata for one resolved package version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPackage {
    pub version: String,
    pub dist: Dist,

    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

impl CachedPackage {
    pub fn into_resolved(self) -> ResolvedPackage {
        ResolvedPackage {
            version: self.version,
            dist: self.dist,
            dependencies: self.dependencies,
        }
    }
}

/// Cached resolved metadata for every version of a package seen so far.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PackageMetadata {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub versions: BTreeMap<String, CachedPackage>,
}

impl Cache {
    /// Create a cache at the platform-specific location.
    ///
    /// Set `RADIUM_CACHE_DIR` to override the location, which is useful for
    /// tests, CI, and keeping a project-local cache.
    pub fn new() -> Result<Self> {
        let root = match env::var_os(CACHE_DIR_ENV) {
            Some(root) => PathBuf::from(root),
            None => default_root()?,
        };

        Ok(Self { root })
    }

    /// Create a cache rooted at an explicit directory.
    #[cfg(test)]
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Ensure the standard cache directory layout exists.
    pub fn ensure(&self) -> Result<()> {
        for path in [
            self.tarball_dir(),
            self.registry_dir(),
            self.package_metadata_dir(),
            self.index_dir(),
            self.tmp_dir(),
        ] {
            fs::create_dir_all(&path)
                .with_context(|| format!("Failed to create cache directory {}", path.display()))?;
        }

        Ok(())
    }

    /// Path to a content-addressed tarball.
    pub fn tarball_path(&self, hash: &str) -> PathBuf {
        let prefix = hash.get(..2).unwrap_or("00");
        self.tarball_dir().join(prefix).join(hash)
    }

    /// Store a tarball by the SHA-256 of its contents and return that hash.
    pub fn write_tarball(&self, bytes: &[u8]) -> Result<String> {
        self.ensure()?;

        let hash = sha256_hex(bytes);
        let path = self.tarball_path(&hash);

        if !path.exists() {
            let parent = path.parent().context("Invalid tarball cache path")?;
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;

            let temporary = self.tmp_dir().join(format!("{hash}.tarball"));
            fs::write(&temporary, bytes)
                .with_context(|| format!("Failed to write {}", temporary.display()))?;

            match fs::rename(&temporary, &path) {
                Ok(()) => {}
                Err(_) if path.exists() => {
                    let _ = fs::remove_file(&temporary);
                }
                Err(error) => {
                    let _ = fs::remove_file(&temporary);
                    return Err(error)
                        .with_context(|| format!("Failed to store {}", path.display()));
                }
            }
        }

        Ok(hash)
    }

    /// Read a cached tarball by its hash, or return `None` if it is absent.
    pub fn read_tarball(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let path = self.tarball_path(hash);

        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("Failed to read {}", path.display())),
        }
    }

    /// Remove a cached tarball. Silently succeeds when the path does not exist.
    pub fn remove_tarball(&self, hash: &str) -> Result<()> {
        let path = self.tarball_path(hash);

        match fs::remove_file(&path) {
            Ok(()) => {
                if let Some(parent) = path.parent() {
                    // This only succeeds when the two-character prefix is empty.
                    let _ = fs::remove_dir(parent);
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("Failed to remove {}", path.display()))
            }
        }
    }

    /// Look up a cached tarball hash for a resolved version, if any.
    pub fn tarball_hash(&self, name: &str, version: &str) -> Result<Option<String>> {
        Ok(self
            .read_index(name)?
            .and_then(|index| index.tarballs.get(version).cloned()))
    }

    /// Remove one version's entry from a package index without touching the
    /// tarball, which may still be referenced by another package.
    pub fn remove_tarball_entry(&self, name: &str, version: &str) -> Result<()> {
        let Some(mut index) = self.read_index(name)? else {
            return Ok(());
        };

        if index.tarballs.remove(version).is_some() {
            if index.tarballs.is_empty() {
                self.remove_file(&self.index_path(name))?;
            } else {
                self.write_index(name, &index)?;
            }
        }

        Ok(())
    }

    /// Record a resolved package version into its per-package metadata file.
    pub fn record_package(&self, name: &str, package: &ResolvedPackage) -> Result<()> {
        let mut metadata = self.read_package_metadata(name)?.unwrap_or_default();
        metadata.name = name.to_string();
        metadata.versions.insert(
            package.version.clone(),
            CachedPackage {
                version: package.version.clone(),
                dist: package.dist.clone(),
                dependencies: package.dependencies.clone(),
            },
        );

        self.write_package_metadata(name, &metadata)
    }

    /// Record a resolved tarball into its per-package index.
    pub fn record_tarball(&self, name: &str, version: &str, hash: &str) -> Result<()> {
        let mut index = self.read_index(name)?.unwrap_or_default();
        index.name = name.to_string();
        index.tarballs.insert(version.to_string(), hash.to_string());
        self.write_index(name, &index)
    }

    /// Read one cached resolved package version from its metadata file.
    pub fn package_version(&self, name: &str, version: &str) -> Result<Option<CachedPackage>> {
        Ok(self
            .read_package_metadata(name)?
            .and_then(|metadata| metadata.versions.get(version).cloned()))
    }

    /// Remove a package's index entry and any cached tarball that is not shared with
    /// another package. Returns whether anything was removed.
    pub fn clear_package(&self, name: &str) -> Result<bool> {
        let mut existed = false;
        let hashes: Vec<String> = self
            .read_index(name)?
            .map(|index| index.tarballs.into_values().collect())
            .unwrap_or_default();
        let shared = self.all_tarball_hashes_except(name)?;

        for hash in hashes {
            if !shared.contains(&hash) && self.tarball_path(&hash).exists() {
                self.remove_tarball(&hash)?;
                existed = true;
            }
        }

        for path in [
            self.index_path(name),
            self.package_metadata_path(name),
            self.registry_metadata_path(name),
        ] {
            if path.exists() {
                self.remove_file(&path)?;
                existed = true;
            }
        }

        Ok(existed)
    }

    /// Remove the entire cache. Silently succeeds when the directory does not exist.
    pub fn clear(&self) -> Result<()> {
        match fs::remove_dir_all(&self.root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("Failed to remove {}", self.root.display()))
            }
        }
    }

    /// Enumerate every shared tarball hash, excluding the one owned by `name`.
    fn all_tarball_hashes_except(&self, name: &str) -> Result<BTreeSet<String>> {
        let mut hashes = BTreeSet::new();
        let directory = self.index_dir();

        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(hashes),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to read {}", directory.display()));
            }
        };

        let skip = self.index_path(name);
        for entry in entries {
            let path = entry?.path();
            if path == skip || path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            if let Ok(Some(index)) = self.read_json::<PackageIndex>(&path) {
                hashes.extend(index.tarballs.into_values());
            }
        }

        Ok(hashes)
    }

    fn registry_metadata_path(&self, name: &str) -> PathBuf {
        self.registry_dir()
            .join(format!("{}.json", encode_name(name)))
    }

    pub fn read_registry_metadata(&self, name: &str) -> Result<Option<PackageDocument>> {
        self.read_json(&self.registry_metadata_path(name))
    }

    pub fn write_registry_metadata(&self, name: &str, document: &PackageDocument) -> Result<()> {
        self.write_json(&self.registry_metadata_path(name), document)
    }

    fn package_metadata_path(&self, name: &str) -> PathBuf {
        self.package_metadata_dir()
            .join(format!("{}.json", encode_name(name)))
    }

    fn read_package_metadata(&self, name: &str) -> Result<Option<PackageMetadata>> {
        self.read_json(&self.package_metadata_path(name))
    }

    fn write_package_metadata(&self, name: &str, metadata: &PackageMetadata) -> Result<()> {
        self.write_json(&self.package_metadata_path(name), metadata)
    }

    fn index_path(&self, name: &str) -> PathBuf {
        self.index_dir().join(format!("{}.json", encode_name(name)))
    }

    fn read_index(&self, name: &str) -> Result<Option<PackageIndex>> {
        self.read_json(&self.index_path(name))
    }

    fn write_index(&self, name: &str, index: &PackageIndex) -> Result<()> {
        self.write_json(&self.index_path(name), index)
    }

    fn read_json<T: for<'de> Deserialize<'de>>(&self, path: &Path) -> Result<Option<T>> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to read {}", path.display()));
            }
        };

        serde_json::from_str(&contents)
            .map(Some)
            .with_context(|| format!("Failed to parse {}", path.display()))
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let contents =
            serde_json::to_vec_pretty(value).context("Failed to serialize cache entry")?;
        self.write_file(path, &contents)
    }

    fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
        self.ensure()?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        fs::write(path, contents).with_context(|| format!("Failed to write {}", path.display()))
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("Failed to remove {}", path.display()))
            }
        }
    }

    fn tarball_dir(&self) -> PathBuf {
        self.root.join("packages").join("sha256")
    }

    fn registry_dir(&self) -> PathBuf {
        self.root.join("metadata").join("registry")
    }

    fn package_metadata_dir(&self) -> PathBuf {
        self.root.join("metadata").join("packages")
    }

    fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }
}

/// Encode a package name so it is safe as a single filename on every platform.
pub fn encode_name(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len());

    for character in name.chars() {
        match character {
            '%' => encoded.push_str("%25"),
            '/' => encoded.push_str("%2F"),
            '\\' => encoded.push_str("%5C"),
            ':' => encoded.push_str("%3A"),
            '*' => encoded.push_str("%2A"),
            '?' => encoded.push_str("%3F"),
            '"' => encoded.push_str("%22"),
            '<' => encoded.push_str("%3C"),
            '>' => encoded.push_str("%3E"),
            '|' => encoded.push_str("%7C"),
            _ => encoded.push(character),
        }
    }

    encoded
}

fn default_root() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = env::var_os("LOCALAPPDATA")
            .context("LOCALAPPDATA is not set; cannot determine the cache directory")?;
        return Ok(PathBuf::from(base).join("radium").join("Cache"));
    }

    #[cfg(target_os = "macos")]
    {
        let home =
            env::var_os("HOME").context("HOME is not set; cannot determine the cache directory")?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Caches")
            .join("radium"));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(cache) = env::var_os("XDG_CACHE_HOME") {
            return Ok(PathBuf::from(cache).join("radium"));
        }

        let home = env::var_os("HOME").context(
            "Neither XDG_CACHE_HOME nor HOME is set; cannot determine the cache directory",
        )?;
        Ok(PathBuf::from(home).join(".cache").join("radium"))
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        Ok(env::temp_dir().join("radium"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(version: &str) -> ResolvedPackage {
        ResolvedPackage {
            version: version.to_string(),
            dist: Dist {
                tarball: format!("https://example.test/demo/-/demo-{version}.tgz"),
                integrity: None,
                shasum: None,
            },
            dependencies: BTreeMap::new(),
        }
    }

    #[test]
    fn stores_tarballs_by_sha256() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = Cache::at(directory.path());

        let hash = cache
            .write_tarball(b"tarball bytes")
            .expect("write tarball");
        let expected = directory
            .path()
            .join("packages")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash);

        assert!(expected.is_file());
        assert_eq!(
            cache.read_tarball(&hash).expect("read tarball"),
            Some(b"tarball bytes".to_vec())
        );
    }

    #[test]
    fn records_package_metadata_and_index() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = Cache::at(directory.path());

        cache
            .record_package("demo", &package("1.0.0"))
            .expect("record package");
        cache
            .record_tarball("demo", "1.0.0", "abc123")
            .expect("record tarball");

        let cached = cache
            .package_version("demo", "1.0.0")
            .expect("read package")
            .expect("cached package");
        assert_eq!(cached.version, "1.0.0");
        assert_eq!(
            cache.tarball_hash("demo", "1.0.0").expect("read index"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn clears_one_package_without_removing_shared_tarballs() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = Cache::at(directory.path());
        let hash = cache.write_tarball(b"shared").expect("write tarball");

        cache
            .record_package("demo", &package("1.0.0"))
            .expect("record demo");
        cache
            .record_tarball("demo", "1.0.0", &hash)
            .expect("index demo");
        cache
            .record_package("other", &package("2.0.0"))
            .expect("record other");
        cache
            .record_tarball("other", "2.0.0", &hash)
            .expect("index other");

        assert!(cache.clear_package("demo").expect("clear demo"));
        assert!(cache.tarball_path(&hash).is_file());
        assert!(!cache.package_metadata_path("demo").exists());
        assert!(cache.package_metadata_path("other").exists());

        assert!(cache.clear_package("other").expect("clear other"));
        assert!(!cache.tarball_path(&hash).is_file());
    }

    #[test]
    fn clears_the_entire_cache() {
        let directory = tempfile::tempdir().expect("temp dir");
        let cache = Cache::at(directory.path());
        cache.write_tarball(b"tarball").expect("write tarball");

        cache.clear().expect("clear cache");
        assert!(!directory.path().exists());
    }

    #[test]
    fn encodes_scoped_package_names() {
        assert_eq!(encode_name("@scope/pkg"), "@scope%2Fpkg");
        assert_eq!(encode_name("plain"), "plain");
    }
}
