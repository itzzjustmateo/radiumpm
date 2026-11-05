# Radium

A small, experimental package manager for JavaScript/TypeScript projects, written in Rust.

Radium installs packages straight from the public npm registry into a local `node_modules`
directory. It keeps its own lockfile (`radium.lock`) and manages dependencies from a familiar
`package.json`, so it can be dropped into a project that already uses the npm ecosystem.

It can also execute `package.json` scripts through a JavaScript runtime. Node.js is supported
today, with a runtime abstraction designed so Deno and Bun can be added later.

> **Status:** early prototype. Radium implements a deliberately small subset of npm behavior
> and is not a drop-in replacement for npm, pnpm, or yarn yet.

## Features

- `init` / `new` — scaffold a project with `package.json`, `radium.lock`, and `index.ts`
- `add` — resolve and install a package from the npm registry
- Recursive install of a package's runtime dependencies
- npm-style semver resolution: dist-tags, exact versions, `^`/`~` ranges, `>=` comparators,
  hyphen ranges, `x` wildcards, and `||` alternatives
- SHA-512/SHA-256/SHA-1 integrity verification for downloaded tarballs
- `upgrade` — re-resolve every dependency in `package.json` to the latest version
- `run` — execute `package.json` scripts with Node.js, including `pre`/`post` hooks and
  `node_modules/.bin` on `PATH`
- `doctor` — report the active runtime, project metadata, and lockfile status
- Gzip/tar extraction of npm tarballs into `node_modules`
- Deterministic, sorted lockfiles that include transitive packages

## Requirements

- Rust 1.88 or newer (edition 2024)
- Node.js 18 or newer for `radium run` and `radium doctor`
- Network access to `https://registry.npmjs.org` (or a configured mirror)

## Installation

Build from source:

```bash
git clone <repository-url>
cd radiumpm
cargo build --release
```

The binary is produced at `target/release/radium`. To put it on your `PATH`:

```bash
cargo install --path .
```

## Usage

### Create a new project

```bash
radium new my-app --author "Jane Doe"
```

Prompts for a project name when omitted, then writes `package.json`, `radium.lock`, and an
`index.ts` starter file inside `my-app/`. The author is prompted for when `--author` is omitted.

### Initialize an existing directory

```bash
cd my-app
radium init
```

Uses the current directory name as the project name and prompts for the author. Radium refuses
to overwrite an existing `package.json`; remove or rename it first.

### Install a package

```bash
radium add typescript
```

Downloads the package's latest release from npm, extracts it into `node_modules/typescript`,
installs its dependencies recursively, and records the resolved version in `package.json` and
`radium.lock`. If the current directory is not a Radium project, you'll be asked whether to
initialize one first.

### Upgrade everything

```bash
radium upgrade
```

Removes `node_modules`, reinstalls every dependency at its latest version, and refreshes the
lockfile.

### Run a script

Add a script to `package.json`:

```json
{
  "scripts": {
    "build": "tsc",
    "greet": "node -e \"console.log('hello ' + process.argv[1])\""
  }
}
```

Then run it:

```bash
radium run build
radium run greet world
```

Scripts execute through the platform shell (`sh -c` or `cmd /C`) with `node_modules/.bin`
prepended to `PATH`. `pre<script>` and `post<script>` hooks run automatically, and npm-style
environment variables such as `npm_lifecycle_event`, `npm_package_name`, and
`npm_package_version` are provided.

### Check your environment

```bash
radium doctor
```

Prints the Radium version, the detected JavaScript runtime and version, the project name, and
the lockfile status. Exits non-zero when the runtime is missing or a project file is invalid.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `RADIUM_REGISTRY` | `https://registry.npmjs.org` | Registry base URL, useful for mirrors |
| `RADIUM_RUNTIME` | `node` | JavaScript runtime used by `radium run` and `doctor` |

`RADIUM_RUNTIME` currently accepts `node`/`nodejs`. `deno` and `bun` are recognized and
rejected with an explanatory error until their adapters are implemented.

## Project layout

```text
.
├── Cargo.toml                  # Rust crate and lint configuration
├── rust-toolchain.toml         # Pinned toolchain components (clippy, rustfmt)
├── rustfmt.toml                # Formatting rules
├── clippy.toml                 # Clippy MSRV
├── justfile                    # Common development tasks
├── .cargo/config.toml          # Cargo aliases
├── .github/workflows/          # CI and release automation
├── src/
│   ├── main.rs                 # Thin CLI entry point
│   ├── cli.rs                  # clap command definitions
│   ├── commands/               # init, new, add, upgrade, run, doctor
│   ├── installer.rs            # Dependency resolution, download, extraction
│   ├── lockfile.rs             # radium.lock model and persistence
│   ├── project.rs              # package.json model and scaffolding
│   ├── registry.rs             # npm registry client and semver resolution
│   ├── runtime.rs              # Pluggable JavaScript runtime (Node today)
│   └── ui.rs                   # Prompts and user-facing output
├── tests/cli.rs                # End-to-end CLI tests, including Node integration
├── resources/templates/        # Scaffolding templates
└── node-project/               # Example project used for manual testing
```

## How it works

1. `radium add <name>` fetches `https://registry.npmjs.org/<name>`.
2. The requested version is resolved from dist-tags, an exact version, or an npm semver range.
3. The tarball is downloaded and checked against the registry's integrity metadata.
4. The tarball is gunzipped and unpacked into a temporary directory beside the destination.
5. The contents of the archive's `package/` folder are moved into `node_modules/<name>`.
6. Dependencies listed in the package metadata are installed with the same routine; the flat
   tree reuses an already-resolved version to avoid cycles.
7. Resolved versions are written to `package.json` (direct dependencies) and `radium.lock`
   (direct and transitive packages).

## Lockfile format

`radium.lock` is deterministic JSON:

```json
{
  "name": "my-app",
  "lockfileVersion": 1,
  "packages": {
    "esbuild": {
      "version": "0.25.0"
    },
    "typescript": {
      "version": "7.0.2"
    }
  }
}
```

## Development

The repository pins the stable Rust toolchain and installs `clippy` and `rustfmt` automatically.

```bash
just check        # format check + clippy + tests
just fmt          # apply formatting
just lint         # clippy with warnings denied
just test         # test suite
just build        # optimized binary
```

If `just` is unavailable, use the Cargo aliases in `.cargo/config.toml`:

```bash
cargo fmt-check
cargo lint
cargo test-all
```

CI runs formatting, Clippy, tests on Linux/macOS/Windows with Node 22, an MSRV check, and a
release build on version tags.

## Limitations

This is a prototype, and the following are intentional simplifications:

- The dependency tree is flat — there is no nested `node_modules` resolution or deduplication.
- `devDependencies` are not installed.
- There is no `install`-from-lockfile, `remove`, or install-time lifecycle script support yet.
- Git, file, tarball, workspace, and npm alias version specs are not supported.
- Only the Node.js runtime is implemented for `radium run`; Deno and Bun adapters are planned.
- Downloads are sequential with no shared cache or concurrency.

See [TODO.md](TODO.md) for the planned work.

## License

No license has been specified for this project yet.
