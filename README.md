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
- `init <template>` / `create <folder> --template <template>` — scaffold with an npm initializer
  such as `next`, `vite`, or `create-*`
- `x` / `exec` / `radix` — run a package executable without installing it into the project
- `add` — resolve and install a package from the npm registry
- Recursive install of a package's runtime dependencies
- npm-style semver resolution: dist-tags, exact versions, `^`/`~` ranges, `>=` comparators,
  hyphen ranges, `x` wildcards, and `||` alternatives
- SHA-512/SHA-256/SHA-1 integrity verification for downloaded tarballs
- `install` — restore every dependency at the exact versions in `radium.lock` without re-resolving
- `cache` — manage the shared content-addressed package cache
- `migrate` — convert `package-lock.json`, `deno.lock`, or `bun.lock`/`bun.lockb` into
  `radium.lock`
- `run` — execute `package.json` scripts or `.ts`/`.js`/`.mjs` files with Node.js, including
  `pre`/`post` script hooks and `node_modules/.bin` on `PATH`
- `doctor` — report the active runtime, project metadata, and lockfile status
- Gzip/tar extraction of npm tarballs into `node_modules`
- Deterministic, sorted lockfiles that include transitive packages
- A `packageManager` field in generated projects that identifies Radium

## Requirements

- Rust 1.88 or newer (edition 2024)
- Node.js 18 or newer for `radium run`, `radium doctor`, and package executables
- Node.js 22.18 or newer for running `.ts` files without extra flags (erasable TypeScript only)
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

This also installs `radix`, a shortcut for `radium x`.

## Usage

### Create a new project

```bash
radium new my-app --author "Jane Doe"
```

Prompts for a project name when omitted, then writes `package.json`, `radium.lock`, and an
`index.ts` starter file inside `my-app/`. The author is prompted for when `--author` is omitted.
The generated `package.json` includes `"packageManager": "radium"`.

### Initialize an existing directory

```bash
cd my-app
radium init
```

Uses the current directory name as the project name and prompts for the author. Radium refuses
to overwrite an existing `package.json`; remove or rename it first.

### Scaffold from a template

```bash
radium init next
radium create my-app --template vite
```

Templates run the matching npm initializer for you. `radium init <template>` scaffolds into the
current directory, while `radium create <folder> --template <template>` scaffolds into
`<folder>`. The initializer is installed in a scratch directory, so it is not added to the
project's dependencies.

Built-in templates are `next`, `vite`, `vue`, `svelte`, `astro`, `nuxt`, and `react`. Any other
name resolves to the conventional `create-<name>@latest` package; for example,
`radium create my-app --template solid` runs `create-solid`.

### Install a package

```bash
radium add typescript
```

Downloads the package's latest release from npm, extracts it into `node_modules/typescript`,
installs its dependencies recursively, and records the resolved version in `package.json` and
`radium.lock`. If the current directory is not a Radium project, you'll be asked whether to
initialize one first.

### Install from the lockfile

```bash
radium install
```

Restores `node_modules` from the exact versions recorded in `radium.lock`. Radium removes the
existing `node_modules` first, then installs every direct dependency and its transitive
dependencies at the locked versions. Version ranges are not re-resolved, and `radium.lock` is
never modified; a dependency that is missing from the lockfile is an error. Run `radium add` or
`radium upgrade` when you intentionally want to update the lockfile.

### Upgrade everything

```bash
radium upgrade
```

Removes `node_modules`, reinstalls every dependency at its latest version, and refreshes the
lockfile.

### Manage the package cache

Radium caches registry metadata and package tarballs so repeat installs do not download the same
files again. The cache is shared across projects and stored in the platform cache directory:

| Platform | Default location |
| --- | --- |
| Linux | `~/.cache/radium/` (or `$XDG_CACHE_HOME/radium/`) |
| macOS | `~/Library/Caches/radium/` |
| Windows | `%LOCALAPPDATA%\radium\Cache\` |

Tarballs are content-addressed by SHA-256 under `packages/sha256/<prefix>/<hash>`. Metadata is
stored under `metadata/registry/` and `metadata/packages/`, and per-package tarball indexes live
under `index/`.

```bash
radium cache clear            # delete the entire cache
radium cache clear typescript # delete only typescript's cached metadata and tarballs
```

Clearing one package leaves tarballs that are still referenced by another cached package in
place. Set `RADIUM_CACHE_DIR` to use a different cache location, for example in CI or for a
project-local cache.

### Migrate from another package manager

```bash
radium migrate node   # package-lock.json (`npm` is accepted as an alias)
radium migrate deno   # deno.lock
radium migrate bun    # bun.lock or bun.lockb
```

Reads the source lockfile, collapses it into Radium's flat dependency tree, writes `radium.lock`,
and marks `package.json` with `"packageManager": "radium"`. An existing `radium.lock` is protected;
pass `--force` to overwrite it.

For npm v2/v3 lockfiles the shallowest version of each package wins; Deno and Bun migrations keep
the highest resolved version. Non-registry specs such as `file:`, `workspace:`, and `github:` are
skipped. Binary `bun.lockb` files are converted with Bun itself, so Bun must be installed:

```bash
bun install --save-text-lockfile --frozen-lockfile --lockfile-only
```

Radium runs that command for you, removes the generated `bun.lock`, and leaves the original
`bun.lockb` untouched. Set `RADIUM_BUN` to use a specific Bun executable.

### Run a script or file

Add a script to `package.json`:

```json
{
  "scripts": {
    "dev": "node server.js",
    "greet": "node -e \"console.log('hello ' + process.argv[1])\""
  }
}
```

Then run it:

```bash
radium run dev
radium run greet world
```

`radium run` can also execute a source file directly. If the target ends in `.ts`, `.js`, or
`.mjs`, Radium treats it as a file relative to the project root and forwards any extra
arguments to the runtime:

```bash
radium run index.ts
radium run ./scripts/build.js --watch
radium run hello.mjs world
```

Otherwise the target is looked up as a `package.json` script. Missing scripts produce an error
that lists the scripts that are available. TypeScript files rely on Node's native type
stripping, so only erasable TypeScript syntax is supported (no enums, namespaces, or parameter
properties without a build step).

Scripts execute through the platform shell (`sh -c` or `cmd /C`) with `node_modules/.bin`
prepended to `PATH`. `pre<script>` and `post<script>` hooks run automatically, and npm-style
environment variables such as `npm_lifecycle_event`, `npm_package_name`, and
`npm_package_version` are provided. File targets run directly without a shell, with
`node_modules/.bin` still available on `PATH`.

### Run a package executable

```bash
radium x typescript --version
radium exec create-next-app@latest my-app
radix next-app@latest .
```

`x`, `exec`, and the `radix` binary run an executable from an npm package without adding it to
the project. Radium resolves the package spec, installs it into a scratch directory (reusing the
shared cache when possible), and forwards the remaining arguments to the package's `bin` entry.
The process runs in your current directory, so initializers scaffold in place.

The package spec accepts a plain name (`typescript`), a version or tag (`typescript@5.7.2`,
`typescript@next`), or a scoped name (`@scope/tool`). `next-app` is shorthand for
`create-next-app`. Because the installation is temporary, `x` and `radix` never modify the
project's `node_modules`, `package.json`, or `radium.lock`.

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
| `RADIUM_CACHE_DIR` | platform cache directory | Override the shared package cache location |
| `RADIUM_RUNTIME` | `node` | JavaScript runtime used by `radium run`, `radix`, and `doctor` |
| `RADIUM_BUN` | `bun` | Bun executable used by `radium migrate bun` to convert `bun.lockb` |

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
│   ├── main.rs                 # Thin `radium` binary entry point
│   ├── lib.rs                  # Shared library entry points for `radium` and `radix`
│   ├── bin/radix.rs            # Thin `radix` binary entry point
│   ├── cli.rs                  # clap command definitions
│   ├── commands/               # init, new, create, add, install, upgrade, migrate, cache, run, x, doctor
│   ├── cache.rs                # Content-addressed tarball and metadata cache
│   ├── hash.rs                 # SHA-256 helpers
│   ├── installer.rs            # Dependency resolution, download, extraction, locked installs
│   ├── lockfile.rs             # radium.lock model and persistence
│   ├── migrate.rs              # npm, Deno, and Bun lockfile conversion
│   ├── project.rs              # package.json model and scaffolding
│   ├── registry.rs             # npm registry client, cache-aware metadata, semver resolution
│   ├── runtime.rs              # Pluggable JavaScript runtime (Node today)
│   ├── spec.rs                 # npx-style package spec parsing
│   ├── templates.rs            # Built-in template to initializer mapping
│   └── ui.rs                   # Prompts and user-facing output
├── tests/cli.rs                # End-to-end CLI tests, including Node integration
├── resources/templates/        # Scaffolding templates
└── test-project/               # Local scratch project for manual testing
```

## How it works

1. `radium add <name>` fetches `https://registry.npmjs.org/<name>` and caches the registry
   document under the shared cache directory.
2. The requested version is resolved from dist-tags, an exact version, or an npm semver range.
   `radium install` skips this step and uses the exact version from `radium.lock` instead.
3. The tarball is downloaded (or read from the content-addressed cache) and checked against the
   registry's integrity metadata.
4. The tarball is gunzipped and unpacked into a temporary directory beside the destination.
5. The contents of the archive's `package/` folder are moved into `node_modules/<name>`.
6. Dependencies listed in the package metadata are installed with the same routine; the flat
   tree reuses an already-resolved version to avoid cycles. `radium install` requires every
   transitive dependency to have a lockfile entry.
7. Resolved versions are written to `package.json` (direct dependencies) and `radium.lock`
   (direct and transitive packages). `radium install` never changes either file apart from
   marking `packageManager`.

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
- Package `bin` entries are not linked into `node_modules/.bin` yet, so scripts should invoke
  tools through `node` directly. The `x`, `exec`, and `radix` commands can still run package
  executables from a scratch installation.
- There is no `remove` or install-time lifecycle script support yet.
- Template names map to `create-*` initializers by convention; there is no template registry.
- Git, file, tarball, workspace, and npm alias version specs are not supported.
- Only the Node.js runtime is implemented for `radium run`; Deno and Bun adapters are planned.
- Downloads are sequential; the shared cache avoids repeat downloads but there is no concurrency.

See [TODO.md](TODO.md) for the planned work.

## License

No license has been specified for this project yet.
