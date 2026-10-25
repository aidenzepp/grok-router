#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "these files are integration-test crates, so clippy.toml allow-expect-in-tests and allow-panic-in-tests do not see the helpers"
)]
#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    reason = "license is unset, and windows-sys 0.52 and 0.61 are transitive"
)]

//! `grok-router show` prints one stored model block.
//!
//! These tests pass `--config` except the built-in path, which uses the temp
//! home. They do not call the network and do not edit the file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMMENT: &str = "# kept comment\n";

const MODEL_SPAN: &str = "\
[model.kimi-k3]
model = \"k3\"
base_url = \"http://127.0.0.1:18180/kimi/v1\"
name = \"Kimi K3\"
description = \"oauth\"
env_key = \"KIMI_API_KEY\"
context_window = 1000000
";

const CHILD_SPAN: &str = "\
[model.kimi-k3.env_http_headers]
x-api-key = \"KIMI_API_KEY\"
";

const CONFIG_TAIL: &str = "\
[models]
default = \"grok-4.7\"
";

fn config_text() -> String {
    format!("{COMMENT}\n{MODEL_SPAN}\n{CHILD_SPAN}\n{CONFIG_TAIL}")
}

fn remove_temp(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => panic!(
            "the temp directory {} should be removable: {err}",
            path.display()
        ),
    }
}

fn temp_root(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("grok-router-show-{label}-{}", std::process::id()));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp show directory should be created");
    path
}

fn write_fixture(dir: &Path) -> PathBuf {
    let config = dir.join("config.toml");
    fs::write(&config, config_text()).expect("the config fixture should be written");
    config
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .args(args)
        .env_remove("GROK_ROUTER_OUTPUT")
        .env_remove("GROK_ROUTER_QUIET")
        .env_remove("GROK_ROUTER_TIMEOUT")
        .env("HOME", dir)
        .env("GROK_HOME", dir)
        .env("NO_COLOR", "1")
        .current_dir(dir)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("show stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("show stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

#[test]
fn show_prints_the_block_and_its_child_table_without_writing() {
    let dir = temp_root("block");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(&dir, &["show", "--config", "config.toml", "kimi-k3"]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "show of a configured slug should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("[model.kimi-k3]"),
        "show should print the model header; stdout: {stdout}"
    );
    assert!(
        stdout.contains("name = \"Kimi K3\""),
        "show should print the stored name; stdout: {stdout}"
    );
    assert!(
        stdout.contains("[model.kimi-k3.env_http_headers]"),
        "show should print the child table; stdout: {stdout}"
    );
    assert!(
        stdout.starts_with("kimi-k3\n"),
        "show should start with the slug; stdout: {stdout}"
    );
    assert_eq!(
        fs::read(&config).expect("the config should still be readable"),
        before,
        "show should not change the file"
    );
    remove_temp(&dir);
}

#[test]
fn show_marks_a_builtin_default_that_has_no_block() {
    let dir = temp_root("builtin");
    let _config = write_fixture(&dir);
    let output = run(&dir, &["show", "--config", "config.toml", "grok-4.7"]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "show of the saved built-in should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("grok-4.7  built-in  default"),
        "show should name the built-in and the saved default; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn show_of_an_unknown_slug_exits_1() {
    let dir = temp_root("missing");
    let _config = write_fixture(&dir);
    let output = run(&dir, &["show", "--config", "config.toml", "not-a-model"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "show of an unknown slug should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("not-a-model"),
        "stderr should name the missing slug; stderr: {stderr}"
    );
    assert!(
        stdout_text(&output).is_empty(),
        "a refused show should not print a body; stdout: {}",
        stdout_text(&output)
    );
    remove_temp(&dir);
}

#[test]
fn show_json_includes_the_body_and_the_default_flag() {
    let dir = temp_root("json");
    let _config = write_fixture(&dir);
    let output = run(
        &dir,
        &["--json", "show", "--config", "config.toml", "kimi-k3"],
    );
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "show --json should succeed; stderr: {}",
        stderr_text(&output)
    );
    let value: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("show --json should be one JSON value");
    assert_eq!(
        value.get("slug").and_then(serde_json::Value::as_str),
        Some("kimi-k3")
    );
    assert_eq!(value.get("default"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(value.get("builtin"), Some(&serde_json::Value::Bool(false)));
    let body = value
        .get("body")
        .and_then(serde_json::Value::as_str)
        .expect("the JSON object should include the stored body");
    assert!(
        body.contains("env_http_headers"),
        "the JSON body should include the child table; body: {body}"
    );
    remove_temp(&dir);
}
