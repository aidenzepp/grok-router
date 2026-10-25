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

//! `grok-router search` resolves one `[router]` provider record, selects its
//! cartridge, and then reads a catalog fixture or that record's cache.
//!
//! `--catalog-file` does not skip resolution and does not GET. A missing
//! config file is not created and is not the compiled `OpenRouter` base. These
//! tests do not call the network and do not read provider env vars. Cache and
//! config paths point at a temp directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPILED_URL: &str = "https://openrouter.ai/api/v1";

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

const DEEPSEEK_DOC: &str = r#"{
    "data": [
        {
            "id": "deepseek-v4-pro",
            "name": "DeepSeek V4 Pro",
            "context_window": 128000,
            "input_modalities": ["text"]
        }
    ]
}"#;

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
        std::env::temp_dir().join(format!("grok-router-search-{label}-{}", std::process::id()));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp search directory should be created");
    path
}

fn write_named(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).expect("the named fixture should be written");
    path
}

fn write_config(dir: &Path, text: &str) -> PathBuf {
    write_named(dir, "config.toml", text)
}

fn mark_fresh(path: &Path) {
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

fn assert_dir_empty(path: &Path) {
    let mut entries = fs::read_dir(path).expect("the cache directory should be readable");
    assert!(
        entries.next().is_none(),
        "the cache directory {} should contain no files",
        path.display()
    );
}

fn utf8_arg(path: &Path, why: &str) -> String {
    path.to_str().expect(why).to_owned()
}

fn provider_record(
    default_line: &str,
    name: &str,
    base: &str,
    env_key: &str,
    catalog: Option<&str>,
) -> String {
    let catalog_line = match catalog {
        Some(id) => format!("catalog = \"{id}\"\n"),
        None => String::new(),
    };
    format!(
        "[router]\n{default_line}\n[router.provider.{name}]\nbase_url = \"{base}\"\nenv_key = \"{env_key}\"\n{catalog_line}"
    )
}

fn openrouter_config() -> String {
    provider_record(
        "default_provider = \"openrouter\"\n",
        "openrouter",
        COMPILED_URL,
        "OPENROUTER_API_KEY",
        Some("openrouter-models"),
    )
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grok-router"));
    command
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("DEEPSEEK_API_KEY")
        .env_remove("ZAI_API_KEY")
        .env_remove("GROK_ROUTER_OUTPUT")
        .env_remove("GROK_ROUTER_QUIET")
        .env_remove("GROK_ROUTER_TIMEOUT")
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

/// Human sentence. JSON errors carry it in `message`; text errors are the raw stderr.
fn refusal_sentence(stderr: &str) -> String {
    serde_json::from_str::<serde_json::Value>(stderr.trim())
        .ok()
        .and_then(|value| {
            value
                .get("message")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| stderr.to_owned())
}

fn assert_missing_router(stderr: &str, situation: &str) {
    assert!(
        stderr.contains("[router]"),
        "{situation} should name the missing [router] data; stderr: {stderr}"
    );
    assert!(
        stderr.contains("must write that record"),
        "{situation} should say the operator must write that record; stderr: {stderr}"
    );
    assert!(
        !stderr.contains(COMPILED_URL),
        "{situation} must not choose the compiled OpenRouter URL; stderr: {stderr}"
    );
    let sentence = refusal_sentence(stderr);
    assert!(
        !sentence.to_ascii_lowercase().contains("json"),
        "{situation} must not report a JSON parse; stderr: {stderr}"
    );
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
        help.contains("grok-router search --catalog-file models.json --config config.toml glm"),
        "search --help should contain the config example; help: {help}"
    );
    assert!(
        help.contains("grok-router search --provider deepseek --catalog-file deepseek.json --config config.toml deepseek"),
        "search --help should contain the provider example; help: {help}"
    );
    assert!(
        help.contains("--provider"),
        "search --help should document --provider; help: {help}"
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
    let fixture = write_named(&dir, "models.json", FIXTURE);
    let config = write_config(&dir, &openrouter_config());
    let before = fs::read(&config).expect("the config fixture should be readable");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&fixture, "the fixture path should be UTF-8"),
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
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
            "context_length": 128_000
        }]),
        "json output should keep the context_length key; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("context_window"),
        "json output must not rename context_length; stdout: {stdout}"
    );
    assert!(
        !stdout.contains('\u{1b}'),
        "json output must not contain color escapes; stdout: {stdout}"
    );
    let config_after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(config_after, before, "search must not write the config");
    assert_dir_empty(&cache);
    remove_temp(&dir);
}

#[test]
fn search_table_lists_id_name_and_context_length() {
    let dir = temp_root("table");
    let fixture = write_named(&dir, "models.json", FIXTURE);
    let config = write_config(&dir, &openrouter_config());
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&fixture, "the fixture path should be UTF-8"),
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
    let fixture = write_named(&dir, "models.json", FIXTURE);
    let config = write_config(&dir, &openrouter_config());
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--quiet",
            "--catalog-file",
            &utf8_arg(&fixture, "the fixture path should be UTF-8"),
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
    let fixture = write_named(&dir, "models.json", FIXTURE);
    let config = write_config(&dir, &openrouter_config());
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--limit",
            "1",
            "--catalog-file",
            &utf8_arg(&fixture, "the fixture path should be UTF-8"),
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
    assert_eq!(
        rows.len(),
        1,
        "limit 1 should print one row; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn an_empty_search_fails_and_names_the_count_and_the_next_step() {
    let dir = temp_root("empty");
    let fixture = write_named(&dir, "models.json", FIXTURE);
    let config = write_config(&dir, &openrouter_config());
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&fixture, "the fixture path should be UTF-8"),
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
    assert_eq!(
        config_after, before,
        "an empty search must not write the config"
    );
    remove_temp(&dir);
}

#[test]
fn a_missing_config_file_refuses_and_is_not_created() {
    let dir = temp_root("missing-config");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let models = cache.join("models.json");
    fs::write(&models, FIXTURE).expect("the fresh models.json should be written");
    mark_fresh(&models);
    let before = fs::read(&models).expect("models.json should be readable before search");
    let omitted = run(
        &dir,
        &[
            "search",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--json",
            "turbo",
        ],
    );
    let omitted_err = stderr_text(&omitted);
    assert!(
        !omitted.status.success(),
        "a missing default config should refuse; stdout: {}",
        stdout_text(&omitted)
    );
    assert_missing_router(&omitted_err, "a missing default config");
    assert!(
        !stdout_text(&omitted).contains("gpt-4"),
        "a missing config must not print a cache row; stdout: {}",
        stdout_text(&omitted)
    );
    let explicit_path = dir.join("absent.toml");
    let explicit = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&explicit_path, "the missing config path should be UTF-8"),
            "search",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "turbo",
        ],
    );
    assert!(
        !explicit.status.success(),
        "a missing --config path should refuse; stdout: {}",
        stdout_text(&explicit)
    );
    assert_missing_router(&stderr_text(&explicit), "a missing --config path");
    assert!(
        !dir.join("config.toml").exists(),
        "search must not create the default config"
    );
    assert!(
        !explicit_path.exists(),
        "search must not create the named missing config"
    );
    assert!(
        !dir.join(".grok").join("config.toml").exists(),
        "search must not create a home config when GROK_HOME is set"
    );
    let after = fs::read(&models).expect("models.json should still exist");
    assert_eq!(before, after, "a refusal must not rewrite models.json");
    remove_temp(&dir);
}

#[test]
fn a_config_without_router_refuses_before_parsing_the_catalog_file() {
    let dir = temp_root("no-router");
    let config = write_config(&dir, "untouched = true\n");
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", "this is not json");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--json",
            "gpt",
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "a config with no [router] should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert_missing_router(&stderr, "a config with no [router]");
    assert!(
        stdout_text(&output).trim().is_empty(),
        "a refused search should not print a row; stdout: {}",
        stdout_text(&output)
    );
    assert_dir_empty(&cache);
    assert_eq!(
        fs::read(&config).expect("the config should still exist"),
        before,
        "search must not write the config"
    );
    remove_temp(&dir);
}

#[test]
fn only_a_grok_router_table_refuses_and_does_not_use_the_compiled_url() {
    let dir = temp_root("grok-router-only");
    let config = write_config(
        &dir,
        "[grok_router]\nopenrouter_base_url = \"https://openrouter.ai/api/v1\"\n",
    );
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", "this is not json");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "gpt",
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "only [grok_router] should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert_missing_router(&stderr, "only [grok_router]");
    assert_dir_empty(&cache);
    assert_eq!(
        fs::read(&config).expect("the config should still exist"),
        before
    );
    remove_temp(&dir);
}

#[test]
fn explicit_provider_does_not_need_default_provider() {
    let dir = temp_root("explicit-provider");
    let source = provider_record(
        "",
        "deepseek",
        "https://api.deepseek.com",
        "DEEPSEEK_API_KEY",
        Some("deepseek-list-models"),
    );
    let config = write_config(&dir, &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "deepseek.json", DEEPSEEK_DOC);
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let added = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--quiet",
            "deepseek-v4-pro",
        ],
    );
    assert!(
        added.status.success(),
        "--provider deepseek should search without default_provider; stderr: {}",
        stderr_text(&added)
    );
    assert_eq!(stdout_text(&added), "deepseek-v4-pro\n");
    assert_dir_empty(&cache);
    let omitted = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--quiet",
            "deepseek-v4-pro",
        ],
    );
    let stderr = stderr_text(&omitted);
    assert!(
        !omitted.status.success(),
        "omitted --provider should not pick a record; stdout: {}",
        stdout_text(&omitted)
    );
    assert!(
        stderr.contains("default_provider"),
        "the refusal should name default_provider; stderr: {stderr}"
    );
    assert!(
        !stdout_text(&omitted).contains("deepseek-v4-pro"),
        "the omitted provider must not print the fixture id; stdout: {}",
        stdout_text(&omitted)
    );
    assert_eq!(
        fs::read(&config).expect("the config should still exist"),
        before,
        "search must not write the config"
    );
    remove_temp(&dir);
}

#[test]
fn blank_default_provider_still_honors_an_explicit_provider() {
    let dir = temp_root("blank-default");
    let source = provider_record(
        "default_provider = \"\"\n",
        "deepseek",
        "https://api.deepseek.com",
        "DEEPSEEK_API_KEY",
        Some("deepseek-list-models"),
    );
    let config = write_config(&dir, &source);
    let catalog = write_named(&dir, "deepseek.json", DEEPSEEK_DOC);
    let omitted = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-pro",
        ],
    );
    let omitted_err = stderr_text(&omitted);
    assert!(
        !omitted.status.success(),
        "a blank default should refuse; stderr: {omitted_err}"
    );
    assert!(
        omitted_err.contains("default_provider"),
        "the refusal should name the blank default; stderr: {omitted_err}"
    );
    assert!(
        !stdout_text(&omitted).contains("deepseek-v4-pro"),
        "a blank default must not print the fixture id"
    );
    let added = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--json",
            "deepseek-v4-pro",
        ],
    );
    let stdout = stdout_text(&added);
    assert!(
        added.status.success(),
        "--provider should ignore a blank default; stderr: {}",
        stderr_text(&added)
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("search --json should parse");
    assert_eq!(
        parsed,
        serde_json::json!([{
            "id": "deepseek-v4-pro",
            "name": "DeepSeek V4 Pro",
            "context_length": 128_000
        }]),
        "the deepseek fixture should keep the context_length key; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn zai_without_a_cartridge_does_not_parse_or_get() {
    let dir = temp_root("zai");
    let source = provider_record(
        "default_provider = \"zai\"\n",
        "zai",
        "https://api.z.ai/api/paas/v4",
        "ZAI_API_KEY",
        None,
    );
    let config = write_config(&dir, &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", "this is not json");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the empty cache directory should be created");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--provider",
            "zai",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "glm",
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "zai should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("zai"),
        "stderr should name zai; stderr: {stderr}"
    );
    assert!(
        stderr.contains("no catalog cartridge"),
        "stderr should say no catalog cartridge is configured; stderr: {stderr}"
    );
    assert!(
        stderr.contains("Nothing was downloaded") && stderr.contains("Nothing was written"),
        "stderr should say nothing was downloaded and nothing was written; stderr: {stderr}"
    );
    assert!(
        stderr.contains("openrouter-models") && stderr.contains("deepseek-list-models"),
        "stderr should name the two catalog ids; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("json"),
        "a non-JSON catalog must not be parsed; stderr: {stderr}"
    );
    assert_dir_empty(&cache);
    assert_eq!(
        fs::read(&config).expect("the config should still exist"),
        before,
        "zai should write nothing"
    );
    remove_temp(&dir);
}

#[test]
fn an_unknown_catalog_id_is_named_and_does_not_select_openrouter() {
    let dir = temp_root("unknown-catalog");
    let source = provider_record(
        "default_provider = \"mirror\"\n",
        "mirror",
        "https://example.invalid/v1",
        "OPENROUTER_API_KEY",
        Some("not-a-cartridge"),
    );
    let config = write_config(&dir, &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", FIXTURE);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "--json",
            "gpt",
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "not-a-cartridge should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("not-a-cartridge"),
        "stderr should name that catalog id; stderr: {stderr}"
    );
    assert!(
        stderr.contains("no catalog cartridge"),
        "stderr should say no catalog cartridge is configured; stderr: {stderr}"
    );
    assert!(
        !refusal_sentence(&stderr)
            .to_ascii_lowercase()
            .contains("json"),
        "the fixture must not be parsed as OpenRouter; stderr: {stderr}"
    );
    let retired = format!("{} was not selected", "openrouter-models");
    assert!(
        !stderr.contains(&retired),
        "stderr must not say OpenRouter was not selected; stderr: {stderr}"
    );
    assert!(
        !stdout_text(&output).contains("gpt-4"),
        "the OpenRouter fixture must not be printed; stdout: {}",
        stdout_text(&output)
    );
    assert_eq!(
        fs::read(&config).expect("the config should still exist"),
        before
    );
    remove_temp(&dir);
}

#[test]
fn a_custom_base_returns_the_hex_cache_row_and_not_a_decoy() {
    let dir = temp_root("custom-base");
    let source = provider_record(
        "default_provider = \"mirror\"\n",
        "mirror",
        "https://example.invalid/v1",
        "OPENROUTER_API_KEY",
        Some("openrouter-models"),
    );
    let config = write_config(&dir, &source);
    let config_before = fs::read(&config).expect("the custom-base config should be readable");
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let base = "https://example.invalid/v1";
    let models_url = grok_router::catalog::models_url(base);
    let hex_path = grok_router::catalog::cache_path_for(&cache, &models_url);
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
    let decoy = cache.join("models.json");
    fs::write(&decoy, FIXTURE).expect("the models.json decoy should be written");
    mark_fresh(&decoy);
    let decoy_before = fs::read(&decoy).expect("the decoy should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
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
        !stdout.contains("gpt-4"),
        "models.json must not answer a custom base; stdout: {stdout}"
    );
    assert_eq!(
        fs::read(&hex_path).expect("the hex cache should still exist"),
        hex_before,
        "a cache hit must not rewrite the hex file"
    );
    assert_eq!(
        fs::read(&decoy).expect("models.json should still exist"),
        decoy_before,
        "search must not rewrite the decoy"
    );
    assert_eq!(
        fs::read(&config).expect("the custom-base config should still exist"),
        config_before,
        "search must not write the config"
    );
    remove_temp(&dir);
}

#[test]
fn an_openrouter_base_uses_models_json_even_when_the_name_is_not_openrouter() {
    let dir = temp_root("mirror-models-json");
    let source = provider_record(
        "default_provider = \"mirror\"\n",
        "mirror",
        COMPILED_URL,
        "OPENROUTER_API_KEY",
        Some("openrouter-models"),
    );
    let config = write_config(&dir, &source);
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let models = cache.join("models.json");
    fs::write(&models, FIXTURE).expect("models.json should be written");
    mark_fresh(&models);
    let models_before = fs::read(&models).expect("models.json should be readable");
    let openrouter_url = grok_router::catalog::models_url(COMPILED_URL);
    let hex = hex_digits(openrouter_url.as_bytes());
    let hex_path = grok_router::catalog::cache_path_for(&cache, &openrouter_url);
    let expected_name = format!("models-{hex}.json");
    assert_eq!(
        hex_path.file_name().and_then(|name| name.to_str()),
        Some(expected_name.as_str()),
        "the OpenRouter URL should cache as the hex of its bytes"
    );
    assert_ne!(hex_path, models);
    fs::write(&hex_path, FIXTURE).expect("the hex cache should be written");
    mark_fresh(&hex_path);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--provider",
            "mirror",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--quiet",
            "turbo",
        ],
    );
    assert!(
        output.status.success(),
        "an OpenRouter base should read its hex cache, not models.json; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout_text(&output), "gpt-4-turbo\n");
    assert_eq!(
        fs::read(&models).expect("models.json should still exist"),
        models_before,
        "search must not use the models.json decoy"
    );
    remove_temp(&dir);
}

#[test]
fn a_provider_named_openrouter_with_another_base_does_not_use_models_json() {
    let dir = temp_root("openrouter-other-base");
    let source = provider_record(
        "default_provider = \"openrouter\"\n",
        "openrouter",
        "https://example.invalid/v1",
        "OPENROUTER_API_KEY",
        Some("openrouter-models"),
    );
    let config = write_config(&dir, &source);
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let decoy = cache.join("models.json");
    fs::write(&decoy, FIXTURE).expect("the models.json decoy should be written");
    mark_fresh(&decoy);
    let decoy_before = fs::read(&decoy).expect("the decoy should be readable");
    let models_url = grok_router::catalog::models_url("https://example.invalid/v1");
    let hex_path = grok_router::catalog::cache_path_for(&cache, &models_url);
    assert_ne!(hex_path, decoy);
    fs::write(&hex_path, HEX_CATALOG).expect("the hex cache should be written");
    mark_fresh(&hex_path);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--quiet",
            "hex-cache",
        ],
    );
    assert!(
        output.status.success(),
        "the other base should read its hex cache; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout_text(&output), "hex-cache/only\n");
    assert_eq!(
        fs::read(&decoy).expect("models.json should still exist"),
        decoy_before
    );
    remove_temp(&dir);
}

#[test]
fn a_deepseek_base_reads_its_hex_cache_and_not_models_json() {
    let dir = temp_root("deepseek-cache");
    let source = provider_record(
        "default_provider = \"deepseek\"\n",
        "deepseek",
        "https://api.deepseek.com",
        "DEEPSEEK_API_KEY",
        Some("deepseek-list-models"),
    );
    let config = write_config(&dir, &source);
    let cache = dir.join("cache");
    fs::create_dir_all(&cache).expect("the cache directory should be created");
    let decoy = cache.join("models.json");
    fs::write(&decoy, FIXTURE).expect("the models.json decoy should be written");
    mark_fresh(&decoy);
    let decoy_before = fs::read(&decoy).expect("the decoy should be readable");
    let models_url = grok_router::catalog::models_url("https://api.deepseek.com");
    let hex_path = grok_router::catalog::cache_path_for(&cache, &models_url);
    let name = hex_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .expect("the hex cache name should be Unicode");
    assert_ne!(name, "models.json");
    fs::write(&hex_path, DEEPSEEK_DOC).expect("the deepseek cache should be written");
    mark_fresh(&hex_path);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "search",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "--quiet",
            "deepseek-v4-pro",
        ],
    );
    assert!(
        output.status.success(),
        "deepseek should read its hex cache; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(stdout_text(&output), "deepseek-v4-pro\n");
    assert!(
        !stdout_text(&output).contains("gpt-4"),
        "models.json must not answer a deepseek base; stdout: {}",
        stdout_text(&output)
    );
    assert_eq!(
        fs::read(&decoy).expect("models.json should still exist"),
        decoy_before
    );
    remove_temp(&dir);
}

#[test]
fn timeout_zero_or_a_non_integer_refuses_before_a_download_or_a_write() {
    let dir = temp_root("timeout");
    let config = write_config(&dir, &openrouter_config());
    let catalog = write_named(&dir, "models.json", FIXTURE);
    let before = fs::read(&config).expect("the config should be readable");
    for bad in ["0", "no"] {
        let output = run(
            &dir,
            &[
                "search",
                "--timeout",
                bad,
                "--catalog-file",
                &utf8_arg(&catalog, "the catalog path should be UTF-8"),
                "--config",
                &utf8_arg(&config, "the config path should be UTF-8"),
                "gpt-4",
            ],
        );
        let stderr = stderr_text(&output);
        assert_eq!(
            output.status.code(),
            Some(1),
            "search --timeout {bad} should exit 1; stderr: {stderr}"
        );
        assert!(
            stderr.contains("--timeout"),
            "stderr should name --timeout; stderr: {stderr}"
        );
        assert!(
            stderr.contains("Nothing was downloaded"),
            "stderr should say nothing was downloaded; stderr: {stderr}"
        );
        assert!(
            stderr.contains("nothing was written"),
            "stderr should say nothing was written; stderr: {stderr}"
        );
        assert_eq!(
            fs::read(&config).expect("the config should still exist"),
            before,
            "search --timeout {bad} should write nothing"
        );
    }
    let help = stdout_text(&run(&dir, &["search", "--help"]));
    assert!(
        help.contains("--timeout"),
        "search help should show --timeout"
    );
    assert!(
        help.contains("30"),
        "search help should show the 30 second default"
    );
    remove_temp(&dir);
}

fn hex_digits(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut hex = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        if write!(hex, "{byte:02x}").is_err() {
            break;
        }
    }
    hex
}
