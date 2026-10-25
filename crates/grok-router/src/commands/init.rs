//! `init` subcommand.
//!
//! Writes a default `OpenRouter` `[router]` table when the file has none.
//! `--config` selects the file. Otherwise the path is
//! `${GROK_HOME:-$HOME/.grok}/config.toml`. `--dry-run` does not create or replace the file.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::OutputMode;
use crate::config_edit::{self, Backup, RouterInit, RouterInitError};
use crate::error::Error;

/// Write a default `OpenRouter` [router] table when the file has none. An existing [router] table is left unchanged. This command writes unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router init\n  grok-router init --config config.toml\n  grok-router init --dry-run --config config.toml"
)]
pub struct InitArgs {
    /// Print the path and whether it would write, then do not change the file.
    #[arg(long, verbatim_doc_comment)]
    dry_run: bool,
}

/// Runs `init`.
///
/// Returns `0` when the block was written, when `[router]` was already a table,
/// or when `--dry-run` would write or would write nothing.
/// Returns `1` when nothing was written because init refused.
#[must_use]
pub fn run(config: Option<&str>, args: &InitArgs, mode: OutputMode) -> i32 {
    match apply(config, args.dry_run, mode) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// Resolve the path, append the router block unless one is already a table, and write unless this is `--dry-run`.
///
/// # Errors
///
/// Returns an error when the path cannot be resolved, the existing `[router]` value is not a table, or the write fails.
fn apply(config: Option<&str>, dry_run: bool, mode: OutputMode) -> Result<String, Error> {
    let path = config_path(config)?;
    let report = match read_config(&path)? {
        None => write_missing(&path, dry_run)?,
        Some(text) => write_existing(&path, &text, dry_run)?,
    };
    render(mode, &report)
}

/// `--config` when it was passed. Otherwise `${GROK_HOME:-$HOME/.grok}/config.toml`.
///
/// # Errors
///
/// Returns an error when `--config` is empty, or when no default path can be named.
/// Nothing is written.
fn config_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some("") => Err(Error::InitConfigEmpty),
        Some(path) => Ok(PathBuf::from(path)),
        None => default_config_path(),
    }
}

/// `${GROK_HOME:-$HOME/.grok}/config.toml`. An empty `GROK_HOME` falls through to `HOME`.
///
/// # Errors
///
/// Returns an error when `GROK_HOME` is unset or empty and `HOME` cannot name a path.
/// Nothing is written.
fn default_config_path() -> Result<PathBuf, Error> {
    if let Some(grok_home) = env::var_os("GROK_HOME")
        && !grok_home.is_empty()
    {
        return Ok(PathBuf::from(grok_home).join("config.toml"));
    }
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".grok").join("config.toml")),
        Some(_) => Err(Error::InitHomeEmpty),
        None => Err(Error::InitHomeUnset),
    }
}

/// Read `path`. [`None`] means the file is missing and may be created.
///
/// # Errors
///
/// Returns an error when the file exists but cannot be read or is not UTF-8.
/// The file is left unchanged.
fn read_config(path: &Path) -> Result<Option<String>, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(Error::InitConfigIo {
                path: path.to_path_buf(),
                source: err,
            });
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => Ok(Some(text)),
        Err(_) => Err(Error::InitConfigUtf8 {
            path: path.to_path_buf(),
        }),
    }
}

/// Create the missing file as the `OpenRouter` block. A missing file has nothing to back up.
///
/// `replace_file` reads the existing bytes before it writes, so a new file uses [`config_edit::create_file`].
///
/// # Errors
///
/// Returns an error when the block fails the secret scan or the file cannot be created.
/// `--dry-run` writes nothing.
fn write_missing(path: &Path, dry_run: bool) -> Result<InitReport, Error> {
    let text = config_edit::openrouter_router_block();
    config_edit::refuse_named_secrets(&text, &[])?;
    if !dry_run {
        config_edit::create_file(path, text.as_bytes(), "")
            .map_err(|err| Error::InitReplace { source: err })?;
    }
    Ok(report(path, !dry_run, false, dry_run))
}

/// Append the block, or leave a file that already has a `[router]` table.
///
/// # Errors
///
/// Returns an error when the text cannot take the block, the secret scan refuses,
/// or the replace fails. A refusal writes nothing. The backup is taken only when
/// a write happens.
fn write_existing(path: &Path, text: &str, dry_run: bool) -> Result<InitReport, Error> {
    match config_edit::init_router_text(text).map_err(|err| text_error(path, err))? {
        RouterInit::AlreadyPresent => Ok(report(path, false, true, dry_run)),
        RouterInit::Append(edited) => {
            config_edit::refuse_named_secrets(&edited, &[])?;
            if !dry_run {
                config_edit::replace_file(path, Backup::Sibling, edited.as_bytes(), "")
                    .map_err(|err| Error::InitReplace { source: err })?;
            }
            Ok(report(path, !dry_run, false, dry_run))
        }
    }
}

/// Map a text refusal onto the command error. The file was not written.
fn text_error(path: &Path, err: RouterInitError) -> Error {
    match err {
        RouterInitError::Parse { detail, appended } => Error::InitConfigParse {
            path: path.to_path_buf(),
            detail,
            appended,
        },
        RouterInitError::NotTable => Error::InitRouterNotTable {
            path: path.to_path_buf(),
        },
        RouterInitError::ChangedTables { tables } => Error::InitChangedTables { tables },
    }
}

/// One finished `init`. `wrote` is false under `--dry-run` and when the file was already set.
fn report(path: &Path, wrote: bool, unchanged: bool, dry_run: bool) -> InitReport {
    InitReport {
        command: "init",
        format: "json",
        path: path.display().to_string(),
        wrote,
        unchanged,
        dry_run,
    }
}

/// Human text, the same line for quiet, or one JSON object.
///
/// # Errors
///
/// Returns [`Error::JsonEncode`] when the report cannot be encoded.
fn render(mode: OutputMode, report: &InitReport) -> Result<String, Error> {
    if mode == OutputMode::Json {
        return serde_json::to_string(report).map_err(|err| Error::JsonEncode {
            what: "the init result",
            source: err,
            retry: "Retry init; report this if it persists.",
        });
    }
    Ok(status_line(report))
}

/// The path and whether init wrote, left the file unchanged, or would write.
fn status_line(report: &InitReport) -> String {
    if report.dry_run {
        if report.unchanged {
            return format!("{} would write nothing", report.path);
        }
        return format!("{} would write", report.path);
    }
    if report.wrote {
        return format!("{} wrote", report.path);
    }
    format!("{} already has [router]; nothing was written", report.path)
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
                what: "the init result",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "init", err)
}

/// One JSON object for a finished `init`.
#[derive(serde::Serialize)]
struct InitReport {
    /// Always `init`.
    command: &'static str,
    /// Always `json`. Text and quiet do not print this struct.
    format: &'static str,
    /// Config path that was written, left unchanged, or would be written.
    path: String,
    /// Whether the file was created or replaced.
    wrote: bool,
    /// Whether a `[router]` table was already present.
    unchanged: bool,
    /// Whether `--dry-run` was set.
    dry_run: bool,
}
