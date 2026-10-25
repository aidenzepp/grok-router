//! `set-default` subcommand.
//!
//! Sets `[models].default` for the next session. It does not switch the running
//! session. The text edit is [`config_edit::set_default`]; the file edit is
//! [`config_edit::replace_file`]. `--dry-run` does not replace the file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::config_edit::{self, Backup, RefuseReason};
use crate::OutputMode;

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
        Err(message) => fail(mode, &message),
    }
}

/// Read `--config`, set the default in that text, and replace the file unless this is `--dry-run`.
fn apply(config: Option<&str>, args: &SetDefaultArgs, mode: OutputMode) -> Result<String, String> {
    let path = configured_path(config)?;
    let slug = requested_slug(&args.slug)?;
    let original = read_config_text(&path)?;
    let edited = config_edit::set_default(&original, &slug)
        .map_err(|refused| edit_failure(&refused.reason))?;
    let unchanged = edited == original;
    let dry_run = args.dry_run;
    let wrote = if unchanged || dry_run {
        false
    } else {
        config_edit::refuse_named_secrets(&edited, &[])?;
        config_edit::replace_file(&path, Backup::Sibling, edited.as_bytes(), "").map_err(
            |err| {
                format!(
                    "{err}. The saved default was not changed. Retry with a writable --config, or pass --dry-run to preview the slug."
                )
            },
        )?;
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
fn configured_path(config: Option<&str>) -> Result<PathBuf, String> {
    match config {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err(
            "--config is empty, so the saved default was not changed. Pass --config with the file to edit."
                .to_owned(),
        ),
        None => Err(
            "no --config path was given, so the saved default was not changed. Pass --config with the file to edit."
                .to_owned(),
        ),
    }
}

/// The required slug. An empty string is a usage failure.
fn requested_slug(slug: &str) -> Result<String, String> {
    if slug.is_empty() {
        Err(
            "the slug is empty, so the saved default was not changed. Pass a built-in or configured slug."
                .to_owned(),
        )
    } else {
        Ok(slug.to_owned())
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
fn read_config_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "could not read config {}. {err}. The saved default was not changed. Pass --config pointing at an existing file.",
            path.display()
        )
    })?;
    String::from_utf8(bytes).map_err(|_| {
        format!(
            "config {} is not UTF-8. The saved default was not changed. Save it as UTF-8 and pass that --config again.",
            path.display()
        )
    })
}

/// What failed, why, and what to do next. The file was not written.
fn edit_failure(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The file was left unchanged. {}",
        next_step(reason)
    )
}

/// The next action for a refused edit.
fn next_step(reason: &RefuseReason) -> &'static str {
    match reason {
        RefuseReason::UnknownSlug { .. } => {
            "Pass a built-in slug (grok-4.7, grok-4.7-build-fast, grok-4.6, grok-4.5) or a slug already in the config."
        }
        RefuseReason::InvalidSlug { .. } => {
            "Pass a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens."
        }
        RefuseReason::MultipleModelsTables => "Leave one [models] table in the config and retry.",
        RefuseReason::DuplicateSlugHeader { .. } => "Leave one header for that slug and retry.",
        RefuseReason::Parse { .. } => "Fix the config so it parses as TOML and retry.",
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::NotConfigured { .. }
        | RefuseReason::BuiltIn { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
        }
    }
}

/// Human text, the slug, or one JSON object.
fn render(mode: OutputMode, report: &SetDefaultReport) -> Result<String, String> {
    if mode == OutputMode::Json {
        return serde_json::to_string(report).map_err(|err| {
            format!(
                "could not encode the set-default result as JSON: {err}. Retry the command; report this if it persists."
            )
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
            &format!(
                "could not write the set-default result: {err}. Retry when stdout accepts output."
            ),
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, message: &str) -> i32 {
    crate::fail_message(mode, message)
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
