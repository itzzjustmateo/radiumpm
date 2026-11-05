use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn radium(directory: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_radium"));
    command.current_dir(directory);
    command
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
