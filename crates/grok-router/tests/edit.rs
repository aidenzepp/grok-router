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

//! `grok-router edit` changes fields inside one stored model block.
//!
//! These tests pass `--config`. They do not call the network and do not read
//! a home config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMMENT: &str = "# kept comment\n";

const OTHER_SPAN: &str = "\
[model.hub-default]
model = \"default\"
base_url = \"http://hub.example/qwen/v1\"
name = \"Hub\"
env_key = \"VIVI_PROXY_TOKEN\"
context_window = 131072
";

const MODEL_SPAN: &str = "\
[model.kimi-k3]
model = \"k3\"
base_url = \"http://127.0.0.1:18180/kimi/v1\"
name = \"Kimi K3\"
env_key = \"KIMI_API_KEY\"
context_window = 1000000
max_completion_tokens = 8192
";

const CHILD_SPAN: &str = "\
[model.kimi-k3.env_http_headers]
x-api-key = \"KIMI_API_KEY\"
";

const CONFIG_TAIL: &str = "\
[models]
default = \"grok-4.7\"
default_reasoning_effort = \"high\"
";

fn config_text() -> String {
    format!("{COMMENT}\n{OTHER_SPAN}\n{MODEL_SPAN}\n{CHILD_SPAN}\n{CONFIG_TAIL}")
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
        std::env::temp_dir().join(format!("grok-router-edit-{label}-{}", std::process::id()));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp edit directory should be created");
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
        .env_remove("KIMI_API_KEY")
        .env_remove("VIVI_PROXY_TOKEN")
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
    String::from_utf8(output.stdout.clone()).expect("edit stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("edit stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn backup_path(config: &Path) -> PathBuf {
    let mut backup = config.to_path_buf();
    let mut name = config
        .file_name()
        .expect("the config fixture should have a file name")
        .to_os_string();
    name.push(".prev");
    backup.set_file_name(name);
    backup
}

#[test]
fn edit_replaces_a_field_appends_a_missing_one_and_keeps_the_child_table() {
    let dir = temp_root("fields");
    let config = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "edit",
            "--config",
            "config.toml",
            "kimi-k3",
            "--name",
            "Kimi K3 (OAuth)",
            "--description",
            "kimi-code oauth",
            "--reasoning-effort",
            "high",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "edit of an existing model should succeed; stderr: {}",
        stderr_text(&output)
    );
    let text = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        text.contains(COMMENT),
        "the comment should stay; config: {text}"
    );
    assert!(
        text.contains(OTHER_SPAN),
        "the other model should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains(CHILD_SPAN),
        "the child table should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("name = \"Kimi K3 (OAuth)\""),
        "the name line should be replaced; config: {text}"
    );
    assert!(
        text.contains("description = \"kimi-code oauth\""),
        "a missing description should be appended; config: {text}"
    );
    assert!(
        text.contains("reasoning_effort = \"high\""),
        "a missing reasoning_effort should be appended; config: {text}"
    );
    assert!(
        text.contains("default_reasoning_effort = \"high\""),
        "the models table should stay; config: {text}"
    );
    assert!(
        backup_path(&config).is_file(),
        "a real edit should write a sibling backup"
    );
    remove_temp(&dir);
}

#[test]
fn edit_dry_run_prints_the_path_and_leaves_the_file_unchanged() {
    let dir = temp_root("dry-run");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "edit",
            "--dry-run",
            "--config",
            "config.toml",
            "kimi-k3",
            "--context-window",
            "1048576",
        ],
    );
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "edit --dry-run should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("would write"),
        "dry-run should say it would write; stdout: {stdout}"
    );
    assert_eq!(
        fs::read(&config).expect("the config should still be readable"),
        before,
        "dry-run should leave the bytes identical"
    );
    assert!(
        !backup_path(&config).exists(),
        "dry-run should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn edit_clear_removes_only_that_assignment() {
    let dir = temp_root("clear");
    let config = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "edit",
            "--config",
            "config.toml",
            "kimi-k3",
            "--clear",
            "max_completion_tokens",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "edit --clear of a present field should succeed; stderr: {}",
        stderr_text(&output)
    );
    let text = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        !text.contains("max_completion_tokens"),
        "the cleared field should be gone; config: {text}"
    );
    assert!(
        text.contains("context_window = 1000000"),
        "the other fields should stay; config: {text}"
    );
    assert!(
        text.contains(CHILD_SPAN),
        "the child table should stay; config: {text}"
    );
    remove_temp(&dir);
}

#[test]
fn edit_can_change_the_provider_id() {
    let dir = temp_root("provider");
    let config = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "edit",
            "--config",
            "config.toml",
            "kimi-k3",
            "--model",
            "kimi-k3",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "edit --model should succeed; stderr: {}",
        stderr_text(&output)
    );
    let text = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        text.contains("model = \"kimi-k3\""),
        "the provider id should change; config: {text}"
    );
    assert!(
        text.contains("[model.kimi-k3]"),
        "the slug header should stay; config: {text}"
    );
    remove_temp(&dir);
}

#[test]
fn edit_without_a_field_exits_1_and_leaves_the_file_unchanged() {
    let dir = temp_root("empty");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(&dir, &["edit", "--config", "config.toml", "kimi-k3"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "edit with no field should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("no field was given"),
        "stderr should say to pass a field; stderr: {stderr}"
    );
    assert_eq!(
        fs::read(&config).expect("the config should still be readable"),
        before,
        "a refused edit should leave the bytes identical"
    );
    remove_temp(&dir);
}

#[test]
fn edit_of_a_builtin_exits_1() {
    let dir = temp_root("builtin");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "edit",
            "--config",
            "config.toml",
            "grok-4.7",
            "--name",
            "Grok",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "edit of a built-in should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("built-in"),
        "stderr should say the slug is a built-in; stderr: {stderr}"
    );
    assert_eq!(
        fs::read(&config).expect("the config should still be readable"),
        before,
        "a refused edit should leave the bytes identical"
    );
    remove_temp(&dir);
}
