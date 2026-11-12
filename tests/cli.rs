use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn radium(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_radium"));
    command.current_dir(directory);
    command
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn make_tarball() -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    let contents = br#"{"name":"demo","version":"1.0.0"}"#;
    let mut header = tar::Header::new_gnu();
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "package/package.json", &contents[..])
        .expect("append archive entry");

    builder
        .into_inner()
        .expect("finish archive")
        .finish()
        .expect("finish gzip")
}

fn write_cached_package(cache: &Path, name: &str, version: &str, tarball: &[u8]) -> String {
    let encoded = name.replace('/', "%2F");
    let hash = sha256_hex(tarball);

    let tarball_dir = cache.join("packages").join("sha256").join(&hash[..2]);
    fs::create_dir_all(&tarball_dir).expect("create tarball dir");
    fs::write(tarball_dir.join(&hash), tarball).expect("write cached tarball");

    let metadata_dir = cache.join("metadata").join("packages");
    fs::create_dir_all(&metadata_dir).expect("create metadata dir");
    fs::write(
        metadata_dir.join(format!("{encoded}.json")),
        format!(
            r#"{{"name":"{name}","versions":{{"{version}":{{"version":"{version}","dist":{{"tarball":"http://127.0.0.1:1/{name}.tgz"}},"dependencies":{{}}}}}}}}"#
        ),
    )
    .expect("write package metadata");

    let index_dir = cache.join("index");
    fs::create_dir_all(&index_dir).expect("create index dir");
    fs::write(
        index_dir.join(format!("{encoded}.json")),
        format!(r#"{{"name":"{name}","tarballs":{{"{version}":"{hash}"}}}}"#),
    )
    .expect("write package index");

    hash
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn help_lists_the_run_command() {
    let directory = tempfile::tempdir().expect("temp dir");
    let output = radium(directory.path())
        .arg("--help")
        .output()
        .expect("run radium");

    assert_success(&output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("doctor"));
    assert!(stdout.contains("run"));
}

#[test]
fn new_scaffolds_a_project() {
    let directory = tempfile::tempdir().expect("temp dir");
    let output = radium(directory.path())
        .args(["new", "demo", "--author", "Jane Doe"])
        .output()
        .expect("run radium new");

    assert_success(&output);

    let project = directory.path().join("demo");
    assert!(project.join("package.json").is_file());
    assert!(project.join("radium.lock").is_file());
    assert!(project.join("index.ts").is_file());

    let package = fs::read_to_string(project.join("package.json")).expect("read package.json");
    assert!(package.contains("\"name\": \"demo\""));
    assert!(package.contains("\"packageManager\": \"radium\""));
}

#[test]
fn run_executes_scripts_with_node() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping Node integration test: node is not installed");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");

    fs::write(directory.path().join("pre.js"), "console.log('pre');\n").expect("write pre.js");
    fs::write(
        directory.path().join("main.js"),
        "console.log('main ' + process.argv[2]);\n",
    )
    .expect("write main.js");
    fs::write(directory.path().join("post.js"), "console.log('post');\n").expect("write post.js");

    fs::write(
        directory.path().join("package.json"),
        r#"{
            "name": "runtime-test",
            "version": "1.0.0",
            "scripts": {
                "prebuild": "node pre.js",
                "build": "node main.js",
                "postbuild": "node post.js"
            }
        }"#,
    )
    .expect("write package.json");

    let output = radium(directory.path())
        .args(["run", "build", "world"])
        .output()
        .expect("run radium run");

    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("pre"));
    assert!(stdout.contains("main world"));
    assert!(stdout.contains("post"));

    let pre = stdout.find("pre").expect("pre output");
    let main = stdout.find("main world").expect("main output");
    let post = stdout.find("post").expect("post output");
    assert!(pre < main && main < post);
}

#[test]
fn doctor_reports_the_runtime() {
    let directory = tempfile::tempdir().expect("temp dir");
    let output = radium(directory.path())
        .arg("doctor")
        .output()
        .expect("run radium doctor");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("radium"));
    assert!(stdout.contains("JavaScript runtime") || stdout.contains("runtime unavailable"));
}

#[test]
fn rejects_unimplemented_runtimes() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"runtime-test","scripts":{"hello":"node -e \"\""}}"#,
    )
    .expect("write package.json");

    let output = radium(directory.path())
        .env("RADIUM_RUNTIME", "deno")
        .args(["run", "hello"])
        .output()
        .expect("run radium run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Deno support is planned"));
}

#[test]
fn run_executes_source_files() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping Node integration test: node is not installed");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"file-test","scripts":{}}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("hello.js"),
        "console.log('js ' + process.argv[2]);\n",
    )
    .expect("write hello.js");
    fs::write(directory.path().join("hello.mjs"), "console.log('mjs');\n")
        .expect("write hello.mjs");

    let js = radium(directory.path())
        .args(["run", "./hello.js", "world"])
        .output()
        .expect("run js file");
    assert_success(&js);
    assert!(String::from_utf8_lossy(&js.stdout).contains("js world"));

    let mjs = radium(directory.path())
        .args(["run", "hello.mjs"])
        .output()
        .expect("run mjs file");
    assert_success(&mjs);
    assert!(String::from_utf8_lossy(&mjs.stdout).contains("mjs"));
}

#[test]
fn run_executes_typescript_files_when_supported() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping TypeScript integration test: node is not installed");
        return;
    }

    let directory = tempfile::tempdir().expect("temp dir");
    let probe = directory.path().join("probe.ts");
    fs::write(&probe, "const value: string = 'ok';\nconsole.log(value);\n").expect("write probe");

    let supports_typescript = Command::new("node")
        .arg(&probe)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);

    if !supports_typescript {
        eprintln!("skipping TypeScript integration test: node cannot strip types");
        return;
    }

    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"typescript-test","scripts":{}}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("hello.ts"),
        "const message: string = 'ts works';\nconsole.log(message);\n",
    )
    .expect("write hello.ts");

    let output = radium(directory.path())
        .args(["run", "hello.ts"])
        .output()
        .expect("run ts file");

    assert_success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("ts works"));
}

#[test]
fn run_errors_when_script_is_missing() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"missing-script","scripts":{}}"#,
    )
    .expect("write package.json");

    let output = radium(directory.path())
        .args(["run", "dev"])
        .output()
        .expect("run radium run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Missing script: dev"));
}

#[test]
fn run_errors_when_file_is_missing() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"missing-file","scripts":{}}"#,
    )
    .expect("write package.json");

    let output = radium(directory.path())
        .args(["run", "missing.js"])
        .output()
        .expect("run radium run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("File not found"));
}

#[test]
fn migrate_node_converts_package_lock() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"migrate-node"}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("package-lock.json"),
        r#"{
            "name": "migrate-node",
            "lockfileVersion": 3,
            "packages": {
                "": { "name": "migrate-node", "version": "1.0.0" },
                "node_modules/debug": { "version": "4.4.1" },
                "node_modules/ms": { "version": "2.1.3" }
            }
        }"#,
    )
    .expect("write package-lock.json");

    let output = radium(directory.path())
        .args(["migrate", "node"])
        .output()
        .expect("run radium migrate");

    assert_success(&output);

    let lockfile: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(directory.path().join("radium.lock")).expect("read radium.lock"),
    )
    .expect("parse radium.lock");

    assert_eq!(lockfile["name"], "migrate-node");
    assert_eq!(lockfile["packages"]["debug"]["version"], "4.4.1");
    assert_eq!(lockfile["packages"]["ms"]["version"], "2.1.3");

    let package =
        fs::read_to_string(directory.path().join("package.json")).expect("read package.json");
    assert!(package.contains("\"packageManager\": \"radium\""));
}

#[test]
fn migrate_deno_converts_deno_lock() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"migrate-deno"}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("deno.lock"),
        r#"{
            "version": "5",
            "specifiers": { "npm:chalk@^5.6.2": "5.6.2" },
            "npm": {
                "chalk@5.6.2": { "integrity": "sha512-..." },
                "ansi-styles@4.3.0": { "integrity": "sha512-..." }
            }
        }"#,
    )
    .expect("write deno.lock");

    let output = radium(directory.path())
        .args(["migrate", "deno"])
        .output()
        .expect("run radium migrate");

    assert_success(&output);

    let lockfile: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(directory.path().join("radium.lock")).expect("read radium.lock"),
    )
    .expect("parse radium.lock");

    assert_eq!(lockfile["packages"]["chalk"]["version"], "5.6.2");
    assert_eq!(lockfile["packages"]["ansi-styles"]["version"], "4.3.0");
}

#[test]
fn migrate_bun_converts_text_lockfile() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"migrate-bun"}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("bun.lock"),
        r#"{
            // Bun lockfiles are JSONC.
            "lockfileVersion": 1,
            "packages": {
                "react": ["react@18.3.1", "", {}, "sha512-..."],
                "bun-types": ["bun-types@workspace:packages/bun-types"],
            },
        }"#,
    )
    .expect("write bun.lock");

    let output = radium(directory.path())
        .args(["migrate", "bun"])
        .output()
        .expect("run radium migrate");

    assert_success(&output);

    let lockfile: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(directory.path().join("radium.lock")).expect("read radium.lock"),
    )
    .expect("parse radium.lock");

    assert_eq!(lockfile["packages"]["react"]["version"], "18.3.1");
    assert!(lockfile["packages"].get("bun-types").is_none());
}

#[test]
fn migrate_refuses_to_overwrite_without_force() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"migrate-force"}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("package-lock.json"),
        r#"{
            "name": "migrate-force",
            "lockfileVersion": 3,
            "packages": { "node_modules/debug": { "version": "4.4.1" } }
        }"#,
    )
    .expect("write package-lock.json");
    fs::write(directory.path().join("radium.lock"), "existing\n").expect("write radium.lock");

    let output = radium(directory.path())
        .args(["migrate", "node"])
        .output()
        .expect("run radium migrate");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force"));
    assert_eq!(
        fs::read_to_string(directory.path().join("radium.lock")).expect("read radium.lock"),
        "existing\n"
    );

    let output = radium(directory.path())
        .args(["migrate", "node", "--force"])
        .output()
        .expect("run radium migrate --force");

    assert_success(&output);
    assert!(
        fs::read_to_string(directory.path().join("radium.lock"))
            .expect("read radium.lock")
            .contains("\"debug\"")
    );
}

#[test]
fn migrate_bun_requires_bun_for_binary_lockfile() {
    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"migrate-bun-binary"}"#,
    )
    .expect("write package.json");
    fs::write(directory.path().join("bun.lockb"), b"not a real lockfile").expect("write bun.lockb");

    let missing_bun = directory.path().join("missing-bun");
    let output = radium(directory.path())
        .env("RADIUM_BUN", &missing_bun)
        .args(["migrate", "bun"])
        .output()
        .expect("run radium migrate");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Bun is required"));
    assert!(!directory.path().join("bun.lock").exists());
}

#[cfg(unix)]
#[test]
fn migrate_bun_converts_binary_lockfile() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().expect("temp dir");
    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"migrate-bun-binary"}"#,
    )
    .expect("write package.json");
    fs::write(directory.path().join("bun.lockb"), b"binary lockfile").expect("write bun.lockb");

    let script = directory.path().join("fake-bun.sh");
    fs::write(
        &script,
        "#!/bin/sh\nrm -f bun.lockb\ncat > bun.lock <<'EOF'\n\
         { \"packages\": { \"react\": [\"react@18.3.1\"] } }\nEOF\n",
    )
    .expect("write fake bun");

    let mut permissions = fs::metadata(&script)
        .expect("script metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&script, permissions).expect("make script executable");

    let output = radium(directory.path())
        .env("RADIUM_BUN", &script)
        .args(["migrate", "bun"])
        .output()
        .expect("run radium migrate");

    assert_success(&output);

    let lockfile: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(directory.path().join("radium.lock")).expect("read radium.lock"),
    )
    .expect("parse radium.lock");

    assert_eq!(lockfile["packages"]["react"]["version"], "18.3.1");
    assert_eq!(
        fs::read_to_string(directory.path().join("bun.lockb")).expect("read bun.lockb"),
        "binary lockfile"
    );
    assert!(!directory.path().join("bun.lock").exists());
}

#[test]
fn install_restores_locked_packages_from_the_cache_offline() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = directory.path().join("cache");
    let tarball = make_tarball();
    write_cached_package(&cache, "demo", "1.0.0", &tarball);

    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"offline","dependencies":{"demo":"^1.0.0"}}"#,
    )
    .expect("write package.json");

    let lockfile = r#"{
        "name": "offline",
        "lockfileVersion": 1,
        "packages": { "demo": { "version": "1.0.0" } }
    }"#;
    fs::write(directory.path().join("radium.lock"), lockfile).expect("write radium.lock");

    let output = radium(directory.path())
        .env("RADIUM_CACHE_DIR", &cache)
        .env("RADIUM_REGISTRY", "http://127.0.0.1:1")
        .arg("install")
        .output()
        .expect("run radium install");

    assert_success(&output);
    assert!(
        directory
            .path()
            .join("node_modules")
            .join("demo")
            .join("package.json")
            .is_file()
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("radium.lock")).expect("read radium.lock"),
        lockfile
    );

    let package =
        fs::read_to_string(directory.path().join("package.json")).expect("read package.json");
    assert!(package.contains("\"packageManager\": \"radium\""));
}

#[test]
fn install_fails_when_a_dependency_is_missing_from_the_lockfile() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = directory.path().join("cache");

    fs::write(
        directory.path().join("package.json"),
        r#"{"name":"missing-lock-entry","dependencies":{"demo":"1.0.0"}}"#,
    )
    .expect("write package.json");
    fs::write(
        directory.path().join("radium.lock"),
        r#"{"lockfileVersion":1,"packages":{}}"#,
    )
    .expect("write radium.lock");

    let node_modules = directory.path().join("node_modules");
    fs::create_dir_all(&node_modules).expect("create node_modules");
    fs::write(node_modules.join("sentinel"), "keep me").expect("write sentinel");

    let output = radium(directory.path())
        .env("RADIUM_CACHE_DIR", &cache)
        .arg("install")
        .output()
        .expect("run radium install");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("missing entries for: demo"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(node_modules.join("sentinel").is_file());
}

#[test]
fn cache_clear_removes_the_whole_cache() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = directory.path().join("cache");
    fs::create_dir_all(cache.join("metadata")).expect("create cache");
    fs::write(cache.join("metadata").join("marker"), "cached").expect("write marker");

    let output = radium(directory.path())
        .env("RADIUM_CACHE_DIR", &cache)
        .args(["cache", "clear"])
        .output()
        .expect("run radium cache clear");

    assert_success(&output);
    assert!(!cache.exists());
}

#[test]
fn cache_clear_package_keeps_shared_tarballs_and_other_packages() {
    let directory = tempfile::tempdir().expect("temp dir");
    let cache = directory.path().join("cache");
    let tarball = make_tarball();
    let hash = write_cached_package(&cache, "demo", "1.0.0", &tarball);
    write_cached_package(&cache, "other", "2.0.0", &tarball);

    let output = radium(directory.path())
        .env("RADIUM_CACHE_DIR", &cache)
        .args(["cache", "clear", "demo"])
        .output()
        .expect("run radium cache clear demo");

    assert_success(&output);
    assert!(
        !cache
            .join("metadata")
            .join("packages")
            .join("demo.json")
            .exists()
    );
    assert!(
        cache
            .join("metadata")
            .join("packages")
            .join("other.json")
            .is_file()
    );
    assert!(
        cache
            .join("packages")
            .join("sha256")
            .join(&hash[..2])
            .join(&hash)
            .is_file()
    );
}
