use anyhow::{Context, Result, bail};
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::project::PackageJson;

/// JavaScript runtimes Radium knows how to drive.
///
/// Node is the only implemented runtime today. Deno and Bun are intentionally
/// rejected with a clear message so the abstraction can grow without changing
/// the CLI or command implementations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Node,
}

impl RuntimeKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
        }
    }

    pub const fn executable(self) -> &'static str {
        match self {
            Self::Node => "node",
        }
    }
}

/// A resolved JavaScript runtime, selected with `RADIUM_RUNTIME` (default: node).
#[derive(Debug, Clone)]
pub struct Runtime {
    kind: RuntimeKind,
}

impl Runtime {
    pub fn detect() -> Result<Self> {
        let requested = env::var("RADIUM_RUNTIME").unwrap_or_else(|_| "node".to_string());

        let kind = match requested.to_ascii_lowercase().as_str() {
            "node" | "nodejs" => RuntimeKind::Node,
            "bun" => bail!(
                "Bun support is planned but not implemented yet; set RADIUM_RUNTIME=node to continue"
            ),
            "deno" => bail!(
                "Deno support is planned but not implemented yet; set RADIUM_RUNTIME=node to continue"
            ),
            other => bail!("Unknown JavaScript runtime `{other}`; supported runtimes: node"),
        };

        Ok(Self { kind })
    }

    pub fn name(&self) -> &'static str {
        self.kind.name()
    }

    /// Query the runtime version, which also verifies that it is installed.
    pub fn version(&self) -> Result<String> {
        let output = Command::new(self.kind.executable())
            .arg("--version")
            .output()
            .with_context(|| {
                format!(
                    "Could not run `{}`. Install it or point RADIUM_RUNTIME at a supported runtime",
                    self.kind.executable()
                )
            })?;

        if !output.status.success() {
            bail!(
                "`{} --version` exited with {}",
                self.kind.executable(),
                output.status
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Run a package.json script, including its `pre`/`post` companions.
    ///
    /// Scripts execute through the platform shell, matching npm's behavior, and
    /// `node_modules/.bin` is prepended to `PATH`.
    pub fn run_script(
        &self,
        root: &Path,
        package: &PackageJson,
        name: &str,
        args: &[String],
    ) -> Result<u8> {
        if !package.scripts.contains_key(name) {
            let available = if package.scripts.is_empty() {
                "none".to_string()
            } else {
                package
                    .scripts
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            };

            bail!("Missing script: {name}. Available scripts: {available}");
        }

        let lifecycle = [
            (format!("pre{name}"), &[][..]),
            (name.to_string(), args),
            (format!("post{name}"), &[][..]),
        ];

        for (event, event_args) in lifecycle {
            if let Some(script) = package.scripts.get(&event) {
                let code = self.execute_script(root, package, &event, script, event_args)?;
                if code != 0 {
                    return Ok(code);
                }
            }
        }

        Ok(0)
    }

    /// Run a `.ts`, `.js`, or `.mjs` file directly with the runtime.
    pub fn run_file(
        &self,
        root: &Path,
        package: &PackageJson,
        file: &str,
        args: &[String],
    ) -> Result<u8> {
        let requested = Path::new(file);
        let resolved = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            root.join(requested)
        };

        if !resolved.is_file() {
            bail!("File not found: {}", resolved.display());
        }

        let mut command = Command::new(self.kind.executable());
        command.arg(&resolved).args(args);
        self.configure(&mut command, root, package)?;

        let status = command
            .status()
            .with_context(|| format!("Failed to run {}", resolved.display()))?;

        Ok(exit_code(status))
    }

    /// Run an executable installed inside a package tree.
    ///
    /// `install_root` is the directory whose `node_modules` contains the
    /// package, while `working_dir` is where the process should run (normally
    /// the user's current directory, so initializers scaffold in place).
    /// JavaScript bins run through the configured runtime, which avoids relying
    /// on executable bits in the installed archive.
    pub fn run_package_bin(
        &self,
        working_dir: &Path,
        install_root: &Path,
        package: &PackageJson,
        bin: &Path,
        args: &[String],
    ) -> Result<u8> {
        let mut command = if is_javascript_file(bin) {
            let mut command = Command::new(self.kind.executable());
            command.arg(bin);
            command
        } else {
            Command::new(bin)
        };

        command.args(args);
        self.configure_for(
            &mut command,
            working_dir,
            Some(package),
            &[install_root.join("node_modules").join(".bin")],
        )?;

        let status = command
            .status()
            .with_context(|| format!("Failed to run {}", bin.display()))?;

        Ok(exit_code(status))
    }

    fn execute_script(
        &self,
        root: &Path,
        package: &PackageJson,
        event: &str,
        script: &str,
        args: &[String],
    ) -> Result<u8> {
        let mut command = shell_command();
        command
            .arg(command_line(script, args))
            .env("npm_lifecycle_event", event)
            .env("npm_lifecycle_script", script);

        self.configure(&mut command, root, package)?;

        let status = command.status().with_context(|| {
            format!("Failed to run `{event}` with `{}`", self.kind.executable())
        })?;

        Ok(exit_code(status))
    }

    fn configure(&self, command: &mut Command, root: &Path, package: &PackageJson) -> Result<()> {
        self.configure_for(command, root, Some(package), &[])
    }

    /// Configure a child process with the runtime environment and a `PATH` that
    /// includes the project's and any extra `node_modules/.bin` directories.
    fn configure_for(
        &self,
        command: &mut Command,
        directory: &Path,
        package: Option<&PackageJson>,
        extra_paths: &[PathBuf],
    ) -> Result<()> {
        command
            .current_dir(directory)
            .env("PATH", build_path(directory, extra_paths)?)
            .env("RADIUM_RUNTIME", self.kind.name());

        if let Some(package) = package {
            command
                .env(
                    "npm_package_name",
                    package.name.as_deref().unwrap_or_default(),
                )
                .env(
                    "npm_package_version",
                    package.version.as_deref().unwrap_or_default(),
                );
        }

        Ok(())
    }
}

/// Whether a `radium run` target should be treated as a source file rather
/// than a package.json script name.
pub fn is_runnable_file(target: &str) -> bool {
    matches!(
        Path::new(target).extension().and_then(OsStr::to_str),
        Some("ts" | "js" | "mjs")
    )
}

fn exit_code(status: ExitStatus) -> u8 {
    u8::try_from(status.code().unwrap_or(1)).unwrap_or(1)
}

fn shell_command() -> Command {
    if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.arg("/C");
        command
    } else {
        let mut command = Command::new("sh");
        command.arg("-c");
        command
    }
}

/// Whether `path` is a JavaScript or TypeScript file that should be handed to
/// the runtime instead of being executed directly.
fn is_javascript_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str),
        Some("js" | "mjs" | "cjs" | "ts" | "mts" | "cts")
    )
}

fn build_path(root: &Path, extra_paths: &[PathBuf]) -> Result<OsString> {
    let mut entries = vec![root.join("node_modules").join(".bin")];
    entries.extend(extra_paths.iter().cloned());

    if let Some(path) = env::var_os("PATH") {
        entries.extend(env::split_paths(&path));
    }

    env::join_paths(entries).context("Failed to construct PATH for script")
}

fn command_line(script: &str, args: &[String]) -> String {
    if args.is_empty() {
        return script.to_string();
    }

    let args = args
        .iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");

    format!("{script} {args}")
}

#[cfg(not(windows))]
fn shell_quote(argument: &str) -> String {
    let safe = !argument.is_empty()
        && argument
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_./:@%+=,".contains(character));

    if safe {
        argument.to_string()
    } else {
        format!("'{}'", argument.replace('\'', "'\\''"))
    }
}

#[cfg(windows)]
fn shell_quote(argument: &str) -> String {
    // cmd.exe quoting is limited; double quotes handle the common cases.
    format!("\"{}\"", argument.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_arguments_to_scripts() {
        assert_eq!(command_line("node test.js", &[]), "node test.js");
        assert_eq!(
            command_line("node test.js", &["--watch".to_string()]),
            "node test.js --watch"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn quotes_unsafe_arguments() {
        assert_eq!(shell_quote("plain"), "plain");
        assert_eq!(shell_quote("two words"), "'two words'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn runtime_names_are_stable() {
        assert_eq!(RuntimeKind::Node.name(), "node");
        assert_eq!(RuntimeKind::Node.executable(), "node");
    }

    #[test]
    fn detects_runnable_files() {
        for file in ["index.ts", "src/index.js", "index.mjs", "/tmp/a.js"] {
            assert!(is_runnable_file(file), "{file} should be a file target");
        }

        for target in ["dev", "build", "index.tsx", "index.cjs", "index"] {
            assert!(
                !is_runnable_file(target),
                "{target} should be a script target"
            );
        }
    }
}
