#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! `grok-router add` writes one model from a `[router]` provider record.
//!
//! These tests do not call the network and do not read a home config. A run
//! that omits `--catalog-file` points `--cache-dir` at the temp directory.
//! The bearer is set only when a case needs a fresh cache. Secret values are
//! not printed when an assertion fails.

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

const OPENROUTER_RECORD: &str = "\
[router]
default_provider = \"openrouter\"

[router.provider.openrouter]
base_url = \"https://openrouter.ai/api/v1\"
env_key = \"OPENROUTER_API_KEY\"
catalog = \"openrouter-models\"
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
    format!("{OPENROUTER_RECORD}\n{COMMENT}\n{DEEPSEEK_SPAN}\n{GLM_SPAN}\n{CONFIG_TAIL}")
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
    let path = std::env::temp_dir().join(format!("grok-router-add-{label}-{}", std::process::id()));
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
    path.to_str().expect(what).to_owned()
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    run_with(dir, args, &[])
}

fn run_with(dir: &Path, args: &[&str], env_vars: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_grok-router"));
    let command = env_vars.iter().fold(
        command
            .args(args)
            .env_remove("OPENROUTER_API_KEY")
            .env_remove("DEEPSEEK_API_KEY")
            .env_remove("ANTHROPIC_AUTH_TOKEN")
            .env_remove("EXAMPLE_API_KEY")
            .env("HOME", dir)
            .env("NO_COLOR", "1")
            .current_dir(dir),
        |command, (key, value)| command.env(key, value),
    );
    command
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
        help.contains(
            "grok-router add --catalog-file models.json --config config.toml z-ai/glm-5.3"
        ),
        "add --help should contain a concrete example; help: {help}"
    );
    assert!(
        help.contains("grok-router add --provider deepseek --catalog-file deepseek.json --config config.toml deepseek-v4-pro"),
        "add --help should show the deepseek record example; help: {help}"
    );
    assert!(
        help.contains("--provider"),
        "add --help should document --provider; help: {help}"
    );
    assert!(
        help.contains("writes unless --dry-run"),
        "add --help should say the command writes unless --dry-run is set; help: {help}"
    );
    assert!(
        help.contains("https://example.test/v1") && help.contains("EXAMPLE_API_KEY"),
        "add --help should show the direct example; help: {help}"
    );
    assert!(
        !help.contains("TokenFix")
            && !help.contains("proxy.tokenfix.trilogy.com")
            && !help.contains("ANTHROPIC_AUTH_TOKEN"),
        "add --help should not contain the acceptance host or its secret name; help: {help}"
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
        "the cartridge slug should be the new header; config: {after}"
    );
    let new_block = "\
[model.or-widget-v2]
model = \"acme/widget.v2\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"Widget\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 4096
vision = true
";
    assert!(
        after.contains(new_block),
        "the new span should use the record base, the record env name, and vision for image input; config: {after}"
    );
    assert!(
        after.contains(OPENROUTER_RECORD),
        "the [router] record should stay byte-identical; config: {after}"
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
    assert_eq!(
        after, before,
        "a refused add should leave the file unchanged"
    );
    assert!(
        !config.with_extension("toml.prev").exists()
            && !config.with_file_name("config.toml.prev").exists(),
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
    assert_eq!(
        after, before,
        "a no-op add should leave the bytes unchanged"
    );
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
    let backed_up =
        fs::read(&backup).expect("one write should leave the original bytes as the sibling backup");
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
    assert_eq!(
        after, before,
        "--dry-run should leave the file bytes identical"
    );
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
    assert_eq!(
        exit_code(&json_out),
        0,
        "json add should succeed; stderr: {stderr}"
    );
    assert!(
        stderr.is_empty(),
        "diagnostics should stay off stdout; stderr: {stderr}"
    );
    let parsed: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("add --json should be one JSON value");
    assert!(
        parsed.is_object(),
        "add --json should print one object; stdout: {stdout}"
    );
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
    assert_eq!(
        after, before,
        "a failed resolve should leave the file unchanged"
    );
    remove_temp(&dir);
}

const CUSTOM_BASE: &str = "https://example.invalid/v1";

fn config_with_record_base(base: &str) -> String {
    format!(
        "[router]\ndefault_provider = \"openrouter\"\n\n[router.provider.openrouter]\nbase_url = \"{base}\"\nenv_key = \"OPENROUTER_API_KEY\"\ncatalog = \"openrouter-models\"\n\n{COMMENT}\n{DEEPSEEK_SPAN}\n{GLM_SPAN}\n{CONFIG_TAIL}"
    )
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

#[test]
fn a_custom_base_is_written_on_the_new_block_and_existing_spans_stay() {
    let dir = temp_root("custom-base");
    let (config, catalog, cache) = write_fixture(&dir);
    let text = config_with_record_base(CUSTOM_BASE);
    fs::write(&config, &text).expect("the custom-base config should be written");
    let before = fs::read(&config).expect("the custom-base config should be readable");
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
            "acme/widget.v2",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "add should write the record base from --config; stderr: {stderr}; stdout: {stdout}"
    );
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    let new_block_base =
        format!("[model.or-widget-v2]\nmodel = \"acme/widget.v2\"\nbase_url = \"{CUSTOM_BASE}\"\n");
    assert!(
        after.contains(&new_block_base),
        "the new block should use the record base {CUSTOM_BASE}; config: {after}"
    );
    assert!(
        after.contains(&format!(
            "[router.provider.openrouter]\nbase_url = \"{CUSTOM_BASE}\"\n"
        )),
        "the [router] record should stay; config: {after}"
    );
    assert!(
        !after.contains("openrouter_base_url"),
        "add must not read or write openrouter_base_url; config: {after}"
    );
    assert_preserved_spans(&after);
    assert_ne!(
        after.as_bytes(),
        before.as_slice(),
        "the add should insert a block"
    );
    assert_dir_empty(&cache);
    remove_temp(&dir);
}

#[test]
fn a_custom_base_cache_miss_reports_a_missing_key_and_does_not_read_models_json() {
    let dir = temp_root("hex-miss");
    let (config, _catalog, cache) = write_fixture(&dir);
    let text = config_with_record_base(CUSTOM_BASE);
    fs::write(&config, &text).expect("the custom-base config should be written");
    let before = fs::read(&config).expect("the custom-base config should be readable");
    let models = cache.join("models.json");
    fs::write(&models, CATALOG).expect("the decoy models.json should be written");
    mark_fresh(&models);
    let models_before = fs::read(&models).expect("models.json should be readable before add");
    let models_url = grok_router::catalog::models_url(CUSTOM_BASE);
    let hex_path = grok_router::catalog::cache_path_for(&cache, &models_url);
    let hex_name = hex_path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .expect("the hex cache name should be Unicode");
    assert_ne!(
        hex_name, "models.json",
        "a custom base must not be stored as models.json; name: {hex_name}"
    );
    assert!(
        !hex_path.exists(),
        "this case misses the hex cache at {}",
        hex_path.display()
    );
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "acme/widget.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "a hex-cache miss should fail closed instead of reading models.json; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("OPENROUTER_API_KEY is missing"),
        "the miss should fall through to a missing key, not a silent models.json read; stderr: {stderr}"
    );
    assert!(
        stderr.contains("not downloaded"),
        "the miss should say the catalog was not downloaded; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the config fixture should still exist");
    assert_eq!(
        after, before,
        "a cache miss should leave the config unchanged"
    );
    let models_after = fs::read(&models).expect("models.json should still exist");
    assert_eq!(
        models_before, models_after,
        "a custom-base miss must not rewrite models.json"
    );
    assert!(
        !hex_path.exists(),
        "a missing key must not create the hex cache at {}",
        hex_path.display()
    );
    assert!(
        !config.with_file_name("config.toml.prev").exists(),
        "a refused add should not write a backup"
    );
    remove_temp(&dir);
}

#[test]
fn add_without_config_exits_1_and_does_not_open_a_home_config() {
    let dir = temp_root("no-config");
    let home_config = dir.join(".grok").join("config.toml");
    let output = run(&dir, &["add", "acme/widget.v2"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "add without --config should fail; stderr: {stderr}"
    );
    assert!(
        stderr.contains("no --config"),
        "stderr should say that no --config path was given, not that a home file was missing; stderr: {stderr}"
    );
    assert!(
        !home_config.exists(),
        "add should not create a home config at {}",
        home_config.display()
    );
    remove_temp(&dir);
}

#[test]
fn an_invalid_base_url_leaves_the_file_and_does_not_download() {
    let dir = temp_root("bad-base");
    let (config, _catalog, cache) = write_fixture(&dir);
    let text = config_with_record_base("ftp://nope");
    fs::write(&config, &text).expect("the invalid-base config should be written");
    let before = fs::read(&config).expect("the invalid-base config should be readable");
    assert_dir_empty(&cache);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "acme/widget.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "an invalid record base_url should fail before a download; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("base_url"),
        "the failure should name base_url; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("openrouter_base_url"),
        "the failure must not treat openrouter_base_url as the base; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("https://openrouter.ai/api/v1"),
        "the failure must not choose the compiled OpenRouter URL; stderr: {stderr}"
    );
    assert!(
        stderr.contains("left unchanged"),
        "the failure should say the file was left unchanged; stderr: {stderr}"
    );
    assert!(
        stderr.contains("Nothing was downloaded"),
        "the failure should say nothing was downloaded; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("OPENROUTER_API_KEY is missing"),
        "an invalid setting should return before the catalog load; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the invalid-base config should still exist");
    assert_eq!(
        after, before,
        "an invalid setting should leave the file unchanged"
    );
    assert_dir_empty(&cache);
    assert!(
        !config.with_file_name("config.toml.prev").exists(),
        "an invalid setting should not write a backup"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("json"),
        "an invalid record should fail before a catalog parse; stderr: {stderr}"
    );
    remove_temp(&dir);
}

const PRO_SPAN: &str = "\
[model.or-deepseek-pro]
model = \"deepseek/deepseek-v4-pro-0813\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Pro\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
";

const GLM_FLASH_SPAN: &str = "\
[model.or-glm-5-3-flash]
model = \"z-ai/glm-5.3-flash\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3 Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
";

const DEEPSEEK_BASE: &str = "https://api.deepseek.com";

const DEEPSEEK_DOC: &str = r#"{
    "data": [
        {
            "id": "deepseek-v4-pro",
            "name": "DeepSeek V4 Pro",
            "context_window": 1000000,
            "input_modalities": ["text", "image"]
        },
        {
            "id": "deepseek-v4-text",
            "name": "DeepSeek V4 Text",
            "context_window": 4096,
            "input_modalities": ["text"]
        }
    ]
}"#;

fn four_spans() -> String {
    format!("{COMMENT}\n{DEEPSEEK_SPAN}\n{PRO_SPAN}\n{GLM_SPAN}\n{GLM_FLASH_SPAN}\n{CONFIG_TAIL}")
}

fn deepseek_router(default_line: &str) -> String {
    format!(
        "[router]\n{default_line}\n[router.provider.deepseek]\nbase_url = \"{DEEPSEEK_BASE}\"\nenv_key = \"DEEPSEEK_API_KEY\"\ncatalog = \"deepseek-list-models\"\n\n[router.provider.openrouter]\nbase_url = \"https://openrouter.ai/api/v1\"\nenv_key = \"OPENROUTER_API_KEY\"\ncatalog = \"openrouter-models\"\n\n{}",
        four_spans()
    )
}

fn write_named(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).expect("the temp fixture should be written");
    path
}

fn assert_secret_absent(label: &str, text: &str, secret: &str) {
    assert!(
        !text.contains(secret),
        "{label} included a watched secret value"
    );
}

#[test]
fn other_commands_do_not_grow_provider() {
    let dir = temp_root("no-provider-flag");
    for command in ["set-default", "remove", "list"] {
        let output = run(&dir, &[command, "--help"]);
        let help = stdout_text(&output);
        assert_eq!(
            exit_code(&output),
            0,
            "{command} --help should succeed; stderr: {}",
            stderr_text(&output)
        );
        assert!(
            !help.contains("--provider"),
            "{command} must not grow --provider; help: {help}"
        );
    }
    remove_temp(&dir);
}

#[test]
fn text_only_input_omits_the_vision_line() {
    let dir = temp_root("no-vision");
    let (config, catalog, _cache) = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "acme/other.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "a text-only row should be added; stderr: {stderr}"
    );
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert!(
        after.contains("[model.or-other-v2]\n"),
        "the cartridge slug should be or-other-v2; config: {after}"
    );
    assert!(
        !after.contains("vision"),
        "text-only input must not write a vision line; config: {after}"
    );
    assert_preserved_spans(&after);
    remove_temp(&dir);
}

#[test]
fn deepseek_add_writes_one_direct_span_and_keeps_the_old_bytes() {
    let dir = temp_root("deepseek-add");
    let router = "\
[router]
default_provider = \"openrouter\"

[router.provider.deepseek]
base_url = \"https://api.deepseek.com\"
env_key = \"DEEPSEEK_API_KEY\"
catalog = \"deepseek-list-models\"

[router.provider.openrouter]
base_url = \"https://openrouter.ai/api/v1\"
env_key = \"OPENROUTER_API_KEY\"
catalog = \"openrouter-models\"
";
    let source = format!("{router}\n{}", four_spans());
    let config = write_named(&dir, "config.toml", &source);
    let catalog = write_named(&dir, "deepseek.json", DEEPSEEK_DOC);
    let openrouter_secret = "openrouter-secret-value";
    let deepseek_secret = "deepseek-secret-value";
    let output = run_with(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-pro",
        ],
        &[
            ("OPENROUTER_API_KEY", openrouter_secret),
            ("DEEPSEEK_API_KEY", deepseek_secret),
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert_secret_absent("stderr", &stderr, openrouter_secret);
    assert_secret_absent("stderr", &stderr, deepseek_secret);
    assert_eq!(
        exit_code(&output),
        0,
        "a deepseek add should exit 0; stderr: {stderr}; stdout: {stdout}"
    );
    assert_eq!(stdout, "deepseek-v4-pro\n");
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    assert_secret_absent("config", &after, openrouter_secret);
    assert_secret_absent("config", &after, deepseek_secret);
    let new_span = "\
[model.deepseek-v4-pro]
model = \"deepseek-v4-pro\"
base_url = \"https://api.deepseek.com\"
name = \"DeepSeek V4 Pro\"
env_key = \"DEEPSEEK_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
vision = true
";
    assert!(
        after.contains(new_span),
        "the new span should be the direct id with the record base and env name; config: {after}"
    );
    assert!(
        !new_span.contains("or-deepseek")
            && !new_span.contains("deepseek/deepseek-v4-pro-0813")
            && !new_span.contains("api_key"),
        "the new span must not use an or- slug, an OpenRouter id, or api_key"
    );
    for span in [DEEPSEEK_SPAN, PRO_SPAN, GLM_SPAN, GLM_FLASH_SPAN] {
        assert!(
            after.contains(span),
            "an existing span should stay byte-identical; config: {after}"
        );
    }
    assert!(
        after.contains(router),
        "the [router] table should stay byte-identical; config: {after}"
    );
    remove_temp(&dir);
}

#[test]
fn deepseek_text_row_omits_vision() {
    let dir = temp_root("deepseek-text");
    let source = deepseek_router("default_provider = \"openrouter\"\n");
    let config = write_named(&dir, "config.toml", &source);
    let catalog = write_named(&dir, "deepseek.json", DEEPSEEK_DOC);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-text",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "a text deepseek row should be added; stderr: {stderr}"
    );
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    let header_at = after
        .find("[model.deepseek-v4-text]\n")
        .expect("the direct slug should be present");
    let span = after
        .get(header_at..)
        .and_then(|rest| rest.split_once("\n\n"))
        .map(|(span, _)| span)
        .expect("the new span should end at a blank line");
    assert!(
        !span.contains("vision"),
        "a text modality must not write vision; span: {span}"
    );
    assert!(span.contains("env_key = \"DEEPSEEK_API_KEY\"\n"));
    remove_temp(&dir);
}

#[test]
fn explicit_provider_does_not_need_default_provider() {
    let dir = temp_root("explicit-provider");
    let source = deepseek_router("");
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "deepseek.json", DEEPSEEK_DOC);
    let added = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-text",
        ],
    );
    assert_eq!(
        exit_code(&added),
        0,
        "--provider deepseek should add without default_provider; stderr: {}",
        stderr_text(&added)
    );
    let omitted = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-pro",
        ],
    );
    let stderr = stderr_text(&omitted);
    assert_eq!(
        exit_code(&omitted),
        1,
        "omitted --provider should not pick a record; stdout: {}",
        stdout_text(&omitted)
    );
    assert!(
        stderr.contains("default_provider"),
        "the refusal should name default_provider; stderr: {stderr}"
    );
    let after = fs::read_to_string(&config).expect("the config should stay UTF-8");
    assert!(
        after.contains("[model.deepseek-v4-text]\n"),
        "the explicit add should have landed before the omitted refusal; config: {after}"
    );
    assert!(
        !after.contains("[model.deepseek-v4-pro]"),
        "the omitted provider must not write a second span; config: {after}"
    );
    assert_ne!(
        after.as_bytes(),
        before.as_slice(),
        "the explicit add should have changed the file"
    );
    remove_temp(&dir);
}

#[test]
fn blank_default_provider_still_honors_an_explicit_provider() {
    let dir = temp_root("blank-default");
    let source = deepseek_router("default_provider = \"\"\n");
    let config = write_named(&dir, "config.toml", &source);
    let catalog = write_named(&dir, "deepseek.json", DEEPSEEK_DOC);
    let omitted = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-text",
        ],
    );
    let omitted_err = stderr_text(&omitted);
    assert_eq!(
        exit_code(&omitted),
        1,
        "a blank default should refuse; stderr: {omitted_err}"
    );
    assert!(
        omitted_err.contains("default_provider"),
        "the refusal should name the blank default; stderr: {omitted_err}"
    );
    let before = fs::read(&config).expect("the refused config should still exist");
    let added = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "deepseek-v4-text",
        ],
    );
    assert_eq!(
        exit_code(&added),
        0,
        "--provider should ignore a blank default; stderr: {}",
        stderr_text(&added)
    );
    let after = fs::read(&config).expect("the explicit add should leave a file");
    assert_ne!(
        after, before,
        "the explicit add should write the direct span"
    );
    remove_temp(&dir);
}

#[test]
fn a_missing_router_refuses_before_a_catalog_parse() {
    let dir = temp_root("missing-router");
    let source = four_spans();
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", "this is not json");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "a missing [router] should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("[router]"),
        "stderr should name the missing [router] data; stderr: {stderr}"
    );
    assert!(
        stderr.contains("must write that record"),
        "stderr should say the operator must write that record; stderr: {stderr}"
    );
    assert!(
        stderr.contains("Nothing was downloaded"),
        "the refusal should happen before a download; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("https://openrouter.ai/api/v1"),
        "stderr must not choose the compiled OpenRouter URL; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("json"),
        "a non-JSON catalog must not be parsed when [router] is missing; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the config should still exist");
    assert_eq!(
        after, before,
        "a missing [router] should leave the file unchanged"
    );
    remove_temp(&dir);
}

#[test]
fn a_grok_router_table_is_not_a_provider_record() {
    let dir = temp_root("grok-router-only");
    let source = format!(
        "[grok_router]\nopenrouter_base_url = \"https://openrouter.ai/api/v1\"\n\n[grok_router.provider.deepseek]\nbase_url = \"{DEEPSEEK_BASE}\"\nenv_key = \"DEEPSEEK_API_KEY\"\ncatalog = \"deepseek-list-models\"\n\n{}",
        four_spans()
    );
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", "this is not json");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "only [grok_router] should refuse; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("[router]"),
        "stderr should name [router]; stderr: {stderr}"
    );
    assert!(
        stderr.contains("must write that record"),
        "stderr should say the operator must write that record; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("https://openrouter.ai/api/v1"),
        "stderr must not choose the compiled URL; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("json"),
        "the catalog file must not be parsed; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the config should still exist");
    assert_eq!(after, before);
    remove_temp(&dir);
}

#[test]
fn a_windowless_deepseek_row_refuses_the_add() {
    let dir = temp_root("deepseek-window");
    let source = deepseek_router("default_provider = \"deepseek\"\n");
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let cases = [
        r#"{"data":[{"id":"deepseek-v4-pro","name":"Pro"}]}"#,
        r#"{"data":[{"id":"deepseek-v4-pro","name":"Pro","context_window":0}]}"#,
        r#"{"data":[{"id":"deepseek-v4-pro","name":"Pro","context_length":128000}]}"#,
    ];
    for (index, document) in cases.iter().enumerate() {
        let catalog = write_named(&dir, &format!("case-{index}.json"), document);
        let output = run(
            &dir,
            &[
                "--config",
                &utf8_arg(&config, "the config path should be UTF-8"),
                "add",
                "--provider",
                "deepseek",
                "--catalog-file",
                &utf8_arg(&catalog, "the catalog path should be UTF-8"),
                "deepseek-v4-pro",
            ],
        );
        let stderr = stderr_text(&output);
        assert_eq!(
            exit_code(&output),
            1,
            "case {index} should refuse a non-positive window; stdout: {}",
            stdout_text(&output)
        );
        assert!(
            stderr.contains("left unchanged"),
            "case {index} should leave the file unchanged; stderr: {stderr}"
        );
        let after = fs::read(&config).expect("the config should still exist");
        assert_eq!(after, before, "case {index} should not write a span");
    }
    remove_temp(&dir);
}

#[test]
fn an_openrouter_fixture_fed_to_deepseek_does_not_write_an_or_slug() {
    let dir = temp_root("wrong-fixture");
    let source = deepseek_router("default_provider = \"deepseek\"\n");
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", CATALOG);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--provider",
            "deepseek",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
        "an OpenRouter fixture should not satisfy a deepseek add; stdout: {}",
        stdout_text(&output)
    );
    let after = fs::read_to_string(&config).expect("the config should stay UTF-8");
    assert_eq!(
        after.as_bytes(),
        before.as_slice(),
        "the file should stay unchanged; stderr: {stderr}"
    );
    assert!(
        !after.contains("[model.or-widget-v2]"),
        "deepseek must not write an or- slug; config: {after}"
    );
    assert!(
        !after.contains("context_window = 4096"),
        "context_length must not be copied into context_window; config: {after}"
    );
    remove_temp(&dir);
}

#[test]
fn zai_without_a_cartridge_does_not_parse_or_write() {
    let dir = temp_root("zai");
    let source = format!(
        "[router]\ndefault_provider = \"zai\"\n\n[router.provider.zai]\nbase_url = \"https://api.z.ai/api/paas/v4\"\nenv_key = \"ZAI_API_KEY\"\n\n{}",
        four_spans()
    );
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", "this is not json");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--provider",
            "zai",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "glm-5.3",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
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
        stderr.contains("--provider"),
        "stderr should say to pass --provider for a record that already has one; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("try again"),
        "stderr must not say try again; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("json"),
        "a non-JSON catalog must not be parsed; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("parser"),
        "stderr must not offer a Z.ai parser; stderr: {stderr}"
    );
    let after = fs::read(&config).expect("the config should still exist");
    assert_eq!(after, before, "zai should write nothing");
    remove_temp(&dir);
}

#[test]
fn an_unknown_catalog_id_is_named_and_does_not_select_openrouter() {
    let dir = temp_root("unknown-catalog");
    let source = format!(
        "[router]\ndefault_provider = \"mirror\"\n\n[router.provider.mirror]\nbase_url = \"https://example.invalid/v1\"\nenv_key = \"OPENROUTER_API_KEY\"\ncatalog = \"not-a-cartridge\"\n\n{}",
        four_spans()
    );
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the config should be readable");
    let catalog = write_named(&dir, "models.json", CATALOG);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        1,
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
        !stderr.to_ascii_lowercase().contains("json"),
        "the fixture must not be parsed as OpenRouter; stderr: {stderr}"
    );
    let retired = format!("{} was not selected", "openrouter-models");
    assert!(
        !stderr.contains(&retired),
        "stderr must not say OpenRouter was not selected; stderr: {stderr}"
    );
    let after = fs::read_to_string(&config).expect("the config should stay UTF-8");
    assert_eq!(after.as_bytes(), before.as_slice());
    assert!(!after.contains("[model.or-widget-v2]"));
    remove_temp(&dir);
}

#[test]
fn a_pasted_secret_refuses_before_a_write() {
    let dir = temp_root("pasted-secret");
    let (config, _catalog, _cache) = write_fixture(&dir);
    let before = fs::read(&config).expect("the config should be readable");
    let openrouter_secret = "pasted-openrouter-token";
    let deepseek_secret = "pasted-deepseek-token";
    let document = format!(
        "{{\"data\":[{{\"id\":\"acme/widget.v2\",\"name\":\"{openrouter_secret}\",\"context_length\":8,\"architecture\":{{\"input_modalities\":[\"text\"]}}}}]}}"
    );
    let catalog = write_named(&dir, "secret.json", &document);
    let output = run_with(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ],
        &[
            ("OPENROUTER_API_KEY", openrouter_secret),
            ("DEEPSEEK_API_KEY", deepseek_secret),
        ],
    );
    let stderr = stderr_text(&output);
    assert_secret_absent("stderr", &stderr, openrouter_secret);
    assert_secret_absent("stderr", &stderr, deepseek_secret);
    assert_eq!(exit_code(&output), 1, "a pasted token should refuse");
    assert!(
        stderr.contains("OPENROUTER_API_KEY"),
        "stderr should name the variable, not its value"
    );
    let after = fs::read(&config).expect("the config should still exist");
    assert_eq!(after, before, "a pasted token should not write the file");
    let after_text = String::from_utf8(after).expect("the config should stay UTF-8");
    assert_secret_absent("config", &after_text, openrouter_secret);
    assert_secret_absent("config", &after_text, deepseek_secret);

    let mirror_dir = temp_root("pasted-mirror");
    let mirror_secret = "pasted-mirror-token";
    let mirror_config = write_named(
        &mirror_dir,
        "config.toml",
        "[router]\ndefault_provider = \"mirror\"\n\n[router.provider.mirror]\nbase_url = \"https://openrouter.ai/api/v1\"\nenv_key = \"MIRROR_API_KEY\"\ncatalog = \"openrouter-models\"\n\n[models]\ndefault = \"grok-4.7\"\n",
    );
    let mirror_before = fs::read(&mirror_config).expect("the mirror config should be readable");
    let mirror_document = format!(
        "{{\"data\":[{{\"id\":\"acme/widget.v2\",\"name\":\"{mirror_secret}\",\"context_length\":8,\"architecture\":{{\"input_modalities\":[\"text\"]}}}}]}}"
    );
    let mirror_catalog = write_named(&mirror_dir, "secret.json", &mirror_document);
    let mirror = run_with(
        &mirror_dir,
        &[
            "--config",
            &utf8_arg(&mirror_config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&mirror_catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ],
        &[("MIRROR_API_KEY", mirror_secret)],
    );
    let mirror_err = stderr_text(&mirror);
    assert_secret_absent("stderr", &mirror_err, mirror_secret);
    assert_eq!(
        exit_code(&mirror),
        1,
        "a third env key should refuse; stderr: {mirror_err}"
    );
    assert!(
        mirror_err.contains("MIRROR_API_KEY"),
        "stderr should name the third variable, not only the old pair; stderr: {mirror_err}"
    );
    assert_eq!(
        fs::read(&mirror_config).expect("the mirror config should still exist"),
        mirror_before,
        "a third env key should leave the file unchanged"
    );

    use std::os::unix::ffi::OsStringExt;
    let raw_dir = temp_root("pasted-raw");
    let raw_config = write_named(
        &raw_dir,
        "config.toml",
        "[router]\ndefault_provider = \"mirror\"\n\n[router.provider.mirror]\nbase_url = \"https://openrouter.ai/api/v1\"\nenv_key = \"MIRROR_API_KEY\"\ncatalog = \"openrouter-models\"\n\n[models]\ndefault = \"grok-4.7\"\n",
    );
    let raw_before = fs::read(&raw_config).expect("the raw config should be readable");
    let raw_catalog = write_named(
        &raw_dir,
        "plain.json",
        r#"{"data":[{"id":"acme/widget.v2","name":"Widget","context_length":8,"architecture":{"input_modalities":["text"]}}]}"#,
    );
    let raw = Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .args([
            "--config",
            &utf8_arg(&raw_config, "the config path should be UTF-8"),
            "add",
            "--catalog-file",
            &utf8_arg(&raw_catalog, "the catalog path should be UTF-8"),
            "acme/widget.v2",
        ])
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("DEEPSEEK_API_KEY")
        .env("MIRROR_API_KEY", std::ffi::OsString::from_vec(vec![0xff, 0xfe]))
        .env("HOME", &raw_dir)
        .env("NO_COLOR", "1")
        .current_dir(&raw_dir)
        .output()
        .expect("the grok-router binary should start");
    let raw_err = stderr_text(&raw);
    assert_eq!(
        exit_code(&raw),
        1,
        "a non-Unicode env value should refuse; stderr: {raw_err}"
    );
    assert!(
        raw_err.contains("MIRROR_API_KEY"),
        "stderr should name the non-Unicode variable; stderr: {raw_err}"
    );
    assert!(
        !raw_err.contains("OPENROUTER_API_KEY"),
        "a mirror key must not be described as OPENROUTER_API_KEY; stderr: {raw_err}"
    );
    assert_eq!(
        fs::read(&raw_config).expect("the raw config should still exist"),
        raw_before,
        "a non-Unicode env value should leave the file unchanged"
    );

    remove_temp(&dir);
    remove_temp(&mirror_dir);
    remove_temp(&raw_dir);
}

#[test]
fn an_openrouter_base_reads_models_json_and_a_deepseek_base_does_not() {
    let dir = temp_root("cache-names");
    let (config, _catalog, cache) = write_fixture(&dir);
    let models = cache.join("models.json");
    fs::write(&models, CATALOG).expect("models.json should be written");
    mark_fresh(&models);
    let models_before = fs::read(&models).expect("models.json should be readable");
    let openrouter_url = grok_router::catalog::models_url("https://openrouter.ai/api/v1");
    let hex_name = {
        let mut hex = String::new();
        for byte in openrouter_url.as_bytes() {
            hex.push_str(&format!("{byte:02x}"));
        }
        format!("models-{hex}.json")
    };
    let hex_path = grok_router::catalog::cache_path_for(&cache, &openrouter_url);
    assert_eq!(
        hex_path.file_name().and_then(|name| name.to_str()),
        Some(hex_name.as_str()),
        "the OpenRouter URL should cache as the hex of its bytes"
    );
    assert_ne!(hex_path, models, "models.json must not be that cache");
    fs::write(&hex_path, CATALOG).expect("the hex cache should be written");
    mark_fresh(&hex_path);
    let hex_before = fs::read(&hex_path).expect("the hex cache should be readable");
    let hit = run_with(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--cache-dir",
            &utf8_arg(&cache, "the cache path should be UTF-8"),
            "acme/widget.v2",
        ],
        &[("OPENROUTER_API_KEY", "cache-hit-token")],
    );
    let hit_err = stderr_text(&hit);
    assert_secret_absent("stderr", &hit_err, "cache-hit-token");
    assert_eq!(
        exit_code(&hit),
        0,
        "a fresh hex cache should answer an OpenRouter base; stderr: {hit_err}; stdout: {}",
        stdout_text(&hit)
    );
    let models_after = fs::read(&models).expect("models.json should still exist");
    assert_eq!(
        models_before, models_after,
        "a cache hit must not rewrite the models.json decoy"
    );
    assert_eq!(
        fs::read(&hex_path).expect("the hex cache should still exist"),
        hex_before,
        "a cache hit must not rewrite the hex file"
    );
    let after = fs::read_to_string(&config).expect("the config should be UTF-8");
    assert_secret_absent("config", &after, "cache-hit-token");
    assert!(after.contains("[model.or-widget-v2]\n"));

    let deepseek_dir = temp_root("deepseek-cache");
    let deepseek_config = write_named(
        &deepseek_dir,
        "config.toml",
        &deepseek_router("default_provider = \"deepseek\"\n"),
    );
    let deepseek_cache = deepseek_dir.join("cache");
    fs::create_dir_all(&deepseek_cache).expect("the deepseek cache dir should exist");
    let decoy = deepseek_cache.join("models.json");
    fs::write(&decoy, CATALOG).expect("the decoy models.json should be written");
    mark_fresh(&decoy);
    let decoy_before = fs::read(&decoy).expect("the decoy should be readable");
    let deepseek_url = grok_router::catalog::models_url(DEEPSEEK_BASE);
    let hex_path = grok_router::catalog::cache_path_for(&deepseek_cache, &deepseek_url);
    assert_ne!(
        hex_path.file_name().and_then(|name| name.to_str()),
        Some("models.json"),
        "a deepseek base must not use models.json"
    );
    fs::write(&hex_path, DEEPSEEK_DOC).expect("the deepseek cache should be written");
    mark_fresh(&hex_path);
    let cached = run_with(
        &deepseek_dir,
        &[
            "--config",
            &utf8_arg(&deepseek_config, "the config path should be UTF-8"),
            "add",
            "--cache-dir",
            &utf8_arg(&deepseek_cache, "the cache path should be UTF-8"),
            "deepseek-v4-text",
        ],
        &[("DEEPSEEK_API_KEY", "deepseek-cache-token")],
    );
    let cached_err = stderr_text(&cached);
    assert_secret_absent("stderr", &cached_err, "deepseek-cache-token");
    assert_eq!(
        exit_code(&cached),
        0,
        "a deepseek base should read its own cache, not models.json; stderr: {cached_err}; stdout: {}",
        stdout_text(&cached)
    );
    let decoy_after = fs::read(&decoy).expect("models.json should still exist");
    assert_eq!(
        decoy_before, decoy_after,
        "a deepseek add must not read or write models.json by changing it"
    );
    let written =
        fs::read_to_string(&deepseek_config).expect("the deepseek config should be UTF-8");
    assert!(written.contains("[model.deepseek-v4-text]\n"));
    assert!(written.contains(&format!("base_url = \"{DEEPSEEK_BASE}\"\n")));
    assert_secret_absent("config", &written, "deepseek-cache-token");
    remove_temp(&dir);
    remove_temp(&deepseek_dir);
}

const ACCEPTANCE_BLOCK: &str = "\
[model.claude-opus]
model = \"claude-opus-5-5\"
base_url = \"https://proxy.tokenfix.trilogy.com/v1\"
name = \"Claude Opus (TokenFix)\"
api_backend = \"messages\"
env_key = \"ANTHROPIC_AUTH_TOKEN\"
";

fn acceptance_args(config: &str) -> Vec<&str> {
    vec![
        "--config",
        config,
        "add",
        "--as",
        "claude-opus",
        "--model",
        "claude-opus-5-5",
        "--base-url",
        "https://proxy.tokenfix.trilogy.com/v1",
        "--name",
        "Claude Opus (TokenFix)",
        "--api-backend",
        "messages",
        "--env-key",
        "ANTHROPIC_AUTH_TOKEN",
    ]
}

#[test]
fn direct_flags_write_the_supplied_block_and_leave_the_four_spans() {
    let dir = temp_root("direct-accept");
    let source = four_spans();
    let config = write_named(&dir, "config.toml", &source);
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &acceptance_args(&config_arg));
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "the acceptance command should write without a provider; stderr: {stderr}; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("download"),
        "a direct add should not download a catalog; stderr: {stderr}"
    );
    let after = fs::read_to_string(&config).expect("the edited config should be UTF-8");
    let header_at = after
        .find("[model.claude-opus]\n")
        .expect("the acceptance slug should be present");
    let written = after
        .get(header_at..)
        .expect("the acceptance header should start a span");
    assert!(
        written.starts_with(ACCEPTANCE_BLOCK),
        "the new span should equal the acceptance block; config: {after}"
    );
    assert!(
        !ACCEPTANCE_BLOCK.contains("chat_completions")
            && !ACCEPTANCE_BLOCK.contains("context_window")
            && !ACCEPTANCE_BLOCK.contains("vision"),
        "the acceptance block should not force a catalog backend, window, or vision"
    );
    for span in [DEEPSEEK_SPAN, PRO_SPAN, GLM_SPAN, GLM_FLASH_SPAN] {
        assert!(
            after.contains(span),
            "an existing span should stay byte-identical; config: {after}"
        );
    }
    assert!(
        !source.contains("[router]"),
        "the fixture should have no [router] table"
    );
    let again = run(&dir, &acceptance_args(&config_arg));
    assert_eq!(
        exit_code(&again),
        0,
        "a second identical direct add should exit 0; stderr: {}",
        stderr_text(&again)
    );
    let second = fs::read_to_string(&config).expect("the second run should leave UTF-8");
    assert_eq!(
        second, after,
        "a second identical direct add should change nothing"
    );
    remove_temp(&dir);
}

#[test]
fn direct_add_of_a_different_block_for_the_same_slug_writes_nothing() {
    let dir = temp_root("direct-differ");
    let source = four_spans();
    let config = write_named(&dir, "config.toml", &source);
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let added = run(&dir, &acceptance_args(&config_arg));
    assert_eq!(
        exit_code(&added),
        0,
        "the first direct add should write; stderr: {}",
        stderr_text(&added)
    );
    let before = fs::read(&config).expect("the first write should be readable");
    let mut changed = acceptance_args(&config_arg);
    let model_at = changed
        .iter()
        .position(|arg| *arg == "--model")
        .expect("the acceptance args should include --model");
    let model_value_at = model_at.saturating_add(1);
    let model_slot = changed
        .get_mut(model_value_at)
        .expect("the model value should follow --model");
    *model_slot = "claude-opus-5-5-other";
    let refused = run(&dir, &changed);
    assert_ne!(
        exit_code(&refused),
        0,
        "a different block for claude-opus should fail; stdout: {}",
        stdout_text(&refused)
    );
    assert_eq!(
        fs::read(&config).expect("the refused config should still exist"),
        before,
        "a different block for the same slug should write nothing"
    );
    remove_temp(&dir);

    let flash_dir = temp_root("direct-flash");
    let flash_source = four_spans();
    let flash_config = write_named(&flash_dir, "config.toml", &flash_source);
    let flash_before = fs::read(&flash_config).expect("the flash fixture should be readable");
    let flash_arg = utf8_arg(&flash_config, "the config path should be UTF-8");
    let mut flash_args = acceptance_args(&flash_arg);
    let as_at = flash_args
        .iter()
        .position(|arg| *arg == "--as")
        .expect("the acceptance args should include --as");
    let as_slot = flash_args
        .get_mut(as_at.saturating_add(1))
        .expect("the slug should follow --as");
    *as_slot = "or-deepseek-flash";
    let flash = run(&flash_dir, &flash_args);
    assert_ne!(
        exit_code(&flash),
        0,
        "--as or-deepseek-flash with a different block should fail; stdout: {}",
        stdout_text(&flash)
    );
    assert_eq!(
        fs::read(&flash_config).expect("the flash fixture should still exist"),
        flash_before,
        "--as or-deepseek-flash with the acceptance model id should write nothing"
    );
    remove_temp(&flash_dir);
}

#[test]
fn partial_direct_flags_refuse_without_a_cartridge_or_a_write() {
    let dir = temp_root("direct-partial");
    let source = format!(
        "[router]\ndefault_provider = \"zai\"\n\n[router.provider.zai]\nbase_url = \"https://api.z.ai/api/paas/v4\"\nenv_key = \"ZAI_API_KEY\"\n\n{}",
        four_spans()
    );
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the partial fixture should be readable");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "add",
            "--model",
            "claude-opus-5-5",
            "--base-url",
            "https://example.test/v1",
        ],
    );
    let stderr = stderr_text(&output);
    assert_ne!(
        exit_code(&output),
        0,
        "a partial direct flag set should fail; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("--model") && stderr.contains("--base-url"),
        "stderr should name the direct flags that were set; stderr: {stderr}"
    );
    assert!(
        stderr.contains("--env-key") && stderr.contains("--api-backend"),
        "stderr should name the missing direct flags; stderr: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("cartridge"),
        "a partial flag set should not say a cartridge is missing; stderr: {stderr}"
    );
    assert_eq!(
        fs::read(&config).expect("the partial fixture should still exist"),
        before,
        "a partial flag set should write nothing"
    );
    remove_temp(&dir);
}

#[test]
fn direct_add_refuses_when_the_env_key_value_would_be_written() {
    let dir = temp_root("direct-secret");
    let source = four_spans();
    let config = write_named(&dir, "config.toml", &source);
    let before = fs::read(&config).expect("the secret fixture should be readable");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let mut args = acceptance_args(&config_arg);
    let name_at = args
        .iter()
        .position(|arg| *arg == "--name")
        .expect("the acceptance args should include --name");
    let secret = "direct-add-secret-value";
    let name_slot = args
        .get_mut(name_at.saturating_add(1))
        .expect("the name value should follow --name");
    *name_slot = secret;
    let output = run_with(&dir, &args, &[("ANTHROPIC_AUTH_TOKEN", secret)]);
    let stderr = stderr_text(&output);
    assert_ne!(
        exit_code(&output),
        0,
        "a direct add that would write the env value should fail; stdout: {}",
        stdout_text(&output)
    );
    assert!(
        stderr.contains("ANTHROPIC_AUTH_TOKEN"),
        "stderr should name ANTHROPIC_AUTH_TOKEN; stderr length {}",
        stderr.len()
    );
    assert_secret_absent("stderr", &stderr, secret);
    assert_secret_absent("stdout", &stdout_text(&output), secret);
    assert_eq!(
        fs::read(&config).expect("the secret fixture should still exist"),
        before,
        "a secret refusal should write nothing"
    );
    remove_temp(&dir);
}
