use anyhow::{Result, bail};

use crate::spec::PackageSpec;

/// A project template backed by an npm initializer package.
///
/// Templates keep the initializer invocation out of the user's way: `next`
/// becomes `create-next-app@latest`, and an unknown name falls back to the
/// conventional `create-<name>@latest` package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    /// Template name as written by the user.
    pub name: String,

    /// Initializer package to execute.
    pub spec: PackageSpec,

    /// Arguments inserted before the target directory.
    pub args: Vec<String>,
}

impl Template {
    /// Resolve a built-in template or the conventional `create-*` fallback.
    pub fn resolve(name: &str) -> Result<Self> {
        let name = name.trim();

        if name.is_empty() {
            bail!("Template name cannot be empty");
        }

        let (package, args): (String, Vec<String>) = match name {
            "next" => ("create-next-app@latest".into(), Vec::new()),
            "vite" => ("create-vite@latest".into(), Vec::new()),
            "vue" => ("create-vue@latest".into(), Vec::new()),
            "svelte" => ("create-svelte@latest".into(), Vec::new()),
            "astro" => ("create-astro@latest".into(), Vec::new()),
            "nuxt" => ("nuxi@latest".into(), vec!["init".to_string()]),
            "react" => ("create-react-app@latest".into(), Vec::new()),
            other => (format!("create-{other}@latest"), Vec::new()),
        };

        Ok(Self {
            name: name.to_string(),
            spec: PackageSpec::parse(&package)?,
            args,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_builtin_templates() {
        let next = Template::resolve("next").expect("resolve next");
        assert_eq!(next.spec.name, "create-next-app");
        assert_eq!(next.spec.version, "latest");
        assert_eq!(next.spec.command, "create-next-app");
        assert!(next.args.is_empty());

        let nuxt = Template::resolve("nuxt").expect("resolve nuxt");
        assert_eq!(nuxt.spec.name, "nuxi");
        assert_eq!(nuxt.args, ["init"]);
    }

    #[test]
    fn falls_back_to_create_packages() {
        let template = Template::resolve("solid").expect("resolve fallback");
        assert_eq!(template.name, "solid");
        assert_eq!(template.spec.name, "create-solid");
        assert_eq!(template.spec.version, "latest");
    }

    #[test]
    fn rejects_empty_template_names() {
        assert!(Template::resolve("").is_err());
        assert!(Template::resolve("   ").is_err());
    }
}
