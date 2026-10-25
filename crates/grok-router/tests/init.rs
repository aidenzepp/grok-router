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

//! `grok-router init` writes a default `OpenRouter` `[router]` table.
//!
//! These tests use temp files. They set `HOME` and `GROK_HOME` themselves.
//! They do not read or write a real home config and do not call the network.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BLOCK: &str = "\
[router]
default_provider = \"openrouter\"

[router.provider.openrouter]
base_url = \"https://openrouter.ai/api/v1\"
env_key = \"OPENROUTER_API_KEY\"
catalog = \"openrouter-models\"
";

const ORIGINAL: &str = "# kept comment\n\n[models]\ndefault = \"grok-4.5\"\n";

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
        std::env::temp_dir().join(format!("grok-router-init-{label}-{}", std::process::id()));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp init directory should be created");
    path
}

fn utf8_arg(path: &Path, what: &str) -> String {
    path.to_str().expect(what).to_owned()
}

fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_grok-router"));
    let _configured = cmd
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("GROK_HOME")
        .env_remove("GROK_ROUTER_OUTPUT")
        .env_remove("GROK_ROUTER_QUIET")
        .env_remove("GROK_ROUTER_TIMEOUT")
        .env("HOME", dir)
        .env("NO_COLOR", "1")
        .current_dir(dir);
    cmd
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    command(dir)
        .args(args)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("init stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("init stderr should be UTF-8")
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

#[test]
fn init_help_lists_the_command() {
    let dir = temp_root("help");
    let root = run(&dir, &["--help"]);
    let root_help = stdout_text(&root);
    assert_eq!(
        exit_code(&root),
        0,
        "grok-router --help should succeed; stderr: {}",
        stderr_text(&root)
    );
    assert!(
        root_help.contains("init"),
        "root help should list init; help: {root_help}"
    );
    let output = run(&dir, &["init", "--help"]);
    let help = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "init --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.contains("grok-router init"),
        "init --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("writes unless --dry-run"),
        "init --help should say the command writes unless --dry-run is set; help: {help}"
    );
    remove_temp(&dir);
}

#[test]
fn init_creates_a_missing_config_as_the_openrouter_block() {
    let dir = temp_root("missing");
    let config = dir.join("config.toml");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    assert!(
        !config.exists(),
        "the missing-path fixture should start absent"
    );
    let output = run(&dir, &["--config", &arg, "init"]);
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "init should create a missing config; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stderr.is_empty(),
        "a successful init should not write diagnostics; stderr: {stderr}"
    );
    assert!(
        stdout.contains(&arg) && stdout.contains("wrote"),
        "text output should name the path and that it wrote; stdout: {stdout}"
    );
    let written = fs::read_to_string(&config).expect("the created config should be UTF-8");
    assert_eq!(
        written, BLOCK,
        "a missing --config path should be created as the OpenRouter block"
    );
    assert_eq!(
        grok_router::config_edit::openrouter_router_block(),
        BLOCK,
        "the rendered block should be the acceptance bytes"
    );
    assert!(
        !backup_path(&config).exists(),
        "a new file has nothing to back up"
    );
    remove_temp(&dir);
}

#[test]
fn init_json_reports_the_write() {
    let dir = temp_root("json");
    let config = dir.join("config.toml");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["--config", &arg, "--output", "json", "init"]);
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "json init should create the file; stderr: {stderr}; stdout: {stdout}"
    );
    let body: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("json init should print one JSON object");
    assert_eq!(
        body.get("command").and_then(|value| value.as_str()),
        Some("init")
    );
    assert_eq!(
        body.get("format").and_then(|value| value.as_str()),
        Some("json")
    );
    assert_eq!(
        body.get("path").and_then(|value| value.as_str()),
        Some(arg.as_str())
    );
    assert_eq!(body.get("wrote"), Some(&serde_json::Value::Bool(true)));
    assert_eq!(body.get("unchanged"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(body.get("dry_run"), Some(&serde_json::Value::Bool(false)));
    assert_eq!(
        fs::read_to_string(&config).expect("the json init should still write the file"),
        BLOCK
    );
    remove_temp(&dir);
}

#[test]
fn init_appends_to_a_comment_and_models_table_and_resolves_openrouter() {
    let dir = temp_root("append");
    let config = dir.join("config.toml");
    fs::write(&config, ORIGINAL).expect("the comment fixture should be written");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["--config", &arg, "init"]);
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "init should append to an existing file; stderr: {stderr}; stdout: {stdout}"
    );
    let edited = fs::read_to_string(&config).expect("the appended config should stay UTF-8");
    assert_eq!(
        edited,
        format!("{ORIGINAL}\n{BLOCK}"),
        "the comment and [models] table should stay, then a blank line, then the block"
    );
    assert_eq!(
        fs::read_to_string(backup_path(&config)).expect("an existing file should be backed up"),
        ORIGINAL,
        "the backup should be the bytes from before the write"
    );
    let record = grok_router::config_edit::resolve_provider(
        &edited,
        grok_router::config_edit::ProviderChoice::Default,
    )
    .expect("omitting --provider should select the written openrouter record");
    assert_eq!(record.base_url, "https://openrouter.ai/api/v1");
    assert_eq!(record.env_key, "OPENROUTER_API_KEY");
    assert_eq!(record.catalog.as_deref(), Some("openrouter-models"));
    remove_temp(&dir);
}

#[test]
fn init_a_second_time_leaves_the_bytes_identical() {
    let dir = temp_root("second");
    let config = dir.join("config.toml");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let first = run(&dir, &["--config", &arg, "init"]);
    assert_eq!(
        exit_code(&first),
        0,
        "the first init should write; stderr: {}",
        stderr_text(&first)
    );
    let written = fs::read(&config).expect("the first init should leave a file");
    let second = run(&dir, &["--config", &arg, "init"]);
    let stdout = stdout_text(&second);
    let stderr = stderr_text(&second);
    assert_eq!(
        exit_code(&second),
        0,
        "a second init should exit 0; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stdout.contains("already has [router]") && stdout.contains("nothing was written"),
        "a second init should say the table is already there and nothing was written; stdout: {stdout}"
    );
    assert_eq!(
        fs::read(&config).expect("the second init should leave the file readable"),
        written,
        "a second init should leave the bytes identical"
    );
    let preview = run(&dir, &["--config", &arg, "init", "--dry-run"]);
    let preview_out = stdout_text(&preview);
    assert_eq!(
        exit_code(&preview),
        0,
        "dry-run on an existing table should exit 0; stderr: {}",
        stderr_text(&preview)
    );
    assert!(
        preview_out.contains("would write nothing"),
        "dry-run should say it would write nothing when [router] is already a table; stdout: {preview_out}"
    );
    assert_eq!(
        fs::read(&config).expect("dry-run should leave the file readable"),
        written,
        "dry-run on an existing table should not change the bytes"
    );
    remove_temp(&dir);
}

#[test]
fn init_dry_run_leaves_a_missing_path_missing() {
    let dir = temp_root("dry-run");
    let config = dir.join("config.toml");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["--config", &arg, "init", "--dry-run"]);
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "dry-run should succeed; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stdout.contains(&arg)
            && stdout.contains("would write")
            && !stdout.contains("would write nothing"),
        "dry-run should name the path and that it would write; stdout: {stdout}"
    );
    assert!(
        !config.exists(),
        "dry-run should leave a missing path missing"
    );
    let quiet = run(&dir, &["--config", &arg, "--quiet", "init", "--dry-run"]);
    assert_eq!(
        stdout_text(&quiet),
        stdout,
        "quiet should print the same path and dry-run status as text"
    );
    assert!(
        !config.exists(),
        "a quiet dry-run should also leave the path missing"
    );
    remove_temp(&dir);
}

#[test]
fn init_uses_home_grok_config_when_config_is_omitted() {
    let dir = temp_root("home");
    let output = command(&dir)
        .args(["init"])
        .output()
        .expect("the grok-router binary should start");
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "init without --config should write under HOME; stderr: {stderr}; stdout: {stdout}"
    );
    let config = dir.join(".grok").join("config.toml");
    assert_eq!(
        fs::read_to_string(&config).expect("HOME/.grok/config.toml should be created"),
        BLOCK,
        "the default path should be $HOME/.grok/config.toml inside the temp home"
    );
    assert!(
        stdout.contains(&utf8_arg(&config, "the home config path should be UTF-8")),
        "text output should name the home path; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn init_treats_an_empty_grok_home_as_unset_and_prefers_a_set_one() {
    let dir = temp_root("grok-home");
    let empty = command(&dir)
        .env("GROK_HOME", "")
        .args(["init"])
        .output()
        .expect("the grok-router binary should start");
    assert_eq!(
        exit_code(&empty),
        0,
        "an empty GROK_HOME should fall through to HOME; stderr: {}",
        stderr_text(&empty)
    );
    let home_config = dir.join(".grok").join("config.toml");
    assert_eq!(
        fs::read_to_string(&home_config).expect("empty GROK_HOME should still write under HOME"),
        BLOCK
    );
    assert!(
        !dir.join("config.toml").exists(),
        "an empty GROK_HOME should not be used as a relative config directory"
    );

    let other = temp_root("grok-home-set");
    let home = other.join("home");
    let grok_home = other.join("grok-home");
    fs::create_dir_all(&home).expect("the alternate home should be created");
    let set = command(&other)
        .env("HOME", &home)
        .env("GROK_HOME", &grok_home)
        .args(["init"])
        .output()
        .expect("the grok-router binary should start");
    assert_eq!(
        exit_code(&set),
        0,
        "a set GROK_HOME should be the config directory; stderr: {}",
        stderr_text(&set)
    );
    assert_eq!(
        fs::read_to_string(grok_home.join("config.toml"))
            .expect("GROK_HOME/config.toml should be created"),
        BLOCK
    );
    assert!(
        !home.join(".grok").exists(),
        "a set GROK_HOME should not also write $HOME/.grok"
    );
    remove_temp(&dir);
    remove_temp(&other);
}

#[test]
fn init_does_not_print_an_openrouter_key_value() {
    let dir = temp_root("secret");
    let config = dir.join("config.toml");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let sentinel = "zq-init-sentinel-7c2e";
    assert!(
        !BLOCK.contains(sentinel),
        "the sentinel must not already be part of the block"
    );
    let output = command(&dir)
        .env("OPENROUTER_API_KEY", sentinel)
        .args(["--config", &arg, "init"])
        .output()
        .expect("the grok-router binary should start");
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "a key value that is not in the block should not refuse the write; stderr: {stderr}"
    );
    let written = fs::read_to_string(&config).expect("the config should be written");
    assert!(
        !written.contains(sentinel),
        "the written file should not contain the key value"
    );
    assert!(
        !stdout.contains(sentinel),
        "stdout should not contain the key value; stdout: {stdout}"
    );
    assert!(
        !stderr.contains(sentinel),
        "stderr should not contain the key value; stderr: {stderr}"
    );
    assert_eq!(written, BLOCK);
    remove_temp(&dir);
}

#[test]
fn init_refuses_a_router_key_that_is_not_a_table_and_writes_nothing() {
    let dir = temp_root("not-table");
    let config = dir.join("config.toml");
    let original = "router = \"nope\"\n";
    fs::write(&config, original).expect("the non-table fixture should be written");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["--config", &arg, "init"]);
    let stderr = stderr_text(&output);
    assert_ne!(
        exit_code(&output),
        0,
        "a non-table [router] key should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("not a table")
            && stderr.contains("left unchanged")
            && stderr.contains("nothing was written"),
        "the refusal should name the key, the unchanged file, and the missing write; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("nope"),
        "the refusal should not need to echo the router value; stderr: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the refused file should stay readable"),
        original,
        "a non-table router key should leave the bytes unchanged"
    );
    assert!(
        !backup_path(&config).exists(),
        "a refusal should not take a backup"
    );
    let json = run(&dir, &["--config", &arg, "--output", "json", "init"]);
    let json_err = stderr_text(&json);
    let body: serde_json::Value = serde_json::from_str(json_err.trim())
        .expect("a json refusal should be one JSON object on stderr");
    assert_eq!(
        body.get("command").and_then(|value| value.as_str()),
        Some("init")
    );
    assert_eq!(
        body.get("format").and_then(|value| value.as_str()),
        Some("json")
    );
    assert_eq!(
        body.get("error").and_then(|value| value.as_str()),
        Some("init_router_not_table")
    );
    assert_eq!(
        body.get("kind").and_then(|value| value.as_str()),
        Some("init_router_not_table")
    );
    assert!(
        body.get("message")
            .and_then(|value| value.as_str())
            .is_some()
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the json refusal should leave the file"),
        original
    );
    remove_temp(&dir);
}

#[test]
fn init_refuses_an_unparsed_file_and_an_empty_config_flag() {
    let dir = temp_root("refuse");
    let config = dir.join("config.toml");
    let broken = "[models\n";
    fs::write(&config, broken).expect("the broken fixture should be written");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let parsed = run(&dir, &["--config", &arg, "init"]);
    let parsed_err = stderr_text(&parsed);
    assert_ne!(exit_code(&parsed), 0, "an unparsed config should refuse");
    assert!(
        parsed_err.contains("did not parse")
            && parsed_err.contains("left unchanged")
            && parsed_err.contains("nothing was written"),
        "a parse refusal should name the problem, the unchanged file, and the next edit; stderr: {parsed_err}"
    );
    assert_eq!(
        fs::read_to_string(&config).expect("the broken file should stay"),
        broken
    );
    assert!(
        !backup_path(&config).exists(),
        "a parse refusal should not take a backup"
    );

    let empty = run(&dir, &["--config", "", "init"]);
    let empty_err = stderr_text(&empty);
    assert_ne!(exit_code(&empty), 0, "an empty --config should refuse");
    assert!(
        empty_err.contains("--config is empty") && empty_err.contains("nothing was written"),
        "an empty --config should name the flag and that nothing was written; stderr: {empty_err}"
    );
    remove_temp(&dir);
}

#[test]
fn init_refuses_when_home_is_missing_and_writes_nothing() {
    let dir = temp_root("no-home");
    let unset = command(&dir)
        .env_remove("HOME")
        .env_remove("GROK_HOME")
        .args(["init"])
        .output()
        .expect("the grok-router binary should start");
    let unset_err = stderr_text(&unset);
    assert_ne!(
        exit_code(&unset),
        0,
        "a missing HOME should refuse; stdout: {}",
        stdout_text(&unset)
    );
    assert!(
        unset_err.contains("HOME is not set") && unset_err.contains("Nothing was written"),
        "a missing HOME should name HOME and that nothing was written; stderr: {unset_err}"
    );
    assert!(
        !dir.join(".grok").exists() && !dir.join("config.toml").exists(),
        "a missing HOME should not create a config"
    );

    let empty = command(&dir)
        .env("HOME", "")
        .env_remove("GROK_HOME")
        .args(["init"])
        .output()
        .expect("the grok-router binary should start");
    let empty_err = stderr_text(&empty);
    assert_ne!(exit_code(&empty), 0, "an empty HOME should refuse");
    assert!(
        empty_err.contains("HOME is empty") && empty_err.contains("Nothing was written"),
        "an empty HOME should name that and write nothing; stderr: {empty_err}"
    );
    assert!(
        !dir.join(".grok").exists() && !dir.join("config.toml").exists(),
        "an empty HOME should not create a config"
    );
    remove_temp(&dir);
}
