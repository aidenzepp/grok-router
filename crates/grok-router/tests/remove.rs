#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! `grok-router remove` deletes one model block from a temp config.
//!
//! These tests pass `--config`. They do not call the network and do not read
//! a home config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMMENT: &str = "# kept comment\n";

const DEEPSEEK_SPAN: &str = "\
[model.or-deepseek-flash]
model = \"deepseek/deepseek-v4.1-flash-1017\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
max_completion_tokens = 65536
";

const GLM_SPAN: &str = "\
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
    format!("{COMMENT}\n{DEEPSEEK_SPAN}\n{GLM_SPAN}\n{CONFIG_TAIL}")
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
        "grok-router-remove-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp remove directory should be created");
    path
}

fn write_config(dir: &Path) -> PathBuf {
    let config = dir.join("config.toml");
    fs::write(&config, config_text()).expect("the config fixture should be written");
    config
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .env("HOME", dir)
        .env("GROK_HOME", dir)
        .env("NO_COLOR", "1")
        .current_dir(dir)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("remove stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("remove stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn sibling_backup(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .expect("the config path should have a file name");
    let mut backup = path.to_path_buf();
    let mut file_name = name.to_os_string();
    file_name.push(".prev");
    backup.set_file_name(file_name);
    backup
}

fn assert_file_unchanged(config: &Path, before: &[u8]) {
    let after = fs::read(config).expect("the config should still be readable");
    assert_eq!(
        after, before,
        "the config bytes should stay identical"
    );
    let backup = sibling_backup(config);
    assert!(
        !backup.exists(),
        "a remove that does not delete should not write {}",
        backup.display()
    );
}

fn model_headers(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("[model."))
        .count()
}

#[test]
fn remove_help_contains_an_example_and_says_it_deletes_a_model_block() {
    let dir = temp_root("help");
    let output = run(&dir, &["remove", "--help"]);
    let help = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "remove --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.to_lowercase().contains("example"),
        "remove --help should contain an example; help: {help}"
    );
    assert!(
        help.contains("grok-router remove --yes --config config.toml or-glm-5-3"),
        "remove --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("deletes a model block"),
        "remove --help should say the command deletes a model block; help: {help}"
    );
    remove_temp(&dir);
}

#[test]
fn remove_with_yes_deletes_that_span_and_leaves_every_other_model_span() {
    let dir = temp_root("delete");
    let config = write_config(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "remove",
            "--yes",
            "--config",
            "config.toml",
            "or-glm-5-3",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "remove --yes of a non-default slug should succeed; stderr: {}",
        stderr_text(&output)
    );
    let text = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        !text.contains("[model.or-glm-5-3]"),
        "remove --yes should delete the or-glm-5-3 span; config: {text}"
    );
    assert!(
        !text.contains(GLM_SPAN),
        "remove --yes should delete the or-glm-5-3 block, not only its header; config: {text}"
    );
    assert!(
        text.contains(DEEPSEEK_SPAN),
        "remove --yes should leave the deepseek span byte-identical; config: {text}"
    );
    assert!(
        text.contains(COMMENT),
        "remove --yes should leave the comment; config: {text}"
    );
    assert!(
        text.contains("default = \"grok-4.7\"\n"),
        "remove --yes should leave the saved default; config: {text}"
    );
    assert_eq!(
        model_headers(&text),
        1,
        "remove --yes should leave exactly one model span; config: {text}"
    );
    let backup = fs::read(sibling_backup(&config))
        .expect("remove --yes should write a sibling config.toml.prev backup");
    assert_eq!(
        backup, before,
        "the sibling backup should be the config from before the delete"
    );
    remove_temp(&dir);
}

#[test]
fn remove_of_the_saved_default_exits_1_and_leaves_the_file_unchanged() {
    let dir = temp_root("default");
    let config = write_config(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &["remove", "--yes", "--config", "config.toml", "grok-4.7"],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "remove of the saved default should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("saved default"),
        "stderr should say the slug is the saved default; stderr: {stderr}"
    );
    assert_file_unchanged(&config, &before);
    remove_temp(&dir);
}

#[test]
fn remove_of_a_built_in_exits_1_and_leaves_the_file_unchanged() {
    let dir = temp_root("builtin");
    let config = write_config(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &["remove", "--yes", "--config", "config.toml", "grok-4.6"],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "remove of a built-in should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("built-in"),
        "stderr should say the slug is a built-in; stderr: {stderr}"
    );
    assert_file_unchanged(&config, &before);
    remove_temp(&dir);
}

#[test]
fn remove_of_a_missing_slug_exits_1_and_leaves_the_file_unchanged() {
    let dir = temp_root("missing");
    let config = write_config(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "remove",
            "--yes",
            "--config",
            "config.toml",
            "or-missing",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "remove of a missing slug should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("not in the config"),
        "stderr should say the slug is not in the config; stderr: {stderr}"
    );
    assert_file_unchanged(&config, &before);
    remove_temp(&dir);
}

#[test]
fn remove_without_yes_exits_1_and_leaves_the_file_unchanged() {
    let dir = temp_root("no-yes");
    let config = write_config(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(&dir, &["remove", "--config", "config.toml", "or-glm-5-3"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "remove without --yes should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("pass --yes"),
        "stderr should say to pass --yes; stderr: {stderr}"
    );
    assert!(
        stdout_text(&output).is_empty(),
        "a refused remove should not print a success body; stdout: {}",
        stdout_text(&output)
    );
    assert_file_unchanged(&config, &before);
    remove_temp(&dir);
}

#[test]
fn dry_run_prints_the_slug_and_leaves_the_file_unchanged() {
    let dir = temp_root("dry-run");
    let config = write_config(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "remove",
            "--dry-run",
            "--config",
            "config.toml",
            "or-glm-5-3",
        ],
    );
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "remove --dry-run of a deletable slug should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("or-glm-5-3"),
        "dry-run should print the slug it would delete; stdout: {stdout}"
    );
    assert!(
        stdout.contains("would delete"),
        "dry-run should say it would delete that slug; stdout: {stdout}"
    );
    assert_file_unchanged(&config, &before);
    remove_temp(&dir);
}

#[test]
fn json_prints_one_object() {
    let dir = temp_root("json");
    let config = write_config(&dir);
    let output = run(
        &dir,
        &[
            "remove",
            "--yes",
            "--json",
            "--config",
            "config.toml",
            "or-glm-5-3",
        ],
    );
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "remove --json should succeed; stderr: {}",
        stderr_text(&output)
    );
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).expect(
        "remove --json should print one JSON value and nothing else",
    );
    assert!(
        value.is_object(),
        "remove --json should print one object; stdout: {stdout}"
    );
    assert_eq!(
        value.get("slug").and_then(serde_json::Value::as_str),
        Some("or-glm-5-3"),
        "the object should name the deleted slug; stdout: {stdout}"
    );
    assert_eq!(
        value.get("removed"),
        Some(&serde_json::Value::Bool(true)),
        "the object should say the block was removed; stdout: {stdout}"
    );
    assert_eq!(
        value.get("dry_run"),
        Some(&serde_json::Value::Bool(false)),
        "the object should say this was not a dry run; stdout: {stdout}"
    );
    let text = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        !text.contains("[model.or-glm-5-3]"),
        "remove --json --yes should still delete the span; config: {text}"
    );
    remove_temp(&dir);
}

#[test]
fn quiet_prints_the_slug_only() {
    let dir = temp_root("quiet");
    let config = write_config(&dir);
    let output = run(
        &dir,
        &[
            "remove",
            "--yes",
            "--quiet",
            "--config",
            "config.toml",
            "or-glm-5-3",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "remove --quiet should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(
        stdout_text(&output),
        "or-glm-5-3\n",
        "quiet should print the slug only"
    );
    let text = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        text.contains(DEEPSEEK_SPAN),
        "remove --quiet should leave the other model span; config: {text}"
    );
    remove_temp(&dir);
}

#[test]
fn remove_without_config_exits_1_and_does_not_open_a_home_config() {
    let dir = temp_root("no-config");
    let home_config = dir.join(".grok").join("config.toml");
    let output = run(&dir, &["remove", "--yes", "or-glm-5-3"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "remove without --config should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("no --config"),
        "stderr should say that no --config path was given, not that a home file was missing; stderr: {stderr}"
    );
    assert!(
        !home_config.exists(),
        "remove should not create a home config at {}",
        home_config.display()
    );
    remove_temp(&dir);
}

#[test]
fn remove_refuses_a_named_env_key_value_and_leaves_the_file() {
    let dir = temp_root("named-secret");
    let config = dir.join("config.toml");
    let secret = "deepseek-secret-value-91";
    let text = format!(
        "\
# kept comment
[model.or-drop]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
env_key = \"DEEPSEEK_API_KEY\"
api_backend = \"chat_completions\"
context_window = 600000

[model.or-kept]
model = \"deepseek/deepseek-v4.1-flash-1017\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"{secret}\"
env_key = \"DEEPSEEK_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000

[models]
default = \"grok-4.7\"
default_reasoning_effort = \"high\"
"
    );
    fs::write(&config, &text).expect("the named-secret fixture should be written");
    let before = fs::read(&config).expect("the named-secret fixture should be readable");
    let output = Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .args(["remove", "--yes", "--config", "config.toml", "or-drop"])
        .env("DEEPSEEK_API_KEY", secret)
        .env_remove("OPENROUTER_API_KEY")
        .env("HOME", &dir)
        .env("GROK_HOME", &dir)
        .env("NO_COLOR", "1")
        .current_dir(&dir)
        .output()
        .expect("the grok-router binary should start");
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "remove should refuse a named env value; stderr: {stderr}"
    );
    assert!(
        stderr.contains("DEEPSEEK_API_KEY"),
        "stderr should name DEEPSEEK_API_KEY; stderr: {stderr}"
    );
    assert!(
        !stderr.contains(secret),
        "stderr should not print the secret value; stderr: {stderr}"
    );
    assert_file_unchanged(&config, &before);
    remove_temp(&dir);
}
