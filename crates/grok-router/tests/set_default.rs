#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! `grok-router set-default` writes `[models].default` in a temp config.
//!
//! These tests pass `--config`, except the missing-path witness. They do not
//! call the network, do not read a home config, and do not start a session.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMMENT: &str = "# kept comment\n";

const MODEL_SPAN: &str = "\
[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 600000
";

const CONFIG_TAIL: &str = "\
[models]
default = \"grok-4.7\"
default_reasoning_effort = \"high\"
";

fn config_text() -> String {
    format!("{COMMENT}\n{MODEL_SPAN}\n{CONFIG_TAIL}")
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
    let path = std::env::temp_dir().join(format!(
        "grok-router-set-default-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp set-default directory should be created");
    path
}

fn write_fixture(dir: &Path) -> PathBuf {
    let config = dir.join("config.toml");
    fs::write(&config, config_text()).expect("the config fixture should be written");
    config
}

fn utf8_arg(path: &Path, what: &str) -> String {
    path.to_str().expect(what).to_owned()
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .env("HOME", dir)
        .env("NO_COLOR", "1")
        .current_dir(dir)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("set-default stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("set-default stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn backup_path(config: &Path) -> PathBuf {
    let name = config
        .file_name()
        .expect("the config fixture should have a file name");
    let mut backed = name.to_os_string();
    backed.push(".prev");
    config.with_file_name(backed)
}

fn assert_model_span_and_effort(text: &str) {
    assert!(
        text.contains(COMMENT),
        "the comment should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains(MODEL_SPAN),
        "the model span should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("default_reasoning_effort = \"high\""),
        "default_reasoning_effort should stay; config: {text}"
    );
    assert_eq!(
        text.lines().filter(|line| line.starts_with("[model.")).count(),
        1,
        "set-default should not add or remove a model span; config: {text}"
    );
}

#[test]
fn set_default_help_contains_an_example_and_says_it_writes_unless_dry_run() {
    let dir = temp_root("help");
    let output = run(&dir, &["set-default", "--help"]);
    let help = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "set-default --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.to_lowercase().contains("example"),
        "set-default --help should contain an example; help: {help}"
    );
    assert!(
        help.contains("grok-router set-default --config config.toml grok-4.6"),
        "set-default --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("writes the saved default unless --dry-run"),
        "set-default --help should say the command writes the saved default unless --dry-run is set; help: {help}"
    );
    remove_temp(&dir);
}

#[test]
fn set_default_saves_a_builtin_and_leaves_the_model_span() {
    let dir = temp_root("builtin");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "set-default",
            "grok-4.6",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "setting grok-4.6 should exit 0; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stderr.is_empty(),
        "a successful set-default should not prompt or write diagnostics; stderr: {stderr}"
    );
    assert_eq!(stdout, "grok-4.6\n", "stdout should be the saved slug");
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        after.contains("default = \"grok-4.6\"\n"),
        "the saved default should be grok-4.6; config: {after}"
    );
    assert!(
        !after.contains("default = \"grok-4.7\""),
        "the previous default line should be replaced; config: {after}"
    );
    assert_model_span_and_effort(&after);
    let previous = fs::read(backup_path(&config)).expect("a real write should leave a sibling backup");
    assert_eq!(
        previous, before,
        "the sibling backup should be the original bytes"
    );
    remove_temp(&dir);
}

#[test]
fn set_default_saves_a_configured_slug_and_leaves_the_model_span() {
    let dir = temp_root("configured");
    let config = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "set-default",
            "or-glm-5-3",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "setting a configured slug should exit 0; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(stderr.is_empty(), "stderr should stay empty; stderr: {stderr}");
    assert_eq!(stdout, "or-glm-5-3\n", "stdout should be the configured slug");
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        after.contains("default = \"or-glm-5-3\"\n"),
        "the saved default should be the configured slug; config: {after}"
    );
    assert_model_span_and_effort(&after);
    remove_temp(&dir);
}

#[test]
fn an_unknown_slug_exits_1_and_leaves_the_file() {
    let dir = temp_root("unknown");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "set-default",
            "not-a-model",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "an unknown slug should exit 1; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("not-a-model"),
        "the failure should name the slug; stderr: {stderr}"
    );
    assert!(
        stderr.contains("built-in") || stderr.contains("configured"),
        "the failure should say what to pass next; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(after, before, "an unknown slug should leave the file bytes identical");
    assert!(
        !backup_path(&config).exists(),
        "an unknown slug should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn dry_run_prints_the_slug_and_changes_nothing() {
    let dir = temp_root("dry-run");
    let config = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "set-default",
            "--dry-run",
            "grok-4.6",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "--dry-run should exit 0; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stdout.contains("grok-4.6"),
        "--dry-run should print the slug it would save; stdout: {stdout}"
    );
    assert!(
        stdout.contains("would save"),
        "--dry-run should say it would save the slug; stdout: {stdout}"
    );
    assert!(stderr.is_empty(), "dry-run should not prompt; stderr: {stderr}");
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(after, before, "--dry-run should leave the file unchanged");
    assert!(
        !backup_path(&config).exists(),
        "--dry-run should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn json_prints_one_object_and_quiet_prints_the_slug_only() {
    let dir = temp_root("output");
    let config = write_fixture(&dir);
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let json_out = run(
        &dir,
        &["--config", &config_arg, "set-default", "--json", "grok-4.6"],
    );
    let json_stdout = stdout_text(&json_out);
    assert_eq!(
        exit_code(&json_out),
        0,
        "--json should exit 0; stderr: {}",
        stderr_text(&json_out)
    );
    let value: serde_json::Value = serde_json::from_str(json_stdout.trim())
        .expect("set-default --json should print one JSON value");
    assert!(
        value.is_object(),
        "--json should print one object; stdout: {json_stdout}"
    );
    assert_eq!(
        value.get("slug").and_then(serde_json::Value::as_str),
        Some("grok-4.6"),
        "--json should name the saved slug; stdout: {json_stdout}"
    );
    assert_eq!(
        value.get("wrote").and_then(serde_json::Value::as_bool),
        Some(true),
        "--json should record that the file was written; stdout: {json_stdout}"
    );

    fs::write(&config, config_text()).expect("the config fixture should be restored");
    let quiet_out = run(
        &dir,
        &[
            "--config",
            &config_arg,
            "set-default",
            "--quiet",
            "or-glm-5-3",
        ],
    );
    let quiet_stdout = stdout_text(&quiet_out);
    assert_eq!(
        exit_code(&quiet_out),
        0,
        "--quiet should exit 0; stderr: {}",
        stderr_text(&quiet_out)
    );
    assert_eq!(
        quiet_stdout, "or-glm-5-3\n",
        "--quiet should print the slug only"
    );
    let after = fs::read_to_string(&config).expect("the quiet edit should be UTF-8");
    assert_model_span_and_effort(&after);
    assert!(
        after.contains("default = \"or-glm-5-3\"\n"),
        "quiet should still save the configured slug; config: {after}"
    );
    remove_temp(&dir);
}

#[test]
fn set_default_without_config_exits_1_and_does_not_open_a_home_config() {
    let dir = temp_root("no-config");
    let home_config = dir.join(".grok").join("config.toml");
    let output = run(&dir, &["set-default", "grok-4.6"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "set-default without --config should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("no --config"),
        "stderr should say that no --config path was given, not that a home file was missing; stderr: {stderr}"
    );
    assert!(
        !home_config.exists(),
        "set-default should not create a home config at {}",
        home_config.display()
    );
    remove_temp(&dir);
}
