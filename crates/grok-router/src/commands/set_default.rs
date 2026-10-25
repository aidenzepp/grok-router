//! `set-default` subcommand.
//!
//! Sets `[models].default` for the next session. It does not switch the running
//! session. The text edit is [`config_edit::set_default`]; the file edit is
//! [`config_edit::replace_file`]. `--dry-run` does not replace the file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::OutputMode;
use crate::config_edit::{self, Backup, RefuseReason};
use crate::error::Error;

/// Set [models].default for the next session. This does not switch the running session. This command writes the saved default unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router set-default --config config.toml grok-4.6"
)]
pub struct SetDefaultArgs {
    /// Built-in or configured slug to save as the default.
    #[arg(value_name = "SLUG", verbatim_doc_comment)]
    slug: String,

    /// Print the slug that would be saved, then do not change the file.
    #[arg(long, verbatim_doc_comment)]
    dry_run: bool,
}

/// Runs `set-default`.
///
/// Returns `0` when the saved default is the requested slug, or would be under `--dry-run`.
/// Returns `1` when the default was not changed.
#[must_use]
pub fn run(config: Option<&str>, args: &SetDefaultArgs, mode: OutputMode) -> i32 {
    match apply(config, args, mode) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// Read `--config`, set the default in that text, and replace the file unless this is `--dry-run`.
///
/// # Errors
///
/// Returns an error when the path, slug, or edit is refused. Nothing is written.
fn apply(config: Option<&str>, args: &SetDefaultArgs, mode: OutputMode) -> Result<String, Error> {
    let path = configured_path(config)?;
    let slug = requested_slug(&args.slug)?;
    let original = read_config_text(&path)?;
    let edited = config_edit::set_default(&original, &slug)
        .map_err(|refused| edit_failure(refused.reason))?;
    let unchanged = edited == original;
    let dry_run = args.dry_run;
    let wrote = if unchanged || dry_run {
        false
    } else {
        config_edit::refuse_named_secrets(&edited, &[])?;
        config_edit::replace_file(&path, Backup::Sibling, edited.as_bytes(), "")
            .map_err(|err| Error::SetDefaultReplace { source: err })?;
        true
    };
    let report = SetDefaultReport {
        slug,
        wrote,
        dry_run,
        unchanged,
    };
    render(mode, &report)
}

/// `--config` from the global flag. Never a home path.
///
/// # Errors
///
/// Returns an error when `--config` is omitted or empty.
fn configured_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err(Error::SetDefaultConfigEmpty),
        None => Err(Error::SetDefaultConfigOmitted),
    }
}

/// The required slug. An empty string is a usage failure.
///
/// # Errors
///
/// Returns an error when the slug is empty.
fn requested_slug(slug: &str) -> Result<String, Error> {
    if slug.is_empty() {
        Err(Error::SetDefaultSlugEmpty)
    } else {
        Ok(slug.to_owned())
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
///
/// # Errors
///
/// Returns an error when the file cannot be read or is not UTF-8.
fn read_config_text(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|err| Error::SetDefaultConfigIo {
        path: path.to_path_buf(),
        source: err,
    })?;
    String::from_utf8(bytes).map_err(|_| Error::SetDefaultConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// What failed, why, and what to do next. The file was not written.
fn edit_failure(reason: RefuseReason) -> Error {
    Error::SetDefaultEdit { reason }
}

/// Human text, the slug, or one JSON object.
///
/// # Errors
///
/// Returns [`Error::JsonEncode`] when the report cannot be encoded.
fn render(mode: OutputMode, report: &SetDefaultReport) -> Result<String, Error> {
    if mode == OutputMode::Json {
        return serde_json::to_string(report).map_err(|err| Error::JsonEncode {
            what: "the set-default result",
            source: err,
            retry: "Retry the command; report this if it persists.",
        });
    }
    if mode == OutputMode::Quiet {
        return Ok(report.slug.clone());
    }
    if report.dry_run {
        if report.unchanged {
            return Ok(format!("{}\nwould save nothing", report.slug));
        }
        return Ok(format!("{}\nwould save {}", report.slug, report.slug));
    }
    Ok(report.slug.clone())
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
                what: "the set-default result",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "set-default", err)
}

/// One JSON object for a finished `set-default`.
#[derive(serde::Serialize)]
struct SetDefaultReport {
    /// Slug that is, or would be, the saved default.
    slug: String,
    /// Whether [`config_edit::replace_file`] ran.
    wrote: bool,
    /// Whether `--dry-run` was set.
    dry_run: bool,
    /// Whether the text was already that default.
    unchanged: bool,
}
