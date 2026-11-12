use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use flate2::read::GzDecoder;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use tar::Archive;

use crate::hash::to_hex;
use crate::lockfile::Lockfile;
use crate::project::PackageJson;
use crate::registry::{CachePolicy, Dist, Registry, ResolvedPackage};
use crate::ui;

/// Installs packages from the registry into a project's `node_modules`.
pub struct Installer<'a> {
    registry: &'a Registry,
    root: PathBuf,
    resolved: BTreeMap<String, String>,
}

impl<'a> Installer<'a> {
    pub fn new(registry: &'a Registry, root: impl Into<PathBuf>) -> Self {
        Self {
            registry,
            root: root.into(),
            resolved: BTreeMap::new(),
        }
    }

    /// Resolved package name -> version for everything installed in this run.
    pub fn resolved(&self) -> &BTreeMap<String, String> {
        &self.resolved
    }

    /// Install `name`, recursively installing its runtime dependencies.
    ///
    /// The dependency tree is flat, so if a package is already resolved in this
    /// run the existing version is reused. Circular dependencies are safe
    /// because the package is recorded before its dependencies are visited.
    ///
    /// Version ranges are re-resolved against fresh registry metadata. Use
    /// [`Installer::install_cached`] for one-off executions that should work
    /// offline, or [`Installer::install_locked`] for reproducible installs from
    /// `radium.lock`.
    pub fn install(&mut self, name: &str, requested: &str) -> Result<String> {
        self.install_with(name, requested, CachePolicy::Refresh)
    }

    /// Install `name` for a one-off execution, preferring cached metadata.
    ///
    /// This is the same resolution as [`Installer::install`] except that a
    /// cached registry document is used when available, so packages that were
    /// seen before can run without network access.
    pub fn install_cached(&mut self, name: &str, requested: &str) -> Result<String> {
        self.install_with(name, requested, CachePolicy::Prefer)
    }

    fn install_with(&mut self, name: &str, requested: &str, policy: CachePolicy) -> Result<String> {
        validate_package_name(name)?;

        if let Some(version) = self.resolved.get(name) {
            return Ok(version.clone());
        }

        let document = match policy {
            CachePolicy::Prefer => self.registry.package(name)?,
            CachePolicy::Refresh => self.registry.package_fresh(name)?,
        };
        let package = self.registry.resolve(&document, requested)?;
        self.install_package(name, &package)?;

        for (dependency, requirement) in &package.dependencies {
            self.install_with(dependency, requirement, policy)?;
        }

        Ok(package.version)
    }

    /// Install `name` at the exact version recorded in `radium.lock`.
    ///
    /// Every package in the dependency graph must have a lockfile entry; a
    /// missing entry is an error rather than an invitation to re-resolve, which
    /// keeps builds reproducible.
    pub fn install_locked(&mut self, name: &str, lockfile: &Lockfile) -> Result<String> {
        validate_package_name(name)?;

        if let Some(version) = self.resolved.get(name) {
            return Ok(version.clone());
        }

        let version = lockfile
            .packages
            .get(name)
            .map(|package| package.version.clone())
            .with_context(|| {
                format!(
                    "`{name}` is not recorded in radium.lock; run `radium add {name}` or `radium upgrade` to update the lockfile"
                )
            })?;

        let package = self.resolve_locked(name, &version)?;
        self.install_package(name, &package)?;

        for dependency in package.dependencies.keys() {
            self.install_locked(dependency, lockfile)?;
        }

        Ok(package.version)
    }

    /// Resolve the metadata for an exact locked version.
    ///
    /// Cached per-version metadata is preferred so installs can work offline.
    /// When it is unavailable, the registry document is consulted (using its
    /// cache when possible) and only refetched if the version is missing.
    fn resolve_locked(&self, name: &str, version: &str) -> Result<ResolvedPackage> {
        if let Some(cache) = self.registry.cache() {
            match cache.package_version(name, version) {
                Ok(Some(package)) => return Ok(package.into_resolved()),
                Ok(None) => {}
                Err(error) => {
                    ui::warn(format!(
                        "Ignoring unreadable cached metadata for {name}@{version}: {error:#}"
                    ));
                }
            }
        }

        let document = self.registry.package(name)?;
        match self.registry.resolve_exact(&document, version) {
            Ok(package) => Ok(package),
            Err(_) => {
                let document = self.registry.package_fresh(name)?;
                self.registry.resolve_exact(&document, version)
            }
        }
    }

    /// Download (or reuse) and extract one resolved package.
    fn install_package(&mut self, name: &str, package: &ResolvedPackage) -> Result<()> {
        let destination = self.root.join("node_modules").join(name);

        let existing_version = PackageJson::load(&destination)
            .ok()
            .and_then(|package| package.version);

        if existing_version.as_deref() == Some(package.version.as_str()) {
            ui::info(format!("Using existing {name}@{}", package.version));
        } else {
            ui::info(format!("Downloading {name}@{}...", package.version));
            let tarball = self.tarball(name, package)?;
            extract_package(&tarball, &destination)?;
        }

        self.resolved
            .insert(name.to_string(), package.version.clone());

        if let Some(cache) = self.registry.cache() {
            cache.record_package(name, package)?;
        }

        Ok(())
    }

    /// Return a verified tarball, reusing the content-addressed cache when it is
    /// valid and downloading it otherwise.
    fn tarball(&self, name: &str, package: &ResolvedPackage) -> Result<Vec<u8>> {
        if let Some(cache) = self.registry.cache()
            && let Some(hash) = cache.tarball_hash(name, &package.version)?
        {
            match cache.read_tarball(&hash) {
                Ok(Some(bytes)) if verify_integrity(&bytes, &package.dist).is_ok() => {
                    return Ok(bytes);
                }
                Ok(Some(_)) => {
                    ui::warn(format!(
                        "Cached tarball for {name}@{} failed verification; downloading it again",
                        package.version
                    ));
                }
                Ok(None) => {}
                Err(error) => {
                    ui::warn(format!(
                        "Ignoring unreadable cached tarball for {name}@{}: {error:#}",
                        package.version
                    ));
                }
            }

            cache.remove_tarball_entry(name, &package.version)?;
            cache.remove_tarball(&hash)?;
        }

        let bytes = self.registry.download(&package.dist.tarball)?;
        verify_integrity(&bytes, &package.dist)?;

        if let Some(cache) = self.registry.cache() {
            let hash = cache.write_tarball(&bytes)?;
            cache.record_tarball(name, &package.version, &hash)?;
        }

        Ok(bytes)
    }
}

/// Reject names that are not safe to use as a path below `node_modules`.
pub fn validate_package_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Package name cannot be empty");
    }

    if name.len() > 214 {
        bail!("Package name is too long: {name}");
    }

    if name.contains('\\') || name.contains("..") || name.starts_with('/') {
        bail!("Invalid package name: {name}");
    }

    let (scope, package) = if let Some(rest) = name.strip_prefix('@') {
        let (scope, package) = rest
            .split_once('/')
            .with_context(|| format!("Scoped package name must look like @scope/name: {name}"))?;

        if package.contains('/') {
            bail!("Invalid package name: {name}");
        }

        (Some(scope), package)
    } else {
        if name.contains('/') {
            bail!("Invalid package name: {name}");
        }

        (None, name)
    };

    if let Some(scope) = scope {
        validate_segment(scope)?;
    }
    validate_segment(package)?;

    Ok(())
}

fn validate_segment(segment: &str) -> Result<()> {
    if segment.is_empty() || segment.starts_with('.') || segment.starts_with('_') {
        bail!("Invalid package name segment: {segment:?}");
    }

    if !segment.chars().all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '~')
    }) {
        bail!("Invalid character in package name segment: {segment:?}");
    }

    Ok(())
}

/// Verify a tarball against its registry integrity metadata.
fn verify_integrity(bytes: &[u8], dist: &Dist) -> Result<()> {
    if let Some(integrity) = dist.integrity.as_deref() {
        for entry in integrity.split_whitespace() {
            let Some((algorithm, encoded)) = entry.split_once('-') else {
                continue;
            };

            let encoded = encoded.split('?').next().unwrap_or(encoded);

            let actual = match algorithm {
                "sha1" => Sha1::digest(bytes).to_vec(),
                "sha256" => Sha256::digest(bytes).to_vec(),
                "sha512" => Sha512::digest(bytes).to_vec(),
                _ => continue,
            };

            let expected = BASE64.decode(encoded).with_context(|| {
                format!("Invalid {algorithm} integrity hash in registry metadata")
            })?;

            if actual == expected {
                return Ok(());
            }

            bail!("Integrity check failed for downloaded package");
        }

        bail!("No supported integrity hash found in registry metadata");
    }

    if let Some(shasum) = dist.shasum.as_deref() {
        let actual = to_hex(&Sha1::digest(bytes));
        if actual.eq_ignore_ascii_case(shasum) {
            return Ok(());
        }

        bail!("SHA-1 checksum mismatch for downloaded package");
    }

    // Older or third-party registries may not publish any hashes.
    Ok(())
}

fn extract_package(bytes: &[u8], destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .with_context(|| format!("Invalid package path: {}", destination.display()))?;

    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;

    // Extract beside the destination so the final rename stays on one filesystem.
    let temporary = tempfile::Builder::new()
        .prefix(".radium-")
        .tempdir_in(parent)
        .with_context(|| {
            format!(
                "Failed to create a temporary directory in {}",
                parent.display()
            )
        })?;

    let mut archive = Archive::new(GzDecoder::new(bytes));
    archive
        .unpack(temporary.path())
        .context("Failed to extract package tarball")?;

    let content_root = find_content_root(temporary.path())?;

    if destination.exists() {
        if destination.is_dir() {
            fs::remove_dir_all(destination)
                .with_context(|| format!("Failed to remove {}", destination.display()))?;
        } else {
            fs::remove_file(destination)
                .with_context(|| format!("Failed to remove {}", destination.display()))?;
        }
    }

    if content_root == temporary.path() {
        // The archive has no single top-level directory; move each entry.
        fs::create_dir_all(destination)
            .with_context(|| format!("Failed to create {}", destination.display()))?;

        for entry in fs::read_dir(&content_root)? {
            let entry = entry?;
            fs::rename(entry.path(), destination.join(entry.file_name())).with_context(|| {
                format!(
                    "Failed to move archive entry into {}",
                    destination.display()
                )
            })?;
        }
    } else {
        fs::rename(&content_root, destination)
            .with_context(|| format!("Failed to move package into {}", destination.display()))?;
    }

    Ok(())
}

/// npm archives normally wrap their contents in `package/`, but some packages
/// use their own name or put files at the archive root. Find the directory that
/// should become `node_modules/<name>`.
fn find_content_root(temporary: &Path) -> Result<PathBuf> {
    let package_directory = temporary.join("package");
    if package_directory.is_dir() {
        return Ok(package_directory);
    }

    let mut directories = Vec::new();
    for entry in fs::read_dir(temporary)
        .with_context(|| format!("Failed to inspect {}", temporary.display()))?
    {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            directories.push(entry.path());
        }
    }

    if directories.len() == 1 {
        return Ok(directories.remove(0));
    }

    // Multiple top-level entries (or files only): treat the archive root as
    // the package directory and move each entry.
    Ok(temporary.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::Compression;
    use flate2::write::GzEncoder;

    #[test]
    fn accepts_valid_package_names() {
        for name in ["typescript", "@types/node", "lodash.merge", "some~pkg"] {
            assert!(
                validate_package_name(name).is_ok(),
                "{name} should be valid"
            );
        }
    }

    #[test]
    fn rejects_unsafe_package_names() {
        for name in [
            "",
            "../escape",
            "/absolute",
            "a/b",
            "@scope",
            "@scope/../x",
            "a\\b",
        ] {
            assert!(
                validate_package_name(name).is_err(),
                "{name} should be rejected"
            );
        }
    }

    #[test]
    fn verifies_sha512_integrity() {
        let bytes = b"radium";
        let hash = BASE64.encode(Sha512::digest(bytes));
        let dist = Dist {
            tarball: String::new(),
            integrity: Some(format!("sha512-{hash}")),
            shasum: None,
        };

        assert!(verify_integrity(bytes, &dist).is_ok());

        let wrong = Dist {
            integrity: Some(format!("sha512-{}", BASE64.encode([0_u8; 64]))),
            ..dist
        };
        assert!(verify_integrity(bytes, &wrong).is_err());
    }

    const PACKAGE_JSON: &[u8] = br#"{"name":"demo","version":"1.0.0"}"#;

    fn make_tarball(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));

        for &(path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, contents)
                .expect("append archive entry");
        }

        let encoder = tar.into_inner().expect("finish archive");
        encoder.finish().expect("finish gzip")
    }

    fn extract_and_load(entries: &[(&str, &[u8])]) {
        let bytes = make_tarball(entries);
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("node_modules").join("demo");

        extract_package(&bytes, &destination).expect("extract package");

        let package = PackageJson::load(&destination).expect("load extracted package.json");
        assert_eq!(package.name.as_deref(), Some("demo"));
    }

    #[test]
    fn extracts_tarball_package_directory() {
        extract_and_load(&[("package/package.json", PACKAGE_JSON)]);
    }

    #[test]
    fn extracts_tarball_with_named_top_level_directory() {
        extract_and_load(&[("ms/package.json", PACKAGE_JSON)]);
    }

    #[test]
    fn extracts_flat_tarball_root() {
        extract_and_load(&[("package.json", PACKAGE_JSON)]);
    }

    #[test]
    fn locked_install_requires_a_lockfile_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = Registry::without_cache().expect("registry");
        let mut installer = Installer::new(&registry, dir.path());
        let lockfile = Lockfile::from_versions(None, &BTreeMap::new());

        let error = installer
            .install_locked("missing", &lockfile)
            .expect_err("missing lockfile entry should fail");

        assert!(format!("{error:#}").contains("is not recorded in radium.lock"));
    }
}
