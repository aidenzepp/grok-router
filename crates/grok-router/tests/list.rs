#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! `grok-router list` prints the saved default and configured models.
//!
//! These tests pass `--config` or point `GROK_HOME` at a temp directory.
//! They do not read a home config.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURE: &str = r#"# kept comment

[model.or-deepseek-flash]
model = "deepseek/deepseek-v4.1-flash-1017"
base_url = "https://openrouter.ai/api/v1"
name = "DeepSeek Flash"

[models]
default = "grok-4.7"
"#;

const CONFIGURED_DEFAULT: &str = r#"# kept comment

[model.or-deepseek-flash]
model = "deepseek/deepseek-v4.1-flash-1017"
base_url = "https://openrouter.ai/api/v1"
name = "DeepSeek Flash"

[models]
default = "or-deepseek-flash"
"#;

const DECOY: &str = r#"# decoy

[model.not-the-fixture]
model = "decoy/model"

[models]
default = "grok-4.6"
"#;

const BROKEN: &str = "default = [\n";

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
        "grok-router-list-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp list directory should be created");
    path
}

fn utf8_arg(path: &Path, what: &str) -> String {
    path.to_str().expect(what).to_owned()
}

fn command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grok-router"));
    _ = command
        .env_remove("OPENROUTER_API_KEY")
        .env("HOME", dir)
        .env("GROK_HOME", dir)
        .env("NO_COLOR", "1")
        .current_dir(dir);
    command
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    command(dir)
        .args(args)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("list stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("list stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn write_pair(dir: &Path) -> (PathBuf, PathBuf) {
    let config = dir.join("named.toml");
    let decoy = dir.join("config.toml");
    fs::write(&config, FIXTURE).expect("the config fixture should be written");
    fs::write(&decoy, DECOY).expect("the decoy config should be written");
    (config, decoy)
}

fn assert_same_bytes(path: &Path, before: &[u8], label: &str) {
    let after = fs::read(path).expect("the config should still be readable");
    assert_eq!(
        after, before,
        "{label} should leave {} byte-identical",
        path.display()
    );
}

fn assert_text_inventory(label: &str, stdout: &str) {
    assert_eq!(
        stdout.trim(),
        "or-deepseek-flash  deepseek/deepseek-v4.1-flash-1017\ngrok-4.7  default",
        "{label} should print the configured model and mark the saved default; stdout: {stdout}"
    );
    for absent in [
        "grok-4.7-build-fast",
        "grok-4.6",
        "grok-4.5",
        "not-the-fixture",
        "decoy/model",
    ] {
        assert!(
            !stdout.contains(absent),
            "{label} should not invent {absent}; stdout: {stdout}"
        );
    }
    assert!(
        !stdout.contains('\u{1b}'),
        "{label} must not contain color escapes; stdout: {stdout}"
    );
}

#[test]
fn list_help_contains_an_example_and_says_it_does_not_modify_the_config() {
    let dir = temp_root("help");
    let output = run(&dir, &["list", "--help"]);
    let help = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "list --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.to_lowercase().contains("example"),
        "list --help should contain an example; help: {help}"
    );
    assert!(
        help.contains("grok-router list --config config.toml"),
        "list --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("does not modify the config"),
        "list --help should say the command does not modify the config; help: {help}"
    );
    remove_temp(&dir);
}

#[test]
fn no_args_prints_the_saved_default_and_the_configured_slug_without_changing_bytes() {
    let dir = temp_root("no-args");
    let (config, decoy) = write_pair(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let decoy_before = fs::read(&decoy).expect("the decoy config should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["--config", &config_arg]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "no-args should list the config; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stderr_text(&output).is_empty(),
        "a successful list should keep diagnostics off stdout; stderr: {}",
        stderr_text(&output)
    );
    assert_text_inventory("no-args", &stdout);
    assert_same_bytes(&config, &before, "no-args");
    assert_same_bytes(&decoy, &decoy_before, "no-args decoy");
    remove_temp(&dir);
}

#[test]
fn list_prints_the_saved_default_and_the_configured_slug_without_changing_bytes() {
    let dir = temp_root("text");
    let (config, decoy) = write_pair(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let decoy_before = fs::read(&decoy).expect("the decoy config should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "list should print the inventory; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stderr_text(&output).is_empty(),
        "a successful list should keep diagnostics off stdout; stderr: {}",
        stderr_text(&output)
    );
    assert_text_inventory("list", &stdout);
    assert_same_bytes(&config, &before, "list");
    assert_same_bytes(&decoy, &decoy_before, "list decoy");
    remove_temp(&dir);
}

#[test]
fn list_json_parses_to_the_saved_default_and_the_configured_slug_without_changing_bytes() {
    let dir = temp_root("json");
    let (config, decoy) = write_pair(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let decoy_before = fs::read(&decoy).expect("the decoy config should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg, "--json"]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "list --json should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stderr_text(&output).is_empty(),
        "json diagnostics should stay off stdout; stderr: {}",
        stderr_text(&output)
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("list --json should parse as JSON");
    assert_eq!(
        parsed,
        serde_json::json!([
            {
                "slug": "or-deepseek-flash",
                "provider_id": "deepseek/deepseek-v4.1-flash-1017",
                "default": false
            },
            {
                "slug": "grok-4.7",
                "default": true
            }
        ]),
        "json should be the configured model and the saved default, with no invented built-ins"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "json output must not contain color escapes; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("grok-4.6") && !stdout.contains("not-the-fixture"),
        "json should not include the decoy config; stdout: {stdout}"
    );
    assert_same_bytes(&config, &before, "list --json");
    assert_same_bytes(&decoy, &decoy_before, "list --json decoy");
    remove_temp(&dir);
}

#[test]
fn list_quiet_prints_one_slug_per_line_without_changing_bytes() {
    let dir = temp_root("quiet");
    let (config, decoy) = write_pair(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let decoy_before = fs::read(&decoy).expect("the decoy config should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg, "--quiet"]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "list --quiet should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(
        stdout.trim(),
        "or-deepseek-flash\ngrok-4.7",
        "quiet output should be one slug per line, including the saved default; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("default") && !stdout.contains("deepseek/"),
        "quiet output should be slugs only; stdout: {stdout}"
    );
    assert_same_bytes(&config, &before, "list --quiet");
    assert_same_bytes(&decoy, &decoy_before, "list --quiet decoy");
    remove_temp(&dir);
}

#[test]
fn list_marks_a_configured_default_once_and_does_not_invent_builtins() {
    let dir = temp_root("configured-default");
    let config = dir.join("named.toml");
    fs::write(&config, CONFIGURED_DEFAULT).expect("the config fixture should be written");
    let before = fs::read(&config).expect("the config fixture should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "a configured default should still list; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(
        stdout.trim(),
        "or-deepseek-flash  deepseek/deepseek-v4.1-flash-1017  default",
        "the configured default should be marked once; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("grok-4.7"),
        "a configured default should not invent built-in rows; stdout: {stdout}"
    );
    assert_same_bytes(&config, &before, "configured default");
    remove_temp(&dir);
}

#[test]
fn missing_config_exits_1_and_says_what_to_do_next() {
    let dir = temp_root("missing");
    let missing = dir.join("missing.toml");
    let missing_arg = utf8_arg(&missing, "the missing config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &missing_arg]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "a missing config should fail; stderr: {stderr}"
    );
    assert!(
        stdout_text(&output).is_empty(),
        "a missing config should not print an inventory; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains(missing_arg.as_str()),
        "the error should name the missing file; stderr: {stderr}"
    );
    assert!(
        stderr.contains("missing"),
        "the error should say the file is missing; stderr: {stderr}"
    );
    assert!(
        stderr.contains("pass --config") || stderr.contains("Create the file"),
        "the error should say what to do next; stderr: {stderr}"
    );
    assert!(
        !missing.exists(),
        "a missing config must not be created"
    );
    remove_temp(&dir);
}

#[test]
fn broken_config_exits_1_and_leaves_the_bytes_unchanged() {
    let dir = temp_root("broken");
    let config = dir.join("named.toml");
    fs::write(&config, BROKEN).expect("the broken config should be written");
    let before = fs::read(&config).expect("the broken config should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "a config that does not parse should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("did not parse") || stderr.contains("parse"),
        "the error should say the config did not parse; stderr: {stderr}"
    );
    assert!(
        stderr.contains("pass --config") || stderr.contains("Fix the config"),
        "the error should say what to do next; stderr: {stderr}"
    );
    assert_same_bytes(&config, &before, "broken config");
    remove_temp(&dir);
}

#[test]
fn list_reads_grok_home_config_when_the_config_flag_is_absent() {
    let dir = temp_root("grok-home");
    let home = dir.join("home");
    let grok_home = dir.join("grok-home");
    let home_grok = home.join(".grok");
    fs::create_dir_all(&home_grok).expect("the home .grok directory should be created");
    fs::create_dir_all(&grok_home).expect("the GROK_HOME directory should be created");
    let grok_config = grok_home.join("config.toml");
    let home_config = home_grok.join("config.toml");
    fs::write(&grok_config, FIXTURE).expect("the GROK_HOME config should be written");
    fs::write(&home_config, DECOY).expect("the HOME config should be written");
    let before = fs::read(&grok_config).expect("the GROK_HOME config should be readable");
    let home_before = fs::read(&home_config).expect("the HOME config should be readable");
    let output = Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .arg("list")
        .env_remove("OPENROUTER_API_KEY")
        .env("HOME", &home)
        .env("GROK_HOME", &grok_home)
        .env("NO_COLOR", "1")
        .current_dir(&dir)
        .output()
        .expect("the grok-router binary should start");
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "list should read GROK_HOME/config.toml when --config is absent; stderr: {}",
        stderr_text(&output)
    );
    assert_text_inventory("GROK_HOME", &stdout);
    assert_same_bytes(&grok_config, &before, "GROK_HOME config");
    assert_same_bytes(&home_config, &home_before, "HOME config");
    remove_temp(&dir);
}
