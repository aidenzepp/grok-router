//! `list` subcommand.
//!
//! List reads the config and prints the saved default and every configured
//! model. It does not modify the config. With no subcommand, the same read runs.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::config_edit::{self, ConfiguredInventory, RefuseReason};
use crate::error::Error;
use crate::OutputMode;

/// List configured models and the saved default. This command does not modify the config.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router list --config config.toml\n  grok-router list --config config.toml --limit 1"
)]
pub struct ListArgs {
    /// Maximum rows to print.
    ///
    /// Omitting --limit lists every row.
    #[arg(long, value_name = "N", verbatim_doc_comment)]
    limit: Option<String>,
}

impl ListArgs {
    /// `--limit` when it was passed. Absent lists every row.
    #[must_use]
    pub fn limit(&self) -> Option<&str> {
        self.limit.as_deref()
    }
}

/// Runs `list`, including the no-args path.
///
/// No subcommand uses the same read. `mode` is the resolved global output.
///
/// Returns `0` when the inventory was printed and `1` when the config could not be read.
#[must_use]
pub fn run(config: Option<&str>, limit: Option<&str>, mode: OutputMode) -> i32 {
    match list_models(config, limit, mode) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// Read the config and render the inventory. Does not write the file.
fn list_models(
    config: Option<&str>,
    limit: Option<&str>,
    mode: OutputMode,
) -> Result<String, Error> {
    let path = config_path(config)?;
    let text = read_config_text(&path)?;
    let inventory =
        config_edit::read_configured(&text).map_err(|refused| read_failure(refused.reason))?;
    let rows = limited(display_rows(&inventory), limit)?;
    render(&rows, mode)
}

/// `--config` when it was passed. Otherwise `${GROK_HOME:-$HOME/.grok}/config.toml`.
fn config_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some("") => Err(Error::ListConfigEmpty),
        Some(path) => Ok(PathBuf::from(path)),
        None => default_config_path(),
    }
}

/// `${GROK_HOME:-$HOME/.grok}/config.toml`. An empty `GROK_HOME` falls through to `HOME`.
fn default_config_path() -> Result<PathBuf, Error> {
    if let Some(grok_home) = env::var_os("GROK_HOME") {
        if !grok_home.is_empty() {
            return Ok(PathBuf::from(grok_home).join("config.toml"));
        }
    }
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".grok").join("config.toml")),
        Some(_) => Err(Error::ListHomeEmpty),
        None => Err(Error::ListHomeUnset),
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
fn read_config_text(path: &Path) -> Result<String, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(Error::ListConfigMissing {
                path: path.to_path_buf(),
            });
        }
        Err(err) => {
            return Err(Error::ListConfigIo {
                path: path.to_path_buf(),
                source: err,
            });
        }
    };
    String::from_utf8(bytes).map_err(|_| Error::ListConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// What failed, why, and what to do next when the config text cannot be read.
fn read_failure(reason: RefuseReason) -> Error {
    Error::ListConfigRefused { reason }
}

/// Keep the first `limit` rows. Omitting the flag keeps every row, in the same order.
///
/// # Errors
///
/// Returns an error when the value is zero or not a whole number. Nothing is printed.
fn limited(rows: Vec<Row>, limit: Option<&str>) -> Result<Vec<Row>, Error> {
    let Some(raw) = limit else {
        return Ok(rows);
    };
    let Ok(count) = raw.parse::<usize>() else {
        return Err(Error::LimitNotWhole {
            raw: raw.to_owned(),
        });
    };
    if count == 0 {
        return Err(Error::LimitZero);
    }
    Ok(rows.into_iter().take(count).collect())
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
fn render(rows: &[Row], mode: OutputMode) -> Result<String, Error> {
    match mode {
        OutputMode::Json => format_json(rows),
        OutputMode::Quiet => Ok(format_quiet(rows)),
        OutputMode::Text => Ok(format_text(rows)),
    }
}

/// JSON array. `provider_id` is omitted when the slug has no block.
fn format_json(rows: &[Row]) -> Result<String, Error> {
    let hits = rows
        .iter()
        .map(|row| JsonRow {
            slug: row.slug.as_str(),
            provider_id: row.provider_id.as_deref(),
            default: row.is_default,
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&hits).map_err(|err| Error::JsonEncode {
        what: "the model list",
        source: err,
        retry: "Retry the list; report this if it persists.",
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
fn write_stdout(mode: OutputMode, body: &str) -> i32 {
    let mut stdout = io::stdout().lock();
    let mut bytes = body.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    match stdout.write_all(&bytes) {
        Ok(()) => 0,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(err) => fail(
            mode,
            &Error::Stdout {
                what: "the model list",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "list", err)
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
