//! `remove` subcommand.
//!
//! Remove deletes one model block from the file named by `--config`.
//! `--yes` is required to delete. `--dry-run` prints the slug and does not write.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::config_edit::{self, Backup, RefuseReason};
use crate::error::Error;
use crate::OutputMode;

/// Delete one model block from the config. This command deletes a model block.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router remove --yes --config config.toml or-glm-5-3"
)]
pub struct RemoveArgs {
    /// Slug of the model block to delete.
    #[arg(value_name = "SLUG", verbatim_doc_comment)]
    slug: String,

    /// Delete the model block. Required unless --dry-run is set. --force is the same flag.
    #[arg(long, visible_alias = "force", verbatim_doc_comment)]
    yes: bool,

    /// Print the slug that would be deleted and do not change the file.
    #[arg(long, verbatim_doc_comment)]
    dry_run: bool,
}

/// Runs `remove`.
///
/// Returns `0` when the block was deleted, or when `--dry-run` would delete it.
/// Returns `1` when the file was left unchanged.
#[must_use]
pub fn run(config: Option<&str>, args: &RemoveArgs, mode: OutputMode) -> i32 {
    match run_remove(config, args, mode) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// Delete one slug, or preview that deletion. Does not prompt.
fn run_remove(config: Option<&str>, args: &RemoveArgs, mode: OutputMode) -> Result<String, Error> {
    let slug = required_slug(&args.slug)?;
    let config_path = configured_path(config)?;
    let original = read_config_text(&config_path)?;
    let edited =
        config_edit::remove_model(&original, &slug).map_err(|refused| edit_failure(refused.reason))?;
    let dry_run = args.dry_run;
    let confirmed = args.yes;
    if !dry_run && !confirmed {
        return Err(Error::RemoveNeedsYes { slug });
    }
    let removed = if dry_run {
        false
    } else {
        config_edit::refuse_named_secrets(&edited, &[])?;
        config_edit::replace_file(&config_path, Backup::Sibling, edited.as_bytes(), "")
            .map_err(|err| Error::RemoveReplace { source: err })?;
        true
    };
    let report = RemoveReport {
        slug,
        removed,
        dry_run,
    };
    render(mode, &report)
}

/// The positional slug. An empty slug is a usage failure and deletes nothing.
fn required_slug(slug: &str) -> Result<String, Error> {
    if slug.is_empty() {
        Err(Error::RemoveSlugEmpty)
    } else {
        Ok(slug.to_owned())
    }
}

/// `--config` from the global flag. Never a home path.
fn configured_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err(Error::RemoveConfigEmpty),
        None => Err(Error::RemoveConfigOmitted),
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
fn read_config_text(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|err| Error::RemoveConfigIo {
        path: path.to_path_buf(),
        source: err,
    })?;
    String::from_utf8(bytes).map_err(|_| Error::RemoveConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// What failed, why, and what to do next. The file was not written.
fn edit_failure(reason: RefuseReason) -> Error {
    Error::RemoveEdit { reason }
}

/// Human text, the slug, or one JSON object.
fn render(mode: OutputMode, report: &RemoveReport) -> Result<String, Error> {
    if mode == OutputMode::Json {
        return serde_json::to_string(report).map_err(|err| Error::JsonEncode {
            what: "the remove result",
            source: err,
            retry: "Retry the remove; report this if it persists.",
        });
    }
    if mode == OutputMode::Quiet {
        return Ok(report.slug.clone());
    }
    if report.dry_run {
        return Ok(format!("would delete {}", report.slug));
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
                what: "the remove result",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "remove", err)
}

/// One JSON object for a finished remove.
#[derive(serde::Serialize)]
struct RemoveReport {
    /// Slug the command was asked to delete.
    slug: String,
    /// Whether the config file was replaced.
    removed: bool,
    /// Whether this run was `--dry-run` and therefore did not write.
    dry_run: bool,
}
