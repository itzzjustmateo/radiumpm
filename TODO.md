# TODO

A running list of improvements for Radium. The project is an early prototype, so items are
ordered roughly by usefulness rather than difficulty.

## JavaScript runtime

- [x] Detect Node.js and use it to execute `package.json` scripts
- [x] Run `pre<script>` / `post<script>` hooks and expose npm-style lifecycle variables
- [x] Add `node_modules/.bin` to `PATH` for scripts
- [x] Add `radium doctor` to report the active runtime and project health
- [x] Introduce a `RuntimeKind` abstraction selected by `RADIUM_RUNTIME`
- [ ] **Make the JavaScript runtime fully pluggable so Node can be swapped for Deno or Bun later.**
  - [ ] Add `RuntimeKind::Deno` and `RuntimeKind::Bun` implementations behind the same trait
  - [ ] Map script execution to each runtime: Node (shell + `node_modules/.bin`), Bun (`bun run`),
        Deno (`deno task`, or a generated task mapping)
  - [ ] Teach `doctor` to report runtime-specific details and capability differences
  - [ ] Add runtime configuration to `package.json` or a dedicated Radium config file
  - [ ] Keep Node.js as the default and preserve current behavior when `RADIUM_RUNTIME` is unset
  - [ ] Run the runtime integration tests against Node, Deno, and Bun in CI
  - [ ] Document migration and compatibility notes for each runtime

## Package management

- [ ] Read `radium.lock` during install so builds are reproducible without re-resolving
- [ ] Install `devDependencies` with a `--save-dev` / `--production` distinction
- [ ] Support `radium remove <package>`
- [ ] Support `radium install` to restore an entire project from `package.json` + lockfile
- [ ] Support explicit version specs such as `radium add typescript@5`
- [ ] Support scoped packages end-to-end in the lockfile and install output
- [ ] Add nested `node_modules` resolution and deduplication
- [ ] Add a download cache shared across projects
- [ ] Parallelize package downloads and extraction
- [ ] Record tarball URLs and integrity hashes in the lockfile
- [ ] Verify installed package contents and detect lockfile drift
- [ ] Run install-time lifecycle scripts (`preinstall`, `install`, `postinstall`) with an opt-out
- [ ] Support `packageManager` and `engines` fields
- [ ] Handle optional dependencies and platform-specific packages

## Code quality

- [ ] Introduce a library target so integration tests can exercise internals directly
- [ ] Add property-based tests for npm semver range normalization
- [ ] Add a test registry fixture (or recorded HTTP fixtures) for offline install tests
- [ ] Replace the remaining stringly-typed JSON access in package metadata with typed models
- [ ] Improve progress output with a proper progress bar or structured logging
- [ ] Add `tracing` or `log` support with a `--verbose` flag
- [ ] Audit error messages so every failure suggests a next step
- [ ] Add benchmarks for registry resolution, extraction, and lockfile serialization

## Tooling, CI, and releases

- [x] Add `rust-toolchain.toml`, `rustfmt.toml`, and `clippy.toml`
- [x] Add Cargo aliases and a `justfile` for common tasks
- [x] Add CI for formatting, Clippy, tests, MSRV, and release builds
- [x] Add a tag-triggered release workflow for Linux, macOS, and Windows
- [x] Add Dependabot for Cargo and GitHub Actions
- [ ] Add `cargo-deny` for license, advisory, and duplicate-dependency checks
- [ ] Add coverage reporting (for example, `cargo-llvm-cov` + Codecov)
- [ ] Add a cross-compiled Linux ARM64 release artifact
- [ ] Add a Homebrew tap and/or install script
- [ ] Add ShellCheck/actionlint validation for workflow and script changes
- [ ] Add a `pre-commit` hook or `cargo-husky` configuration
- [ ] Publish the binary to crates.io once the package is ready

## Documentation and community

- [ ] Add a LICENSE and update the README
- [ ] Add `CONTRIBUTING.md` with architecture notes and review expectations
- [ ] Add a `CHANGELOG.md` and release process documentation
- [ ] Add a compatibility matrix against npm/pnpm/yarn behavior
- [ ] Add issue and pull-request templates
- [ ] Add a design document for the runtime abstraction and lockfile format versioning
- [ ] Document security expectations for tarball extraction and integrity verification
