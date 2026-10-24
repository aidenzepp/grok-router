#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! `grok-router search` reads a catalog fixture or the cache for a resolved base.
//!
//! `--catalog-file` returns before the config is read. Without it, search reads
//! `--config` or `${GROK_HOME:-$HOME/.grok}/config.toml` and does not create
//! that file. These tests do not call the network and do not read
//! `OPENROUTER_API_KEY`. Cache and config paths point at a temp directory.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

const FIXTURE: &str = r#"{
    "data": [
        {
            "id": "gpt-4",
            "name": "GPT-4",
            "description": "Older flagship",
            "context_length": 8192,
            "architecture": { "input_modalities": ["text"] }
        },
        {
            "id": "gpt-4-turbo",
            "name": "GPT-4 Turbo",
            "description": "Sees pictures",
            "context_length": 128000,
            "architecture": { "input_modalities": ["text", "image"] }
        }
    ]
}"#;

fn remove_temp(path: &std::path::Path) {
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
        "grok-router-search-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp search directory should be created");
    path
}

fn write_fixture(dir: &std::path::Path) -> PathBuf {
    let path = dir.join("models.json");
    fs::write(&path, FIXTURE).expect("the catalog fixture should be written");
    path
}

fn write_config(dir: &std::path::Path, text: &str) -> PathBuf {
    let path = dir.join("config.toml");
    fs::write(&path, text).expect("the config fixture should be written");
    path
}

fn mark_fresh(path: &std::path::Path) {
    let file = fs::File::options()
        .write(true)
        .open(path)
        .expect("the cache fixture should open so its age can be set");
    let modified = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(60))
        .expect("one minute before now should be representable");
    file.set_modified(modified)
        .expect("the cache fixture should be marked younger than one hour");
}

fn assert_dir_empty(path: &std::path::Path) {
    let mut entries = fs::read_dir(path).expect("the cache directory should be readable");
    assert!(
        entries.next().is_none(),
        "the cache directory {} should contain no files",
        path.display()
    );
}

fn run(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grok-router"));
    command
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .env("HOME", dir)
        .env("NO_COLOR", "1")
        .env("GROK_HOME", dir)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("search stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("search stderr should be UTF-8")
}

#[test]
fn search_help_contains_an_example_and_says_it_does_not_modify_the_config() {
    let dir = temp_root("help");
    let output = run(&dir, &["search", "--help"]);
    let help = stdout_text(&output);
    assert!(
        output.status.success(),
        "search --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.to_lowercase().contains("example"),
        "search --help should contain an example; help: {help}"
    );
    assert!(
        help.contains("grok-router search --catalog-file models.json glm"),
        "search --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("does not modify the config"),
        "search --help should say the command does not modify the config; help: {help}"
    );
    remove_temp(&dir);
}

#[test]
fn search_json_parses_and_uses_the_fixture_without_a_key_or_a_network() {
    let dir = temp_root("json");
    let fixture = write_fixture(&dir);
    let config = dir.join("config.toml");
    fs::write(&config, "untouched = true\n").expect("the config fixture should be written");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "search",
            "--catalog-file",
            fixture.to_str().expect("the fixture path should be UTF-8"),
            "--cache-dir",
            cache.to_str().expect("the cache path should be UTF-8"),
            "--json",
            "gpt",
            "turbo",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert!(
        output.status.success(),
        "a unique fixture match should succeed; stderr: {stderr}"
    );
    assert!(
        stderr.is_empty(),
        "a successful search should keep diagnostics off stdout's sibling; stderr: {stderr}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("search --json should parse as JSON");
    assert_eq!(
        parsed,
        serde_json::json!([{
            "id": "gpt-4-turbo",
            "name": "GPT-4 Turbo",
            "context_length": 128000
        }]),
        "json output should be the matched row and nothing else"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "json output must not contain color escapes; stdout: {stdout}"
    );
    let config_after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(
        config_after,
        b"untouched = true\n",
        "search must not write the config"
    );
    assert!(
        cache.read_dir().expect("the cache dir should be readable").next().is_none(),
        "a --catalog-file search must not write the cache"
    );
    remove_temp(&dir);
}

#[test]
fn search_table_lists_id_name_and_context_length() {
    let dir = temp_root("table");
    let fixture = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "search",
            "--catalog-file",
            fixture.to_str().expect("the fixture path should be UTF-8"),
            "gpt",
        ],
    );
    let stdout = stdout_text(&output);
    assert!(
        output.status.success(),
        "search should print the table; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("ID") && stdout.contains("NAME") && stdout.contains("CONTEXT"),
        "the table should name its columns; stdout: {stdout}"
    );
    assert!(
        stdout.contains("gpt-4") && stdout.contains("8192") && stdout.contains("GPT-4 Turbo"),
        "the table should include both gpt rows; stdout: {stdout}"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "piped output must not contain color escapes; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn search_quiet_prints_one_id_per_line_without_a_header() {
    let dir = temp_root("quiet");
    let fixture = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "search",
            "--quiet",
            "--catalog-file",
            fixture.to_str().expect("the fixture path should be UTF-8"),
            "gpt",
        ],
    );
    let stdout = stdout_text(&output);
    assert!(
        output.status.success(),
        "quiet search should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout, "gpt-4\ngpt-4-turbo\n");
    assert!(
        !stdout.contains("CONTEXT"),
        "quiet output should not include the table header; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn search_limit_caps_the_table() {
    let dir = temp_root("limit");
    let fixture = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "search",
            "--limit",
            "1",
            "--catalog-file",
            fixture.to_str().expect("the fixture path should be UTF-8"),
            "--json",
            "gpt",
        ],
    );
    let stdout = stdout_text(&output);
    assert!(
        output.status.success(),
        "a limited search should succeed; stderr: {}",
        stderr_text(&output)
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("limited --json should parse");
    let rows = parsed
        .as_array()
        .expect("limited --json should be an array");
    assert_eq!(rows.len(), 1, "limit 1 should print one row; stdout: {stdout}");
    remove_temp(&dir);
}

#[test]
fn an_empty_search_fails_and_names_the_count_and_the_next_step() {
    let dir = temp_root("empty");
    let fixture = write_fixture(&dir);
    let config = dir.join("config.toml");
    fs::write(&config, "untouched = true\n").expect("the config fixture should be written");
    let output = run(
        &dir,
        &[
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "search",
            "--catalog-file",
            fixture.to_str().expect("the fixture path should be UTF-8"),
            "missing-model",
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "zero matches should fail; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("0 matches"),
        "the failure should name the count; stderr: {stderr}"
    );
    assert!(
        stderr.contains("fewer terms"),
        "the failure should name the next step; stderr: {stderr}"
    );
    let config_after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(config_after, b"untouched = true\n");
    remove_temp(&dir);
}

const BAD_BASE: &str = "[grok_router]\nopenrouter_base_url = \"ftp://nope\"\n";

const CUSTOM_BASE: &str = "[grok_router]\nopenrouter_base_url = \"https://example.invalid/v1\"\n";

const HEX_CATALOG: &str = r#"{
    "data": [
        {
            "id": "hex-cache/only",
            "name": "Hex Cache",
            "description": "row stored for the custom base",
            "context_length": 1024,
            "architecture": { "input_modalities": ["text"] }
        }
    ]
}"#;

#[test]
fn a_missing_config_reads_fresh_models_json_and_does_not_create_the_file() {
    let dir = temp_root("missing-config");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let models = cache.join("models.json");
    fs::write(&models, FIXTURE).expect("the fresh models.json should be written");
    mark_fresh(&models);
    let before = fs::read(&models).expect("models.json should be readable before search");
    let output = run(
        &dir,
        &[
            "search",
            "--cache-dir",
            cache.to_str().expect("the cache path should be UTF-8"),
            "--json",
            "turbo",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert!(
        output.status.success(),
        "a missing config should still search the fresh default cache; stderr: {stderr}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("search --json should parse as JSON");
    assert_eq!(
        parsed,
        serde_json::json!([{
            "id": "gpt-4-turbo",
            "name": "GPT-4 Turbo",
            "context_length": 128000
        }]),
        "the fresh models.json should supply the row; stdout: {stdout}"
    );
    assert!(
        !dir.join("config.toml").exists(),
        "search must not create the default config at {}",
        dir.join("config.toml").display()
    );
    assert!(
        !dir.join(".grok").join("config.toml").exists(),
        "search must not create a home config when GROK_HOME is set"
    );
    let after = fs::read(&models).expect("models.json should still exist");
    assert_eq!(before, after, "a cache hit must not rewrite models.json");
    remove_temp(&dir);
}

#[test]
fn an_invalid_base_url_fails_and_leaves_the_empty_cache_untouched() {
    let dir = temp_root("bad-base");
    let config = write_config(&dir, BAD_BASE);
    let before = fs::read(&config).expect("the bad config should be readable");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "search",
            "--cache-dir",
            cache.to_str().expect("the cache path should be UTF-8"),
            "gpt",
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "an invalid openrouter_base_url should fail before a download; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("openrouter_base_url"),
        "the failure should name openrouter_base_url; stderr: {stderr}"
    );
    assert_dir_empty(&cache);
    let after = fs::read(&config).expect("the bad config should still exist");
    assert_eq!(before, after, "search must not write the config");
    remove_temp(&dir);
}

#[test]
fn catalog_file_still_succeeds_when_the_config_base_url_is_invalid() {
    let dir = temp_root("bad-base-fixture");
    let config = write_config(&dir, BAD_BASE);
    let before = fs::read(&config).expect("the bad config should be readable");
    let fixture = write_fixture(&dir);
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "search",
            "--catalog-file",
            fixture.to_str().expect("the fixture path should be UTF-8"),
            "--cache-dir",
            cache.to_str().expect("the cache path should be UTF-8"),
            "--json",
            "turbo",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert!(
        output.status.success(),
        "--catalog-file should ignore an invalid base and read the fixture; stderr: {stderr}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("search --json should parse as JSON");
    assert_eq!(
        parsed,
        serde_json::json!([{
            "id": "gpt-4-turbo",
            "name": "GPT-4 Turbo",
            "context_length": 128000
        }]),
        "the fixture row should be returned, not a cache or a download; stdout: {stdout}"
    );
    assert_dir_empty(&cache);
    let after = fs::read(&config).expect("the bad config should still exist");
    assert_eq!(before, after, "search must not write the config");
    remove_temp(&dir);
}

#[test]
fn a_custom_base_returns_the_hex_cache_row_and_not_a_decoy() {
    let dir = temp_root("custom-base");
    let config = write_config(&dir, CUSTOM_BASE);
    let config_before = fs::read(&config).expect("the custom-base config should be readable");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let base = "https://example.invalid/v1";
    let models_url = grok_router::catalog::models_url(base);
    let hex_path = grok_router::catalog::cache_file_for(&cache, &models_url);
    let name = hex_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .expect("the hex cache name should be Unicode");
    assert_ne!(
        name, "models.json",
        "a custom base must not be stored as models.json; name: {name}"
    );
    fs::write(&hex_path, HEX_CATALOG).expect("the hex cache fixture should be written");
    mark_fresh(&hex_path);
    let hex_before = fs::read(&hex_path).expect("the hex cache should be readable before search");
    assert!(
        !cache.join("models.json").exists(),
        "this case has no models.json, so a default-base read has nothing to return"
    );
    let output = run(
        &dir,
        &[
            "--config",
            config.to_str().expect("the config path should be UTF-8"),
            "search",
            "--cache-dir",
            cache.to_str().expect("the cache path should be UTF-8"),
            "--json",
            "hex-cache",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert!(
        output.status.success(),
        "search should return the hex cache row for the configured base; stderr: {stderr}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("search --json should parse as JSON");
    assert_eq!(
        parsed,
        serde_json::json!([{
            "id": "hex-cache/only",
            "name": "Hex Cache",
            "context_length": 1024
        }]),
        "the hex file's row should be returned, not a decoy; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("gpt-4") && !stdout.contains("decoy"),
        "the result should not contain a default-catalog or decoy row; stdout: {stdout}"
    );
    assert!(
        !cache.join("models.json").exists(),
        "search must not create models.json for a custom base"
    );
    let hex_after = fs::read(&hex_path).expect("the hex cache should still exist");
    assert_eq!(hex_before, hex_after, "a cache hit must not rewrite the hex file");
    let config_after = fs::read(&config).expect("the custom-base config should still exist");
    assert_eq!(
        config_before, config_after,
        "search must not write the config"
    );
    remove_temp(&dir);
}
