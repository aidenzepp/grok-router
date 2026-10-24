#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! `grok-router add` writes one model into a temp config.
//!
//! These tests pass `--catalog-file` and `--config`. They do not call the
//! network, do not read `OPENROUTER_API_KEY`, and do not read a home config.

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

const CATALOG: &str = r#"{
    "data": [
        {
            "id": "z-ai/glm-5.3",
            "name": "GLM 5.3",
            "description": "already configured",
            "context_length": 1048576,
            "architecture": { "input_modalities": ["text"] }
        },
        {
            "id": "acme/widget.v2",
            "name": "Widget",
            "description": "a different provider",
            "context_length": 4096,
            "architecture": { "input_modalities": ["text", "image"] }
        },
        {
            "id": "acme/other.v2",
            "name": "Other",
            "description": "another widget",
            "context_length": 2048,
            "architecture": { "input_modalities": ["text"] }
        }
    ]
}"#;

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
        "grok-router-add-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp add directory should be created");
    path
}

fn write_fixture(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let config = dir.join("config.toml");
    let catalog = dir.join("models.json");
    let cache = dir.join("cache");
    fs::write(&config, config_text()).expect("the config fixture should be written");
    fs::write(&catalog, CATALOG).expect("the catalog fixture should be written");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    (config, catalog, cache)
}

fn utf8_arg(path: &Path, what: &str) -> String {
    path.to_str()
        .expect(what)
        .to_owned()
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
    String::from_utf8(output.stdout.clone()).expect("add stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("add stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn assert_preserved_spans(text: &str) {
    assert!(
        text.contains(COMMENT),
        "the comment should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains(DEEPSEEK_SPAN),
        "the deepseek span should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains(GLM_SPAN),
        "the or-glm-5-3 span, including context_window = 600000, should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("default_reasoning_effort = \"high\""),
        "the reasoning-effort line should stay; config: {text}"
    );
}

fn model_headers(text: &str) -> usize {
    text.lines()
        .filter(|line| line.starts_with("[model."))
        .count()
}

#[test]
fn add_help_contains_an_example_and_says_it_writes_unless_dry_run() {
    let dir = temp_root("help");
    let output = run(&dir, &["add", "--help"]);
    let help = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "add --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.to_lowercase().contains("example"),
        "add --help should contain an example; help: {help}"
    );
    assert!(
        help.contains("grok-router add --catalog-file models.json --config config.toml z-ai/glm-5.3"),
        "add --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("writes unless --dry-run"),
        "add --help should say the command writes unless --dry-run is set; help: {help}"
    );
    remove_temp(&dir);
}

#[test]
fn a_named_add_inserts_one_block_and_leaves_existing_spans_unchanged() {
    let dir = temp_root("insert");
    let (config, catalog, cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--refresh",
            "acme/widget.v2",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "a named add should exit 0; stderr: {stderr}; stdout: {stdout}"
    );
    assert!(
        stderr.is_empty(),
        "a successful add should not prompt or write diagnostics; stderr: {stderr}"
    );
    assert!(
        !stdout.contains('?') && !stdout.to_lowercase().contains("confirm"),
        "stdout should not ask for confirmation; stdout: {stdout}"
    );
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert_ne!(
        after.as_bytes(),
        before.as_slice(),
        "the add should insert a block"
    );
    assert_eq!(
        model_headers(&after),
        model_headers(&config_text()).saturating_add(1),
        "the add should insert one model block; config: {after}"
    );
    assert!(
        after.contains("[model.or-widget-v2]"),
        "the derived slug should be the new header; config: {after}"
    );
    assert_preserved_spans(&after);
    assert!(
        cache
            .read_dir()
            .expect("the cache dir should be readable")
            .next()
            .is_none(),
        "a --catalog-file add must not write the cache or use the network"
    );
    remove_temp(&dir);
}

#[test]
fn a_second_slug_for_the_same_provider_exits_1_and_leaves_the_file() {
    let dir = temp_root("second-slug");
    let (config, catalog, _cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--as",
            "or-glm-copy",
            "z-ai/glm-5.3",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "a second slug for the same provider should exit 1; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("z-ai/glm-5.3") && stderr.contains("or-glm-5-3"),
        "the failure should name the provider and the existing slug; stderr: {stderr}"
    );
    assert!(
        stderr.contains("remove") || stderr.contains("Keep"),
        "the failure should say what to do next; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(after, before, "a refused add should leave the file unchanged");
    assert!(
        !config.with_extension("toml.prev").exists() && !config.with_file_name("config.toml.prev").exists(),
        "a refused add should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn the_same_slug_and_provider_exits_0_and_leaves_the_bytes() {
    let dir = temp_root("same-slug");
    let (config, catalog, _cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "z-ai/glm-5.3",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "the same slug and provider should be a no-op success; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout_text(&output), "or-glm-5-3\n");
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(after, before, "a no-op add should leave the bytes unchanged");
    assert!(
        !config.with_file_name("config.toml.prev").exists(),
        "a no-op add should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn default_sets_the_default_in_the_same_write() {
    let dir = temp_root("default");
    let (config, catalog, _cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--default",
            "acme/widget.v2",
        ],
    );
    assert_eq!(
        exit_code(&output),
        0,
        "--default should succeed in one write; stderr: {}",
        stderr_text(&output)
    );
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        after.contains("default = \"or-widget-v2\""),
        "the same write should set the default; config: {after}"
    );
    assert!(
        after.contains("[model.or-widget-v2]"),
        "the same write should insert the model; config: {after}"
    );
    assert_preserved_spans(&after);
    let backup = config.with_file_name("config.toml.prev");
    let backed_up = fs::read(&backup).expect("one write should leave the original bytes as the sibling backup");
    assert_eq!(
        backed_up, before,
        "the backup should be the original file, which means the insert and the default were one write"
    );
    remove_temp(&dir);
}

#[test]
fn dry_run_prints_the_slug_and_would_write_and_changes_nothing() {
    let dir = temp_root("dry-run");
    let (config, catalog, _cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--dry-run",
            "acme/widget.v2",
        ],
    );
    let stdout = stdout_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "--dry-run should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("or-widget-v2"),
        "dry-run should print the slug; stdout: {stdout}"
    );
    assert!(
        stdout.contains("would write"),
        "dry-run should say that it would write; stdout: {stdout}"
    );
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(after, before, "--dry-run should leave the file bytes identical");
    assert!(
        !config.with_file_name("config.toml.prev").exists(),
        "--dry-run should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn json_prints_one_object_and_quiet_prints_the_slug_only() {
    let dir = temp_root("json");
    let (config, catalog, _cache) = write_fixture(&dir);
    let json_out = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--json",
            "acme/widget.v2",
        ],
    );
    let stdout = stdout_text(&json_out);
    let stderr = stderr_text(&json_out);
    assert_eq!(exit_code(&json_out), 0, "json add should succeed; stderr: {stderr}");
    assert!(stderr.is_empty(), "diagnostics should stay off stdout; stderr: {stderr}");
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("add --json should be one JSON value");
    assert!(parsed.is_object(), "add --json should print one object; stdout: {stdout}");
    assert_eq!(
        parsed.get("slug").and_then(serde_json::Value::as_str),
        Some("or-widget-v2"),
        "the object should name the slug; stdout: {stdout}"
    );
    assert_eq!(
        parsed.get("wrote").and_then(serde_json::Value::as_bool),
        Some(true),
        "the object should say the file was written; stdout: {stdout}"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "json output must not contain color escapes; stdout: {stdout}"
    );

    let quiet = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--quiet",
            "acme/widget.v2",
        ],
    );
    assert_eq!(
        exit_code(&quiet),
        0,
        "a second add of the same slug should stay quiet and succeed; stderr: {}",
        stderr_text(&quiet)
    );
    assert_eq!(stdout_text(&quiet), "or-widget-v2\n");
    remove_temp(&dir);
}

#[test]
fn zero_matches_and_several_matches_exit_1_without_writing() {
    let dir = temp_root("resolve");
    let (config, catalog, _cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let missing = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "missing-model",
        ],
    );
    let missing_err = stderr_text(&missing);
    assert_eq!(
        exit_code(&missing),
        1,
        "zero matches should exit 1; stdout: {}",
        stdout_text(&missing)
    );
    assert!(
        missing_err.contains("0 matches"),
        "zero matches should name the count; stderr: {missing_err}"
    );
    assert!(
        missing_err.contains("--catalog-file") || missing_err.contains("fewer"),
        "zero matches should say what to do next; stderr: {missing_err}"
    );

    let many = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "widget",
        ],
    );
    let many_err = stderr_text(&many);
    assert_eq!(
        exit_code(&many),
        1,
        "several matches should exit 1; stdout: {}",
        stdout_text(&many)
    );
    assert!(
        many_err.contains("2 matches"),
        "several matches should name the count; stderr: {many_err}"
    );
    assert!(
        many_err.contains("provider id"),
        "several matches should say what to do next; stderr: {many_err}"
    );
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(after, before, "a failed resolve should leave the file unchanged");
    remove_temp(&dir);
}
