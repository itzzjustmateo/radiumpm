use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::time::Duration;

use crate::cache::Cache;
use crate::ui;

const DEFAULT_REGISTRY: &str = "https://registry.npmjs.org";
const USER_AGENT: &str = concat!("radium/", env!("CARGO_PKG_VERSION"));

/// The parts of an npm package document that Radium uses.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PackageDocument {
    #[serde(rename = "dist-tags", default)]
    pub dist_tags: BTreeMap<String, String>,
    pub versions: BTreeMap<String, VersionDocument>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionDocument {
    pub dist: Dist,
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Dist {
    pub tarball: String,
    #[serde(default)]
    pub integrity: Option<String>,
    #[serde(default)]
    pub shasum: Option<String>,
}

/// A concrete package version selected from the registry.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub version: String,
    pub dist: Dist,
    pub dependencies: BTreeMap<String, String>,
}

/// Whether cached registry metadata may be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Reuse cached metadata when it is available.
    Prefer,
    /// Always fetch fresh metadata and refresh the cache.
    Refresh,
}

/// A blocking client for the npm registry.
pub struct Registry {
    client: Client,
    base_url: String,
    cache: Option<Cache>,
}

impl Registry {
    /// Create a registry client backed by the platform cache.
    pub fn new() -> Result<Self> {
        Self::with_cache(Cache::new()?)
    }

    /// Create a registry client that uses an explicit cache.
    pub fn with_cache(cache: Cache) -> Result<Self> {
        Ok(Self {
            client: build_client()?,
            base_url: base_url(),
            cache: Some(cache),
        })
    }

    /// Create a registry client that never reads or writes cached metadata.
    #[cfg(test)]
    pub fn without_cache() -> Result<Self> {
        Ok(Self {
            client: build_client()?,
            base_url: base_url(),
            cache: None,
        })
    }

    /// The cache used by this client, if any.
    pub fn cache(&self) -> Option<&Cache> {
        self.cache.as_ref()
    }

    /// Fetch the package document for `name`, preferring cached metadata.
    pub fn package(&self, name: &str) -> Result<PackageDocument> {
        self.package_with_policy(name, CachePolicy::Prefer)
    }

    /// Fetch the package document for `name`, bypassing cached metadata.
    ///
    /// This is used by commands that intentionally re-resolve versions, such as
    /// `radium add` and `radium upgrade`.
    pub fn package_fresh(&self, name: &str) -> Result<PackageDocument> {
        self.package_with_policy(name, CachePolicy::Refresh)
    }

    fn package_with_policy(&self, name: &str, policy: CachePolicy) -> Result<PackageDocument> {
        if let Some(cache) = &self.cache
            && policy == CachePolicy::Prefer
        {
            match cache.read_registry_metadata(name) {
                Ok(Some(document)) => return Ok(document),
                Ok(None) => {}
                Err(error) => {
                    ui::warn(format!(
                        "Ignoring unreadable cached metadata for {name}: {error:#}"
                    ));
                }
            }
        }

        let document = self.fetch_package(name)?;

        if let Some(cache) = &self.cache {
            cache.write_registry_metadata(name, &document)?;
        }

        Ok(document)
    }

    fn fetch_package(&self, name: &str) -> Result<PackageDocument> {
        // Scoped packages must be percent-encoded in the registry URL.
        let encoded_name = name.replace('/', "%2F");
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), encoded_name);

        let response = self
            .client
            .get(&url)
            .send()
            .with_context(|| format!("Failed to reach npm registry for {name}"))?
            .error_for_status()
            .with_context(|| format!("Registry returned an error for {name}"))?;

        response
            .json()
            .with_context(|| format!("Failed to parse registry metadata for {name}"))
    }

    /// Download a package tarball.
    pub fn download(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(url)
            .send()
            .context("Failed to download package tarball")?
            .error_for_status()
            .context("Package download returned an error")?;

        let bytes = response.bytes().context("Failed to read package tarball")?;

        Ok(bytes.to_vec())
    }

    /// Resolve a version requirement against a package document.
    ///
    /// Supports dist-tags (`latest`), exact versions, and semver ranges such as
    /// `^1.2.0`, `~1.2.0`, and `>=1 <2`. Non-registry specs (git, file, npm
    /// aliases, workspaces) are rejected for now.
    pub fn resolve(&self, document: &PackageDocument, requested: &str) -> Result<ResolvedPackage> {
        let requested = requested.trim();

        let version = if requested.is_empty() || requested == "*" {
            document
                .dist_tags
                .get("latest")
                .cloned()
                .context("Registry did not provide a `latest` dist-tag")?
        } else if let Some(tagged) = document.dist_tags.get(requested) {
            tagged.clone()
        } else if let Ok(exact) = Version::parse(requested) {
            let exact = exact.to_string();
            if !document.versions.contains_key(&exact) {
                bail!("Version {exact} is not available");
            }
            exact
        } else {
            let requirements = parse_requirements(requested)?;

            document
                .versions
                .keys()
                .filter_map(|candidate| Version::parse(candidate).ok())
                .filter(|candidate| {
                    requirements
                        .iter()
                        .any(|requirement| requirement.matches(candidate))
                })
                .max()
                .map(|candidate| candidate.to_string())
                .with_context(|| format!("No version matching `{requested}` was found"))?
        };

        let metadata = document
            .versions
            .get(&version)
            .with_context(|| format!("Registry metadata is missing version {version}"))?;

        Ok(ResolvedPackage {
            version,
            dist: metadata.dist.clone(),
            dependencies: metadata.dependencies.clone(),
        })
    }

    /// Resolve an exact, already-pinned version from a package document.
    pub fn resolve_exact(
        &self,
        document: &PackageDocument,
        version: &str,
    ) -> Result<ResolvedPackage> {
        let metadata = document
            .versions
            .get(version)
            .with_context(|| format!("Registry metadata is missing version {version}"))?;

        Ok(ResolvedPackage {
            version: version.to_string(),
            dist: metadata.dist.clone(),
            dependencies: metadata.dependencies.clone(),
        })
    }
}

fn build_client() -> Result<Client> {
    Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(300))
        .build()
        .context("Failed to create HTTP client")
}

fn base_url() -> String {
    env::var("RADIUM_REGISTRY").unwrap_or_else(|_| DEFAULT_REGISTRY.to_string())
}

/// Parse npm's range syntax into one or more semver requirements.
///
/// The `semver` crate understands most comparators, but not npm's whitespace
/// AND, `||` OR, hyphen ranges, or `x` wildcards, so normalize those first.
fn parse_requirements(specification: &str) -> Result<Vec<VersionReq>> {
    let mut requirements = Vec::new();

    for alternative in specification.split("||") {
        let alternative = alternative.trim();
        if alternative.is_empty() {
            bail!("Unsupported version requirement `{specification}`");
        }

        let normalized = normalize_requirement(alternative);
        let requirement = VersionReq::parse(&normalized).with_context(|| {
            format!(
                "Unsupported version requirement `{specification}` (only npm semver ranges are supported)"
            )
        })?;
        requirements.push(requirement);
    }

    Ok(requirements)
}

fn normalize_requirement(alternative: &str) -> String {
    // Commas and whitespace are both AND separators in npm.
    let cleaned = alternative.replace(',', " ");
    let tokens: Vec<String> = cleaned.split_whitespace().map(replace_wildcards).collect();

    if tokens.len() == 3 && tokens[1] == "-" {
        return format!(">={}, <={}", tokens[0], tokens[2]);
    }

    let mut comparators = Vec::new();
    let mut index = 0;

    while index < tokens.len() {
        let token = &tokens[index];
        if is_operator(token) && index + 1 < tokens.len() {
            index += 1;
            comparators.push(format!("{token}{}", tokens[index]));
        } else {
            comparators.push(token.clone());
        }
        index += 1;
    }

    comparators.join(", ")
}

fn replace_wildcards(token: &str) -> String {
    let looks_like_version = token.chars().all(|character| {
        character.is_ascii_digit()
            || matches!(
                character,
                '.' | 'x' | 'X' | '*' | '^' | '~' | '<' | '>' | '='
            )
    });

    if looks_like_version {
        token.replace(['x', 'X'], "*")
    } else {
        token.to_string()
    }
}

fn is_operator(token: &str) -> bool {
    matches!(token, ">" | ">=" | "<" | "<=" | "=" | "~" | "^")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document() -> PackageDocument {
        let mut versions = BTreeMap::new();
        for version in ["1.0.0", "1.2.0", "1.2.5", "1.3.0", "2.0.0-beta.1", "2.0.0"] {
            versions.insert(
                version.to_string(),
                VersionDocument {
                    dist: Dist {
                        tarball: format!("https://example.test/pkg/-/pkg-{version}.tgz"),
                        integrity: None,
                        shasum: None,
                    },
                    dependencies: BTreeMap::new(),
                },
            );
        }

        PackageDocument {
            dist_tags: BTreeMap::from([
                ("latest".to_string(), "2.0.0".to_string()),
                ("next".to_string(), "2.0.0-beta.1".to_string()),
            ]),
            versions,
        }
    }

    fn registry() -> Registry {
        Registry {
            client: Client::new(),
            base_url: DEFAULT_REGISTRY.to_string(),
            cache: None,
        }
    }

    #[test]
    fn resolves_latest_and_tags() {
        let registry = registry();
        assert_eq!(
            registry.resolve(&document(), "latest").unwrap().version,
            "2.0.0"
        );
        assert_eq!(
            registry.resolve(&document(), "next").unwrap().version,
            "2.0.0-beta.1"
        );
    }

    #[test]
    fn resolves_exact_and_ranges() {
        let registry = registry();
        assert_eq!(
            registry.resolve(&document(), "1.2.5").unwrap().version,
            "1.2.5"
        );
        assert_eq!(
            registry.resolve(&document(), "^1.2.0").unwrap().version,
            "1.3.0"
        );
        assert_eq!(
            registry.resolve(&document(), "~1.2.0").unwrap().version,
            "1.2.5"
        );
        assert_eq!(
            registry.resolve(&document(), ">=1.2 <2").unwrap().version,
            "1.3.0"
        );
        assert_eq!(
            registry
                .resolve(&document(), ">=1.2.0, <2.0.0")
                .unwrap()
                .version,
            "1.3.0"
        );
        assert_eq!(
            registry.resolve(&document(), "1.x").unwrap().version,
            "1.3.0"
        );
        assert_eq!(
            registry.resolve(&document(), "^1.2 || ^3").unwrap().version,
            "1.3.0"
        );
        assert_eq!(
            registry
                .resolve(&document(), "1.2.0 - 1.2.5")
                .unwrap()
                .version,
            "1.2.5"
        );
    }

    #[test]
    fn rejects_unsupported_and_missing_versions() {
        let registry = registry();
        assert!(registry.resolve(&document(), "workspace:*").is_err());
        assert!(registry.resolve(&document(), "9.9.9").is_err());
        assert!(registry.resolve(&document(), "^9").is_err());
    }
}
