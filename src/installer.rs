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

use crate::project::PackageJson;
use crate::registry::{Dist, Registry};
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
    pub fn install(&mut self, name: &str, requested: &str) -> Result<String> {
        validate_package_name(name)?;

        if let Some(version) = self.resolved.get(name) {
            return Ok(version.clone());
        }

        let document = self.registry.package(name)?;
        let package = self.registry.resolve(&document, requested)?;
        let destination = self.root.join("node_modules").join(name);

        let existing_version = PackageJson::load(&destination)
            .ok()
            .and_then(|package| package.version);

        if existing_version.as_deref() == Some(package.version.as_str()) {
            ui::info(format!("Using existing {name}@{}", package.version));
        } else {
            ui::info(format!("Downloading {name}@{}...", package.version));
            let tarball = self.registry.download(&package.dist.tarball)?;
            verify_integrity(&tarball, &package.dist)?;
            extract_package(&tarball, &destination)?;
        }

        self.resolved
            .insert(name.to_string(), package.version.clone());

        for (dependency, requirement) in &package.dependencies {
            self.install(dependency, requirement)?;
        }

        Ok(package.version)
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

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a String cannot fail.
        let _ = write!(output, "{byte:02x}");
    }
    output
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
    fn writes_hex() {
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
