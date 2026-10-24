//! `list` subcommand.
//!
//! List reads the config and prints the saved default and every configured
//! model. It does not modify the config. With no subcommand, the same read runs.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config_edit::{self, ConfiguredInventory, RefuseReason};

/// Adds the `list` subcommand and its flags.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(
        Command::new("list")
            .about("List configured models and the saved default. This command does not modify the config.")
            .after_help("Examples:\n  grok-router list --config config.toml")
            .arg(
                Arg::new("json")
                    .long("json")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("quiet")
                    .help("Print a JSON array of slug, provider id, and whether it is the default."),
            )
            .arg(
                Arg::new("quiet")
                    .long("quiet")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("json")
                    .help("Print one slug per line."),
            ),
    )
}

/// Runs `list`, including the no-args path.
///
/// Returns `0` when the inventory was printed and `1` when the config could not be read.
#[must_use]
pub fn run(matches: &ArgMatches) -> i32 {
    let sub = matches.subcommand_matches("list");
    if matches.subcommand_name().is_some() && sub.is_none() {
        return fail(
            "list was not selected. Run `grok-router list --help` for the flags and an example.",
        );
    }
    match list_models(matches, sub) {
        Ok(body) => write_stdout(&body),
        Err(message) => fail(&message),
    }
}

/// Read the config and render the inventory. Does not write the file.
fn list_models(root: &ArgMatches, sub: Option<&ArgMatches>) -> Result<String, String> {
    let path = config_path(root, sub)?;
    let text = read_config_text(&path)?;
    let inventory = config_edit::read_configured(&text).map_err(|refused| read_failure(&refused.reason))?;
    let rows = display_rows(&inventory);
    render(&rows, flag(sub, "json"), flag(sub, "quiet"))
}

/// `--config` from the subcommand or the root. Otherwise `${GROK_HOME:-$HOME/.grok}/config.toml`.
fn config_path(root: &ArgMatches, sub: Option<&ArgMatches>) -> Result<PathBuf, String> {
    let from_sub = sub.and_then(|matches| matches.get_one::<String>("config"));
    let from_root = root.get_one::<String>("config");
    match from_sub.or(from_root).map(String::as_str) {
        Some("") => Err(
            "--config is empty, so the models were not listed. Pass --config with the file to read."
                .to_owned(),
        ),
        Some(path) => Ok(PathBuf::from(path)),
        None => default_config_path(),
    }
}

/// `${GROK_HOME:-$HOME/.grok}/config.toml`. An empty `GROK_HOME` falls through to `HOME`.
fn default_config_path() -> Result<PathBuf, String> {
    if let Some(grok_home) = env::var_os("GROK_HOME") {
        if !grok_home.is_empty() {
            return Ok(PathBuf::from(grok_home).join("config.toml"));
        }
    }
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".grok").join("config.toml")),
        Some(_) => Err(
            "HOME is empty and GROK_HOME is unset, so there is no config path. Pass --config with the file to read."
                .to_owned(),
        ),
        None => Err(
            "HOME is not set and GROK_HOME is unset, so there is no config path. Pass --config with the file to read."
                .to_owned(),
        ),
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
fn read_config_text(path: &Path) -> Result<String, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(format!(
                "could not read config {}. The file is missing, so the models were not listed. Create the file or pass --config with an existing file.",
                path.display()
            ));
        }
        Err(err) => {
            return Err(format!(
                "could not read config {}. {err}. The models were not listed. Pass --config pointing at a readable file.",
                path.display()
            ));
        }
    };
    String::from_utf8(bytes).map_err(|_| {
        format!(
            "config {} is not UTF-8, so the models were not listed. Save it as UTF-8 and pass that --config again.",
            path.display()
        )
    })
}

/// What failed, why, and what to do next when the config text cannot be read.
fn read_failure(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The models were not listed. Fix the config so it parses as TOML, or pass --config with a readable file."
    )
}

/// A flag from the `list` subcommand. No-args has neither `--json` nor `--quiet`.
fn flag(sub: Option<&ArgMatches>, name: &str) -> bool {
    sub.is_some_and(|matches| matches.get_flag(name))
}

/// Configured models in slug order, plus the saved default when it has no block.
fn display_rows(inventory: &ConfiguredInventory) -> Vec<Row> {
    let mut rows = Vec::new();
    for model in &inventory.models {
        rows.push(Row {
            slug: model.slug.clone(),
            provider_id: Some(model.provider_id.clone()),
            is_default: inventory.saved_default.as_deref() == Some(model.slug.as_str()),
        });
    }
    if let Some(slug) = &inventory.saved_default {
        let already = rows.iter().any(|row| row.slug == *slug);
        if !already {
            rows.push(Row {
                slug: slug.clone(),
                provider_id: None,
                is_default: true,
            });
        }
    }
    rows
}

/// JSON, one slug per line, or text that marks the default.
fn render(rows: &[Row], json: bool, quiet: bool) -> Result<String, String> {
    if json {
        return format_json(rows);
    }
    if quiet {
        return Ok(format_quiet(rows));
    }
    Ok(format_text(rows))
}

/// JSON array. `provider_id` is omitted when the slug has no block.
fn format_json(rows: &[Row]) -> Result<String, String> {
    let hits = rows
        .iter()
        .map(|row| JsonRow {
            slug: row.slug.as_str(),
            provider_id: row.provider_id.as_deref(),
            default: row.is_default,
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&hits).map_err(|err| {
        format!(
            "could not encode the model list as JSON: {err}. Retry the list; report this if it persists."
        )
    })
}

/// One slug per line and no default marker.
fn format_quiet(rows: &[Row]) -> String {
    rows.iter()
        .map(|row| row.slug.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Slug, provider id when there is one, and the word `default` on the saved default.
fn format_text(rows: &[Row]) -> String {
    let mut lines = Vec::new();
    for row in rows {
        lines.push(text_line(row));
    }
    lines.join("\n")
}

/// One text row.
fn text_line(row: &Row) -> String {
    match (&row.provider_id, row.is_default) {
        (Some(provider), true) => format!("{}  {}  default", row.slug, provider),
        (Some(provider), false) => format!("{}  {}", row.slug, provider),
        (None, true) => format!("{}  default", row.slug),
        (None, false) => row.slug.clone(),
    }
}

/// Write the body and a trailing newline. A closed pipe is success.
fn write_stdout(body: &str) -> i32 {
    let mut stdout = io::stdout().lock();
    let mut bytes = body.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    match stdout.write_all(&bytes) {
        Ok(()) => 0,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(err) => fail(&format!(
            "could not write the model list: {err}. Retry when stdout accepts output."
        )),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
}

/// One printed model.
struct Row {
    /// Config slug, or the saved default when that slug has no block.
    slug: String,
    /// Provider id. Absent for a saved default that has no block.
    provider_id: Option<String>,
    /// Whether this slug is `[models].default`.
    is_default: bool,
}

/// One JSON object in the list array.
#[derive(serde::Serialize)]
struct JsonRow<'a> {
    /// Config slug, or the saved default when that slug has no block.
    slug: &'a str,
    /// Provider id. Omitted when the slug has no block.
    #[serde(skip_serializing_if = "Option::is_none")]
    provider_id: Option<&'a str>,
    /// Whether this slug is `[models].default`.
    default: bool,
}
