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

//! `grok-router benchmarks` reads published scores from one provider catalog.
//!
//! `--catalog-file` does not skip resolution and does not GET. A cartridge
//! with no benchmark feed refuses before that file is read and before a
//! download. These tests do not call the network and do not read provider env
//! vars. The config is not modified.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const COMPILED_URL: &str = "https://openrouter.ai/api/v1";

const FIXTURE: &str = r#"{
    "data": [
        {
            "id": "low/one",
            "name": "Low",
            "description": "plain",
            "pricing": {"prompt": "0.000001", "completion": "0.000002"},
            "benchmarks": {
                "artificial_analysis": {"intelligence_index": 10, "coding_index": 20, "agentic_index": null},
                "design_arena": {"uicomponent": {"elo": 9999}}
            }
        },
        {
            "id": "high/two",
            "name": "High Two",
            "description": "kimi cousin",
            "pricing": {"prompt": "0.000002", "completion": "0.00001"},
            "benchmarks": {"artificial_analysis": {"intelligence_index": 55.1, "coding_index": null, "agentic_index": 3}}
        },
        {
            "id": "tie/four",
            "name": "Tie",
            "description": "also high",
            "pricing": {"prompt": "0.000000079", "completion": "0.0000014"},
            "benchmarks": {"artificial_analysis": {"intelligence_index": 55.1, "coding_index": 1, "agentic_index": 1}}
        },
        {
            "id": "blank/three",
            "name": "Blank",
            "description": "kimi blank",
            "pricing": {"prompt": "-1", "completion": "0"}
        }
    ]
}"#;

const UNSCORED: &str = r#"{
    "data": [
        {
            "id": "plain/model",
            "name": "Plain",
            "description": "no score yet",
            "pricing": {"prompt": "0.000001", "completion": "0.000002"}
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
    let path = std::env::temp_dir().join(format!(
        "grok-router-benchmarks-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp benchmarks directory should be created");
    path
}

fn write_named(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).expect("the named fixture should be written");
    path
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
    Command::new(env!("CARGO_BIN_EXE_grok-router"))
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
    String::from_utf8(output.stdout.clone()).expect("benchmarks stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("benchmarks stderr should be UTF-8")
}

fn openrouter_paths(dir: &Path, body: &str) -> (PathBuf, String, String) {
    let fixture = write_named(dir, "models.json", body);
    let config = write_named(dir, "config.toml", &openrouter_config());
    let fixture_arg = utf8_arg(&fixture, "the catalog path should be UTF-8");
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    (config, fixture_arg, config_arg)
}

#[test]
fn benchmarks_help_names_the_feed_and_says_it_does_not_modify_the_config() {
    let dir = temp_root("help");
    let output = run(&dir, &["benchmarks", "--help"]);
    let help = stdout_text(&output);
    assert!(
        output.status.success(),
        "benchmarks --help should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        help.contains("grok-router benchmarks --catalog-file models.json --config config.toml"),
        "benchmarks --help should contain the example; help: {help}"
    );
    assert!(
        help.contains("does not modify the config"),
        "benchmarks --help should say the command does not modify the config; help: {help}"
    );
    assert!(
        help.contains("openrouter-models"),
        "benchmarks --help should name the feed; help: {help}"
    );
    let root = stdout_text(&run(&dir, &["--help"]));
    assert!(
        root.contains("benchmarks"),
        "root help should list benchmarks; help: {root}"
    );
    remove_temp(&dir);
}

#[test]
fn benchmarks_json_ranks_scored_rows_and_leaves_the_config_unchanged() {
    let dir = temp_root("json");
    let (config, fixture_arg, config_arg) = openrouter_paths(&dir, FIXTURE);
    let before = fs::read(&config).expect("the config fixture should be readable");
    let output = run(
        &dir,
        &[
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
            "--output",
            "json",
        ],
    );
    let stdout = stdout_text(&output);
    let stderr = stderr_text(&output);
    assert!(
        output.status.success(),
        "benchmarks --json should succeed; stderr: {stderr}"
    );
    assert!(
        stderr.is_empty(),
        "success should keep stderr empty; stderr: {stderr}"
    );
    assert!(
        !stdout.contains("9999") && !stdout.contains("design_arena"),
        "design arena must not be in the JSON; stdout: {stdout}"
    );
    assert!(
        stdout.contains("\"intelligence_index\":55.1")
            && stdout.contains("\"prompt_per_million\":\"2\""),
        "JSON should keep the index number and the per-million price; stdout: {stdout}"
    );
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(stdout.trim()).expect("benchmarks JSON should be an array");
    let ids: Vec<&str> = rows
        .iter()
        .filter_map(|row| row.get("id").and_then(serde_json::Value::as_str))
        .collect();
    assert_eq!(
        ids,
        ["high/two", "tie/four", "low/one"],
        "default order is intelligence, then catalog order, and a blank score is omitted"
    );
    let high = rows.first().expect("the highest row");
    assert_eq!(
        high.get("name").and_then(serde_json::Value::as_str),
        Some("High Two")
    );
    assert_eq!(
        high.get("completion_per_million")
            .and_then(serde_json::Value::as_str),
        Some("10")
    );
    assert!(
        high.get("coding_index")
            .is_some_and(serde_json::Value::is_null),
        "a null coding index stays null; row: {high}"
    );
    assert_eq!(
        high.get("agentic_index")
            .and_then(serde_json::Value::as_i64),
        Some(3)
    );
    let tie = rows.get(1).expect("the tied row");
    assert_eq!(
        tie.get("prompt_per_million")
            .and_then(serde_json::Value::as_str),
        Some("0.079")
    );
    assert_eq!(
        fs::read(&config).expect("the config should still be readable"),
        before,
        "benchmarks must not write the config"
    );
    remove_temp(&dir);
}

#[test]
fn benchmarks_table_shows_price_and_leaves_blank_scores_out() {
    let dir = temp_root("table");
    let (_config, fixture_arg, config_arg) = openrouter_paths(&dir, FIXTURE);
    let output = run(
        &dir,
        &[
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
        ],
    );
    let stdout = stdout_text(&output);
    assert!(
        output.status.success(),
        "the table should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert!(
        stdout.contains("PROMPT$/M")
            && stdout.contains("COMPLETION$/M")
            && stdout.contains("INTELLIGENCE"),
        "the table should name the columns; stdout: {stdout}"
    );
    assert!(
        stdout.contains("high/two") && stdout.contains("55.1") && stdout.contains("0.079"),
        "the table should show the index and the shifted price; stdout: {stdout}"
    );
    assert!(
        !stdout.contains("blank/three"),
        "a row with no intelligence index is not in the default table; stdout: {stdout}"
    );
    remove_temp(&dir);
}

#[test]
fn benchmarks_quiet_and_limit_follow_the_ranked_order() {
    let dir = temp_root("quiet");
    let (_config, fixture_arg, config_arg) = openrouter_paths(&dir, FIXTURE);
    let quiet = run(
        &dir,
        &[
            "--quiet",
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
        ],
    );
    assert!(
        quiet.status.success(),
        "quiet benchmarks should succeed; stderr: {}",
        stderr_text(&quiet)
    );
    assert_eq!(
        stdout_text(&quiet),
        "high/two\ntie/four\nlow/one\n",
        "quiet output is one ranked id per line"
    );
    let limited = run(
        &dir,
        &[
            "--quiet",
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
            "--limit",
            "1",
        ],
    );
    assert_eq!(
        stdout_text(&limited),
        "high/two\n",
        "limit keeps the highest intelligence index"
    );
    remove_temp(&dir);
}

#[test]
fn a_term_keeps_catalog_order_and_includes_a_blank_score() {
    let dir = temp_root("term");
    let (_config, fixture_arg, config_arg) = openrouter_paths(&dir, FIXTURE);
    let output = run(
        &dir,
        &[
            "--quiet",
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
            "kimi",
        ],
    );
    assert!(
        output.status.success(),
        "a term match should succeed; stderr: {}",
        stderr_text(&output)
    );
    assert_eq!(
        stdout_text(&output),
        "high/two\nblank/three\n",
        "kimi matches the scored row and the blank row, in catalog order"
    );
    let table = run(
        &dir,
        &[
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
            "kimi",
        ],
    );
    let text = stdout_text(&table);
    assert!(
        text.contains("blank/three") && text.contains('-'),
        "the blank score and the unpublished price print as a dash; stdout: {text}"
    );
    let missed = run(
        &dir,
        &[
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
            "zzzz",
        ],
    );
    let stderr = stderr_text(&missed);
    assert!(
        !missed.status.success(),
        "a miss should fail; stdout: {}",
        stdout_text(&missed)
    );
    assert!(
        stderr.contains("0 matches") && stderr.contains("zzzz"),
        "a miss should name the count and the term; stderr: {stderr}"
    );
    remove_temp(&dir);
}

#[test]
fn no_intelligence_index_asks_for_a_term_and_a_term_still_prints_the_row() {
    let dir = temp_root("unscored");
    let (_config, fixture_arg, config_arg) = openrouter_paths(&dir, UNSCORED);
    let bare = run(
        &dir,
        &[
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
        ],
    );
    let stderr = stderr_text(&bare);
    assert!(!bare.status.success(), "no index should fail");
    assert!(
        stderr.contains("0 rows") && stderr.contains("Pass a term"),
        "the empty ranking should say what to do next; stderr: {stderr}"
    );
    let named = run(
        &dir,
        &[
            "--quiet",
            "benchmarks",
            "--catalog-file",
            &fixture_arg,
            "--config",
            &config_arg,
            "plain",
        ],
    );
    assert_eq!(
        stdout_text(&named),
        "plain/model\n",
        "a term prints the row even when it has no score; stderr: {}",
        stderr_text(&named)
    );
    remove_temp(&dir);
}

/// Run benchmarks against a catalog file that does not exist. Returns stderr.
///
/// The missing path must still be missing, and the refusal must not be a read error.
fn refuse_unread_catalog(dir: &Path, config_text: &str) -> String {
    let missing = dir.join("missing.json");
    let missing_arg = utf8_arg(&missing, "the missing catalog path should be UTF-8");
    let config = write_named(dir, "config.toml", config_text);
    let config_arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(
        dir,
        &[
            "benchmarks",
            "--config",
            &config_arg,
            "--catalog-file",
            &missing_arg,
        ],
    );
    let stderr = stderr_text(&output);
    assert!(
        !output.status.success(),
        "the feed should refuse; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("could not read"),
        "the refusal happens before the catalog file is read; stderr: {stderr}"
    );
    assert!(
        !missing.exists(),
        "the missing catalog file should stay missing"
    );
    stderr
}

#[test]
fn deepseek_refuses_the_benchmark_feed_before_a_download() {
    let dir = temp_root("deepseek");
    let stderr = refuse_unread_catalog(
        &dir,
        &provider_record(
            "default_provider = \"deepseek\"\n",
            "deepseek",
            "https://api.deepseek.com",
            "DEEPSEEK_API_KEY",
            Some("deepseek-list-models"),
        ),
    );
    assert!(
        stderr.contains("no benchmark feed") && stderr.contains("deepseek-list-models"),
        "deepseek should name the missing feed; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("DEEPSEEK_API_KEY"),
        "the refusal happens before a key check; stderr: {stderr}"
    );
    remove_temp(&dir);
}

#[test]
fn a_provider_with_no_catalog_refuses_before_a_download() {
    let dir = temp_root("zai");
    let stderr = refuse_unread_catalog(
        &dir,
        &provider_record(
            "default_provider = \"zai\"\n",
            "zai",
            "https://api.z.ai/api/paas/v4",
            "ZAI_API_KEY",
            None,
        ),
    );
    assert!(
        stderr.contains("no benchmark feed") && stderr.contains("No catalog is set"),
        "a provider with no catalog should say so; stderr: {stderr}"
    );
    remove_temp(&dir);
}

#[test]
fn an_unknown_catalog_is_not_offered_the_deepseek_feed() {
    let dir = temp_root("unknown");
    let stderr = refuse_unread_catalog(
        &dir,
        &provider_record(
            "default_provider = \"custom\"\n",
            "custom",
            "https://example.test/v1",
            "CUSTOM_API_KEY",
            Some("not-a-cartridge"),
        ),
    );
    assert!(
        stderr.contains("not-a-cartridge") && stderr.contains("openrouter-models"),
        "an unknown catalog should name the id and the feed that works; stderr: {stderr}"
    );
    assert!(
        !stderr.contains("deepseek-list-models"),
        "an unknown catalog must not offer deepseek as the benchmark feed; stderr: {stderr}"
    );
    remove_temp(&dir);
}

#[test]
fn timeout_zero_and_an_empty_term_refuse_before_a_catalog_read() {
    let dir = temp_root("usage");
    let (config, fixture_arg, config_arg) = openrouter_paths(&dir, FIXTURE);
    let before = fs::read(&config).expect("the config should be readable");
    let missing = dir.join("absent.json");
    let missing_arg = utf8_arg(&missing, "the absent catalog path should be UTF-8");
    let timeout = run(
        &dir,
        &[
            "benchmarks",
            "--config",
            &config_arg,
            "--catalog-file",
            &missing_arg,
            "--timeout",
            "0",
        ],
    );
    let timeout_err = stderr_text(&timeout);
    assert!(!timeout.status.success(), "timeout 0 should fail");
    assert!(
        timeout_err.contains("timeout") && !timeout_err.contains("could not read"),
        "timeout 0 refuses before the catalog file is read; stderr: {timeout_err}"
    );
    let empty = run(
        &dir,
        &[
            "benchmarks",
            "--config",
            &config_arg,
            "--catalog-file",
            &fixture_arg,
            "",
        ],
    );
    let empty_err = stderr_text(&empty);
    assert!(!empty.status.success(), "an empty term should fail");
    assert!(
        empty_err.contains("empty benchmark term"),
        "an empty term should name itself; stderr: {empty_err}"
    );
    assert_eq!(
        fs::read(&config).expect("the config should still be readable"),
        before,
        "a refused benchmarks command must not write the config"
    );
    remove_temp(&dir);
}

#[test]
fn a_missing_config_is_not_created() {
    let dir = temp_root("missing-config");
    let missing = dir.join("config.toml");
    let missing_arg = utf8_arg(&missing, "the missing config path should be UTF-8");
    let output = run(&dir, &["benchmarks", "--config", &missing_arg]);
    let stderr = stderr_text(&output);
    assert!(!output.status.success(), "a missing config should fail");
    assert!(
        stderr.contains("[router]") && stderr.contains("must write that record"),
        "a missing config should name the missing router table; stderr: {stderr}"
    );
    assert!(
        !stderr.contains(COMPILED_URL),
        "a missing config must not choose the compiled OpenRouter URL; stderr: {stderr}"
    );
    assert!(!missing.exists(), "benchmarks must not create the config");
    remove_temp(&dir);
}
