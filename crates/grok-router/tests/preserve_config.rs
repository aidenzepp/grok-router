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

//! Lived-in config bytes survive `add`, `set-default`, and `remove`.
//!
//! The fixture is not a tidy sample. It carries a comment, an irregular blank
//! gap, and tables the model edit does not own. These tests pass `--config`
//! and `--catalog-file`. They do not read a home config and do not write one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ODD_GAP: &str = "theme = \"plain\"\n\n\n\n[ui]\n";

const GLM_SPAN: &str = "\
[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 600000
";

const NEW_BLOCK: &str = "\
[model.or-widget-v2]
model = \"acme/widget.v2\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"Widget\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 4096
";

const LIVED_IN: &str = "\
# lived-in comment: leave this line alone

[cli]
theme = \"plain\"\n\
\n\
\n\
\n\
[ui]
compact = true

[[marketplace.sources]]
name = \"local\"

[privacy]
share_prompts = false

[model.or-deepseek-flash]
model = \"deepseek/deepseek-v4.1-flash-1017\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000
max_completion_tokens = 65536

[model.or-deepseek-pro]
model = \"deepseek/deepseek-v4-pro-0813\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"DeepSeek Pro\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000

[model.or-glm-5-3]
model = \"z-ai/glm-5.3\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 600000

[model.or-glm-5-3-flash]
model = \"z-ai/glm-5.3-flash\"
base_url = \"https://openrouter.ai/api/v1\"
name = \"GLM 5.3 Flash\"
env_key = \"OPENROUTER_API_KEY\"
api_backend = \"chat_completions\"
context_window = 1000000

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
            "id": "acme/widget.v2",
            "name": "Widget",
            "context_length": 4096,
            "architecture": { "input_modalities": ["text"] }
        },
        {
            "id": "z-ai/glm-5.3",
            "name": "GLM 5.3",
            "context_length": 1048576,
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
    let path = std::env::temp_dir().join(format!(
        "grok-router-preserve-{label}-{}",
        std::process::id()
    ));
    remove_temp(&path);
    fs::create_dir_all(&path).expect("the temp preserve directory should be created");
    path
}

fn write_fixture(dir: &Path) -> (PathBuf, PathBuf) {
    let config = dir.join("config.toml");
    let catalog = dir.join("models.json");
    fs::write(&config, LIVED_IN).expect("the lived-in fixture should be written");
    fs::write(&catalog, CATALOG).expect("the catalog fixture should be written");
    (config, catalog)
}

fn utf8_arg(path: &Path, what: &str) -> String {
    path.to_str().expect(what).to_owned()
}

fn run(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_grok-router"))
        .args(args)
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .env_remove("EXAMPLE_API_KEY")
        .env_remove("GROK_ROUTER_OUTPUT")
        .env_remove("GROK_ROUTER_QUIET")
        .env_remove("GROK_ROUTER_TIMEOUT")
        .env("HOME", dir)
        .env("NO_COLOR", "1")
        .current_dir(dir)
        .output()
        .expect("the grok-router binary should start")
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("preserve stdout should be UTF-8")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("preserve stderr should be UTF-8")
}

fn exit_code(output: &std::process::Output) -> i32 {
    output
        .status
        .code()
        .expect("the child should have an exit status")
}

fn read_config(path: &Path) -> String {
    fs::read_to_string(path).expect("the temp config should stay UTF-8")
}

fn assert_lived_in_outside_models(text: &str) {
    assert!(
        text.contains("# lived-in comment: leave this line alone\n"),
        "the comment should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains(ODD_GAP),
        "the irregular blank gap between [cli] and [ui] should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("[cli]\ntheme = \"plain\"\n"),
        "the [cli] table should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("[ui]\ncompact = true\n"),
        "the [ui] table should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("[[marketplace.sources]]\nname = \"local\"\n"),
        "the marketplace array should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains("[privacy]\nshare_prompts = false\n"),
        "the [privacy] table should stay byte-identical; config: {text}"
    );
    assert!(
        text.contains(GLM_SPAN),
        "the or-glm-5-3 span, including context_window = 600000, should stay byte-identical; config: {text}"
    );
    assert!(
        text.ends_with('\n'),
        "the config should still end with a newline; config: {text}"
    );
}

#[test]
fn add_inserts_only_the_new_block_and_keeps_every_other_byte() {
    let dir = temp_root("add");
    let (config, catalog) = write_fixture(&dir);
    let source = format!("{OPENROUTER_RECORD}\n{LIVED_IN}");
    fs::write(&config, &source).expect("the add fixture should include a provider record");
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
        0,
        "adding a different provider should exit 0; stderr: {stderr}; stdout: {}",
        stdout_text(&output)
    );
    let edited = read_config(&config);
    let models_at = source
        .rfind("[models]\n")
        .expect("the fixture should end with one [models] table");
    let head = source
        .get(..models_at)
        .expect("the [models] header should sit inside the fixture");
    let tail = source
        .get(models_at..)
        .expect("the [models] header should start the fixture tail");
    assert!(
        head.ends_with("\n\n"),
        "the fixture should already keep one blank line before [models]"
    );
    let expected = format!("{head}{NEW_BLOCK}\n{tail}");
    assert_eq!(
        edited, expected,
        "add should insert only the new block and the blank line that separates it from [models]"
    );
    let block: toml::Table = NEW_BLOCK
        .parse()
        .expect("the inserted block should be readable TOML");
    let model = block
        .get("model")
        .and_then(toml::Value::as_table)
        .and_then(|models| models.get("or-widget-v2"))
        .and_then(toml::Value::as_table)
        .expect("the inserted block should contain [model.or-widget-v2]");
    assert_eq!(
        model
            .get("context_window")
            .and_then(toml::Value::as_integer),
        Some(4096),
        "the new block should record the catalog context_length"
    );
    assert!(
        edited.contains(&format!("\n\n{NEW_BLOCK}\n[models]\n")),
        "one blank line should surround the new block, and [models] should follow it; config: {edited}"
    );
    assert_lived_in_outside_models(&edited);
    assert!(
        edited.contains("default = \"grok-4.7\"\n"),
        "add should not change the saved default; config: {edited}"
    );
    remove_temp(&dir);
}

#[test]
fn set_default_changes_only_the_default_line() {
    let dir = temp_root("set-default");
    let (config, _) = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "set-default",
            "grok-4.6",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "set-default grok-4.6 should exit 0; stderr: {stderr}; stdout: {}",
        stdout_text(&output)
    );
    let edited = read_config(&config);
    let expected = LIVED_IN.replacen("default = \"grok-4.7\"\n", "default = \"grok-4.6\"\n", 1);
    assert_eq!(
        edited, expected,
        "set-default should change only the default line"
    );
    assert_eq!(
        LIVED_IN.matches("default = \"grok-4.7\"\n").count(),
        1,
        "the fixture should contain one default line to replace"
    );
    assert_lived_in_outside_models(&edited);
    remove_temp(&dir);
}

#[test]
fn remove_deletes_only_the_named_model_span() {
    let dir = temp_root("remove");
    let (config, _) = write_fixture(&dir);
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
            "remove",
            "--yes",
            "or-deepseek-pro",
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "remove --yes of a non-default slug should exit 0; stderr: {stderr}; stdout: {}",
        stdout_text(&output)
    );
    let edited = read_config(&config);
    let start = LIVED_IN
        .find("[model.or-deepseek-pro]\n")
        .expect("the fixture should contain the or-deepseek-pro span");
    let end = LIVED_IN
        .find("[model.or-glm-5-3]\n")
        .expect("the glm span should follow the deleted span");
    let expected = format!("{}{}", &LIVED_IN[..start], &LIVED_IN[end..]);
    assert_eq!(
        edited, expected,
        "remove should delete only the or-deepseek-pro span, including the blank lines inside that span"
    );
    assert!(
        !edited.contains("[model.or-deepseek-pro]"),
        "the deleted header should be gone; config: {edited}"
    );
    assert_lived_in_outside_models(&edited);
    assert!(
        edited.contains("default = \"grok-4.7\"\n"),
        "remove should leave the saved default; config: {edited}"
    );
    remove_temp(&dir);
}

#[test]
fn refused_edits_leave_the_fixture_byte_identical() {
    let cases: [(&str, &[&str]); 3] = [
        (
            "add-configured",
            &[
                "add",
                "--catalog-file",
                "models.json",
                "--as",
                "or-glm-copy",
                "z-ai/glm-5.3",
            ],
        ),
        ("set-unknown", &["set-default", "not-a-model"]),
        ("remove-default", &["remove", "--yes", "grok-4.7"]),
    ];
    for (label, command) in cases {
        let dir = temp_root(label);
        let (config, _) = write_fixture(&dir);
        let source = if label == "add-configured" {
            format!("{OPENROUTER_RECORD}\n{LIVED_IN}")
        } else {
            LIVED_IN.to_owned()
        };
        fs::write(&config, &source).expect("the refusal fixture should be written");
        let config_arg = utf8_arg(&config, "the config path should be UTF-8");
        let mut args = vec!["--config", config_arg.as_str()];
        args.extend_from_slice(command);
        let output = run(&dir, &args);
        let stderr = stderr_text(&output);
        assert_ne!(
            exit_code(&output),
            0,
            "{label} should refuse; stdout: {}",
            stdout_text(&output)
        );
        assert!(
            stderr
                .to_ascii_lowercase()
                .contains("file was left unchanged"),
            "{label} should say that the file was left unchanged; stderr: {stderr}"
        );
        let edited = read_config(&config);
        assert_eq!(
            edited, source,
            "{label} should leave the fixture byte-identical"
        );
        assert!(
            !config.with_file_name("config.toml.prev").exists(),
            "{label} should not write a backup when it refuses"
        );
        remove_temp(&dir);
    }
}

const DIRECT_BLOCK: &str = "\
[model.claude-opus]
model = \"claude-opus-5-5\"
base_url = \"https://proxy.tokenfix.trilogy.com/v1\"
name = \"Claude Opus (TokenFix)\"
api_backend = \"messages\"
env_key = \"ANTHROPIC_AUTH_TOKEN\"
";

#[test]
fn direct_add_inserts_only_the_supplied_block() {
    let dir = temp_root("direct-add");
    let config = dir.join("config.toml");
    fs::write(&config, LIVED_IN).expect("the direct fixture should be written");
    let output = run(
        &dir,
        &[
            "--config",
            &utf8_arg(&config, "the config path should be UTF-8"),
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
        ],
    );
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "a direct add should insert one block; stderr: {stderr}; stdout: {}",
        stdout_text(&output)
    );
    let edited = read_config(&config);
    let models_at = LIVED_IN
        .rfind("[models]\n")
        .expect("the fixture should end with one [models] table");
    let head = LIVED_IN
        .get(..models_at)
        .expect("the [models] header should sit inside the fixture");
    let tail = LIVED_IN
        .get(models_at..)
        .expect("the [models] header should start the fixture tail");
    assert!(
        head.ends_with("\n\n"),
        "the fixture should already keep one blank line before [models]"
    );
    let expected = format!("{head}{DIRECT_BLOCK}\n{tail}");
    assert_eq!(
        edited, expected,
        "direct add should insert only the supplied block and the blank line that separates it from [models]"
    );
    assert_lived_in_outside_models(&edited);
    remove_temp(&dir);
}

#[test]
fn init_appends_the_openrouter_block_and_keeps_lived_in_bytes() {
    let dir = temp_root("init");
    let (config, _) = write_fixture(&dir);
    let before = fs::read(&config).expect("the lived-in fixture should be readable");
    let arg = utf8_arg(&config, "the config path should be UTF-8");
    let output = run(&dir, &["--config", &arg, "init"]);
    let stderr = stderr_text(&output);
    assert_eq!(
        exit_code(&output),
        0,
        "init should append to the lived-in fixture; stderr: {stderr}; stdout: {}",
        stdout_text(&output)
    );
    let edited = read_config(&config);
    let block = grok_router::config_edit::openrouter_router_block();
    assert_eq!(
        edited,
        format!("{LIVED_IN}{block}"),
        "init should keep every lived-in byte and append the router block"
    );
    assert_lived_in_outside_models(&edited);
    assert_eq!(
        fs::read(config.with_file_name("config.toml.prev"))
            .expect("init should back up an existing file"),
        before,
        "the backup should be the lived-in bytes from before the write"
    );
    let again = run(&dir, &["--config", &arg, "init"]);
    assert_eq!(
        exit_code(&again),
        0,
        "a second init of the lived-in file should exit 0; stderr: {}",
        stderr_text(&again)
    );
    assert_eq!(
        read_config(&config),
        edited,
        "a second init should leave the lived-in file byte-identical"
    );
    remove_temp(&dir);
}
