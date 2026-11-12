use anyhow::{Result, bail};

use crate::installer::validate_package_name;

const DEFAULT_VERSION: &str = "latest";

/// Shorthand names that map to a differently named registry package.
const ALIASES: &[(&str, &str)] = &[("next-app", "create-next-app")];

/// A package requested for one-off execution, in the style of `npx`.
///
/// The input may be a plain name (`typescript`), an exact version
/// (`typescript@5.7.2`), a dist-tag (`typescript@next`), or a scoped package
/// (`@scope/tool@1.0.0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSpec {
    /// Registry package name, after resolving any shorthand alias.
    pub name: String,

    /// Requested version, tag, or range. Defaults to `latest`.
    pub version: String,

    /// Preferred executable name when the package declares more than one `bin`.
    pub command: String,
}

impl PackageSpec {
    /// Parse a command-line package specification.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();

        if input.is_empty() {
            bail!("Package name cannot be empty");
        }

        if input.starts_with('-') {
            bail!("Expected a package name, not the flag `{input}`");
        }

        let (raw_name, version) = split_version(input);
        let name = aliased(raw_name);

        validate_package_name(&name)?;

        let version = match version {
            Some(version) if !version.is_empty() => version,
            _ => DEFAULT_VERSION,
        };

        Ok(Self {
            command: executable_name(&name),
            name,
            version: version.to_string(),
        })
    }
}

/// Split `name@version`, accounting for scoped package names that also begin
/// with `@`.
fn split_version(input: &str) -> (&str, Option<&str>) {
    // Skip the leading `@` of a scope so it is not mistaken for a separator.
    let search_from = if input.starts_with('@') { 1 } else { 0 };

    match input[search_from..].rfind('@') {
        Some(offset) => {
            let separator = search_from + offset;
            (&input[..separator], Some(&input[separator + 1..]))
        }
        None => (input, None),
    }
}

fn aliased(name: &str) -> String {
    ALIASES
        .iter()
        .find(|(alias, _)| *alias == name)
        .map_or_else(|| name.to_string(), |(_, target)| (*target).to_string())
}

/// The executable name npm derives from a package name: the final path
/// segment, without the scope.
fn executable_name(name: &str) -> String {
    name.rsplit('/').next().unwrap_or(name).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_versioned_names() {
        let plain = PackageSpec::parse("typescript").expect("parse plain name");
        assert_eq!(plain.name, "typescript");
        assert_eq!(plain.version, "latest");
        assert_eq!(plain.command, "typescript");

        let versioned = PackageSpec::parse("typescript@5.7.2").expect("parse version");
        assert_eq!(versioned.name, "typescript");
        assert_eq!(versioned.version, "5.7.2");

        let tagged = PackageSpec::parse("typescript@next").expect("parse tag");
        assert_eq!(tagged.version, "next");
    }

    #[test]
    fn parses_scoped_names() {
        let spec = PackageSpec::parse("@scope/tool@1.0.0").expect("parse scoped name");
        assert_eq!(spec.name, "@scope/tool");
        assert_eq!(spec.version, "1.0.0");
        assert_eq!(spec.command, "tool");

        let plain = PackageSpec::parse("@scope/tool").expect("parse scoped name");
        assert_eq!(plain.version, "latest");
    }

    #[test]
    fn resolves_shorthand_aliases() {
        let spec = PackageSpec::parse("next-app@latest").expect("parse alias");
        assert_eq!(spec.name, "create-next-app");
        assert_eq!(spec.version, "latest");
        assert_eq!(spec.command, "create-next-app");
    }

    #[test]
    fn treats_an_empty_version_as_latest() {
        let spec = PackageSpec::parse("typescript@").expect("parse empty version");
        assert_eq!(spec.version, "latest");
    }

    #[test]
    fn rejects_invalid_specs() {
        for input in ["", "   ", "../escape", "a/b", "-g", "@scope"] {
            assert!(
                PackageSpec::parse(input).is_err(),
                "{input:?} should be rejected"
            );
        }
    }
}
