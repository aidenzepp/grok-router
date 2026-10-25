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
        .env_remove("GROK_ROUTER_OUTPUT")
        .env_remove("GROK_ROUTER_QUIET")
        .env_remove("GROK_ROUTER_TIMEOUT")
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
    assert!(
        help.contains("Omitting --limit lists every row"),
        "list --help should say that omitting --limit lists every row; help: {help}"
    );
    assert!(
        help.contains("--limit"),
        "list --help should advertise --limit; help: {help}"
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
        .env_remove("GROK_ROUTER_OUTPUT")
        .env_remove("GROK_ROUTER_QUIET")
        .env_remove("GROK_ROUTER_TIMEOUT")
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

#[test]
fn root_help_shows_global_output_flags_and_a_shorter_short_help() {
    let dir = temp_root("root-help");
    let long = run(&dir, &["--help"]);
    let short = run(&dir, &["-h"]);
    let long_text = stdout_text(&long);
    let short_text = stdout_text(&short);
    assert_eq!(exit_code(&long), 0, "long help should exit 0; stderr: {}", stderr_text(&long));
    assert_eq!(exit_code(&short), 0, "short help should exit 0; stderr: {}", stderr_text(&short));
    assert!(long_text.contains("--output"), "root help should show --output; help: {long_text}");
    assert!(long_text.contains("--json"), "root help should show --json; help: {long_text}");
    assert!(long_text.contains("--quiet"), "root help should show --quiet; help: {long_text}");
    assert!(
        long_text.contains("[default: text]"),
        "root help should print the text default; help: {long_text}"
    );
    assert!(
        long_text.contains("$GROK_HOME") && long_text.contains("~/.grok") && long_text.contains("require --config"),
        "config help should name the default read path and the write requirement; help: {long_text}"
    );
    assert!(
        short_text.len() < long_text.len(),
        "-h should be shorter than --help"
    );
    let mut paired = false;
    let mut waiting = false;
    let mut distance = 0u8;
    for line in long_text.lines() {
        if waiting {
            if line.contains("grok-router list --config config.toml --output json") {
                paired = true;
                break;
            }
            distance = distance.saturating_add(1);
            if distance > 5 {
                waiting = false;
            }
        }
        if line.contains("grok-router list --config config.toml") && !line.contains("--output json") {
            waiting = true;
            distance = 0;
        }
    }
    assert!(paired, "help should pair a text list with --output json within five lines; help: {long_text}");
    remove_temp(&dir);
}

#[test]
fn output_json_on_help_and_version_prints_one_json_value() {
    let dir = temp_root("help-json");
    let help = run(&dir, &["--help", "--output", "json"]);
    let version = run(&dir, &["--version", "--output", "json"]);
    assert_eq!(exit_code(&help), 0, "json help should exit 0; stderr: {}", stderr_text(&help));
    assert_eq!(exit_code(&version), 0, "json version should exit 0; stderr: {}", stderr_text(&version));
    let help_body: serde_json::Value = serde_json::from_str(stdout_text(&help).trim())
        .expect("json help should be one JSON value");
    assert!(help_body.is_object(), "json help should be one JSON object");
    let version_body: serde_json::Value = serde_json::from_str(stdout_text(&version).trim())
        .expect("json version should be one JSON value");
    assert!(version_body.get("version").is_some(), "json version should name version");
    let bad = run(&dir, &["--this-flag-does-not-exist-anc", "--output", "json"]);
    assert_eq!(exit_code(&bad), 2, "a bad flag should exit 2; stderr: {}", stderr_text(&bad));
    let bad_body: serde_json::Value = serde_json::from_str(stderr_text(&bad).trim())
        .expect("a bad flag in json mode should print one JSON value on stderr");
    assert!(bad_body.get("message").is_some(), "the usage error should include a message");
    remove_temp(&dir);
}

#[test]
fn env_selects_json_and_an_explicit_flag_wins() {
    let dir = temp_root("env-output");
    let config = dir.join("config.toml");
    fs::write(&config, FIXTURE).expect("the config fixture should be written");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let from_env = command(&dir)
        .env("GROK_ROUTER_OUTPUT", "json")
        .args(["list", "--config", &config_arg])
        .output()
        .expect("the grok-router binary should start");
    assert_eq!(exit_code(&from_env), 0, "env json should list; stderr: {}", stderr_text(&from_env));
    let env_body: serde_json::Value = serde_json::from_str(stdout_text(&from_env).trim())
        .expect("GROK_ROUTER_OUTPUT=json should print JSON");
    assert!(env_body.is_array(), "env json list should be a JSON array");
    let forced_text = command(&dir)
        .env("GROK_ROUTER_OUTPUT", "json")
        .args(["list", "--config", &config_arg, "--output", "text"])
        .output()
        .expect("the grok-router binary should start");
    assert_eq!(exit_code(&forced_text), 0, "explicit text should win; stderr: {}", stderr_text(&forced_text));
    assert!(
        !stdout_text(&forced_text).trim_start().starts_with('['),
        "explicit --output text should stay text; stdout: {}",
        stdout_text(&forced_text)
    );
    let json_flag = run(&dir, &["list", "--config", &config_arg, "--json"]);
    let as_json = run(&dir, &["list", "--config", &config_arg, "--output", "json"]);
    let as_jsonl = run(&dir, &["list", "--config", &config_arg, "--output", "jsonl"]);
    assert_eq!(exit_code(&json_flag), 0, "--json should take no value; stderr: {}", stderr_text(&json_flag));
    assert_eq!(stdout_text(&as_json), stdout_text(&as_jsonl), "json and jsonl should print the same value");
    let conflict = run(&dir, &["list", "--config", &config_arg, "--json", "--output", "text"]);
    assert_ne!(exit_code(&conflict), 0, "--json with --output text should fail");
    let quiet = run(&dir, &["list", "--config", &config_arg, "--quiet", "--output", "json"]);
    assert_ne!(exit_code(&quiet), 0, "--quiet with json should fail");
    assert!(
        stderr_text(&quiet).contains("quiet"),
        "the quiet conflict should name quiet; stderr: {}",
        stderr_text(&quiet)
    );
    remove_temp(&dir);
}

#[test]
fn completions_bash_prints_a_script_without_reading_config() {
    let dir = temp_root("completions");
    let output = run(&dir, &["completions", "bash"]);
    let stdout = stdout_text(&output);
    assert_eq!(exit_code(&output), 0, "completions bash should exit 0; stderr: {}", stderr_text(&output));
    assert!(
        stdout.contains("grok-router") && stdout.contains("complete"),
        "bash completions should be a completion script; stdout: {stdout}"
    );
    assert!(
        !dir.join("config.toml").exists() && !dir.join(".grok").exists(),
        "completions should not read or create a config"
    );
    let unknown = run(&dir, &["completions", "nope"]);
    let stderr = stderr_text(&unknown);
    assert_eq!(exit_code(&unknown), 1, "an unknown shell should exit 1; stderr: {stderr}");
    for name in ["bash", "zsh", "fish", "elvish", "powershell"] {
        assert!(stderr.contains(name), "the refusal should list {name}; stderr: {stderr}");
    }
    assert!(
        stderr.contains("not read"),
        "the refusal should say the config was not read; stderr: {stderr}"
    );
    remove_temp(&dir);
}

#[test]
fn schema_prints_a_json_schema_without_reading_config() {
    let dir = temp_root("schema");
    let output = run(&dir, &["schema"]);
    let stdout = stdout_text(&output);
    assert_eq!(exit_code(&output), 0, "schema should exit 0; stderr: {}", stderr_text(&output));
    let body: serde_json::Value = serde_json::from_str(stdout.trim()).expect("schema should be JSON");
    assert!(
        body.get("$schema").and_then(|value| value.as_str()).is_some_and(|schema| schema.contains("json-schema.org")),
        "schema should name a JSON Schema dialect; stdout: {stdout}"
    );
    assert!(!dir.join("config.toml").exists(), "schema should not create a config");
    remove_temp(&dir);
}

#[test]
fn list_without_limit_prints_every_row() {
    let dir = temp_root("limit-all");
    let config = dir.join("config.toml");
    fs::write(&config, FIXTURE).expect("the config fixture should be written");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "omitting --limit should list; stderr: {}",
        stderr_text(&output)
    );
    assert_text_inventory("omitting --limit", &stdout);
    let bare = run(&dir, &["--config", &config_arg]);
    assert_eq!(
        stdout_text(&bare),
        stdout,
        "a bare invocation should list the same rows as list"
    );
    remove_temp(&dir);
}

#[test]
fn list_limit_keeps_the_first_rows_and_drops_the_rest() {
    let dir = temp_root("limit-n");
    let config = dir.join("config.toml");
    fs::write(&config, FIXTURE).expect("the config fixture should be written");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg, "--limit", "1"]);
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "--limit 1 should list; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(
        stdout.trim(),
        "or-deepseek-flash  deepseek/deepseek-v4.1-flash-1017",
        "--limit 1 should keep the first row and drop the rest; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("grok-4.7"),
        "--limit 1 should drop the later default row; stdout: {stdout}"
    );
    let bare = run(&dir, &["--config", &config_arg, "--limit", "1"]);
    assert_eq!(
        stdout_text(&bare).trim(),
        stdout.trim(),
        "a bare invocation should honor --limit"
    );
    let json = run(
        &dir,
        &["list", "--config", &config_arg, "--limit", "1", "--output", "json"],
    );
    assert_eq!(exit_code(&json), 0, "json --limit should list; stderr: {}", stderr_text(&json));
    let body: serde_json::Value =
        serde_json::from_str(stdout_text(&json).trim()).expect("limited json should be one value");
    let rows = body.as_array().expect("limited json should stay an array");
    assert_eq!(rows.len(), 1, "json --limit 1 should keep one object; body: {body}");
    assert_eq!(rows[0]["slug"], "or-deepseek-flash");
    let quiet = run(
        &dir,
        &["list", "--config", &config_arg, "--limit", "1", "--quiet"],
    );
    assert_eq!(
        stdout_text(&quiet).trim(),
        "or-deepseek-flash",
        "quiet --limit 1 should keep the first slug; stdout: {}",
        stdout_text(&quiet)
    );
    remove_temp(&dir);
}

#[test]
fn list_limit_zero_exits_nonzero_and_prints_nothing() {
    let dir = temp_root("limit-zero");
    let config = dir.join("config.toml");
    fs::write(&config, FIXTURE).expect("the config fixture should be written");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["list", "--config", &config_arg, "--limit", "0"]);
    assert_ne!(exit_code(&output), 0, "--limit 0 should fail");
    assert!(
        stdout_text(&output).is_empty(),
        "--limit 0 should print nothing; stdout: {}",
        stdout_text(&output)
    );
    let stderr = stderr_text(&output);
    assert!(
        stderr.contains("--limit 0 would hide every row"),
        "the refusal should say --limit 0 would hide every row; stderr: {stderr}"
    );
    assert!(
        stderr.contains("Omit --limit") && stderr.contains("greater than zero"),
        "the refusal should name the next action; stderr: {stderr}"
    );
    let bad = run(&dir, &["list", "--config", &config_arg, "--limit", "nope"]);
    assert_ne!(exit_code(&bad), 0, "a non-integer --limit should fail");
    assert!(
        stdout_text(&bad).is_empty(),
        "a non-integer --limit should print nothing; stdout: {}",
        stdout_text(&bad)
    );
    assert!(
        stderr_text(&bad).contains("nope"),
        "a non-integer refusal should name the bad value; stderr: {}",
        stderr_text(&bad)
    );
    remove_temp(&dir);
}

#[test]
fn json_error_carries_command_format_and_the_text_sentence() {
    let dir = temp_root("json-error");
    let missing = dir.join("missing.toml");
    let missing_arg = utf8_arg(&missing, "the missing path should be UTF-8");
    let text = run(&dir, &["list", "--config", &missing_arg]);
    let sentence = stderr_text(&text);
    assert_ne!(exit_code(&text), 0, "a missing config should fail");
    assert!(
        stdout_text(&text).is_empty(),
        "a text failure should not print a list; stdout: {}",
        stdout_text(&text)
    );
    assert!(
        !sentence.trim_start().starts_with('{'),
        "text mode should stay the sentence; stderr: {sentence}"
    );
    let json = run(
        &dir,
        &["list", "--config", &missing_arg, "--output", "json"],
    );
    assert_ne!(exit_code(&json), 0, "json mode should still fail");
    assert!(
        stdout_text(&json).is_empty(),
        "a json failure should keep stdout empty; stdout: {}",
        stdout_text(&json)
    );
    let body: serde_json::Value = serde_json::from_str(stderr_text(&json).trim())
        .expect("a json failure should be one JSON object");
    assert_eq!(body["command"], "list");
    assert_eq!(body["format"], "json");
    assert_eq!(body["error"], "list_config_missing");
    assert_eq!(body["kind"], "list_config_missing");
    assert_eq!(
        body["message"].as_str(),
        Some(sentence.trim()),
        "the json message should be the text sentence"
    );
    let jsonl = run(
        &dir,
        &["list", "--config", &missing_arg, "--jsonl"],
    );
    let jsonl_body: serde_json::Value = serde_json::from_str(stderr_text(&jsonl).trim())
        .expect("jsonl should still be one JSON object");
    assert_eq!(jsonl_body["format"], "json", "jsonl errors use format json");
    remove_temp(&dir);
}
