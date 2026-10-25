//! `edit` subcommand.
//!
//! Edits fields inside one existing model block. Child tables, comments on
//! other lines, and unrelated tables stay. `--dry-run` does not replace the file.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::OutputMode;
use crate::config_edit::{self, Backup, FieldAction, FieldChange, RefuseReason};
use crate::error::Error;

/// Change fields inside one configured model block. This command writes unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router edit --config config.toml kimi-k3 --name \"Kimi K3\"\n  grok-router edit --dry-run --config config.toml kimi-k3 --reasoning-effort high\n  grok-router edit --config config.toml kimi-k3 --clear description"
)]
pub struct EditArgs {
    /// Slug of the model block to change.
    #[arg(value_name = "SLUG", verbatim_doc_comment)]
    slug: String,

    /// Provider id written on the `model` line. Not trimmed.
    #[arg(long, verbatim_doc_comment)]
    model: Option<String>,

    /// Base URL written on the `base_url` line. Not trimmed.
    #[arg(long, verbatim_doc_comment)]
    base_url: Option<String>,

    /// Display name written on the `name` line. Not trimmed.
    #[arg(long, verbatim_doc_comment)]
    name: Option<String>,

    /// Description written on the `description` line. Not trimmed.
    #[arg(long, verbatim_doc_comment)]
    description: Option<String>,

    /// Environment variable name written on the `env_key` line. Not the value.
    #[arg(long, verbatim_doc_comment)]
    env_key: Option<String>,

    /// Literal written on the `api_key` line. A value copied from the environment is refused.
    #[arg(long, verbatim_doc_comment)]
    api_key: Option<String>,

    /// `api_backend` value. Not trimmed.
    #[arg(long, verbatim_doc_comment)]
    api_backend: Option<String>,

    /// `context_window` when greater than zero.
    #[arg(long, verbatim_doc_comment)]
    context_window: Option<i64>,

    /// `max_completion_tokens` when greater than zero.
    #[arg(long, verbatim_doc_comment)]
    max_completion_tokens: Option<i64>,

    /// `reasoning_effort` value. Not trimmed.
    #[arg(long, verbatim_doc_comment)]
    reasoning_effort: Option<String>,

    /// `vision` as true or false.
    #[arg(long, value_name = "BOOL", verbatim_doc_comment)]
    vision: Option<String>,

    /// `stream_tool_calls` as true or false.
    #[arg(long, value_name = "BOOL", verbatim_doc_comment)]
    stream_tool_calls: Option<String>,

    /// `supports_reasoning_effort` as true or false.
    #[arg(long, value_name = "BOOL", verbatim_doc_comment)]
    supports_reasoning_effort: Option<String>,

    /// Remove this assignment from the model block. Repeat to clear more than one field.
    #[arg(long, value_name = "KEY", verbatim_doc_comment)]
    clear: Vec<String>,

    /// Print the slug and whether it would write, then do not change the file.
    #[arg(long, verbatim_doc_comment)]
    dry_run: bool,
}

/// Runs `edit`.
///
/// Returns `0` when the block was updated, or would be under `--dry-run`.
/// Returns `1` when the file was not changed.
#[must_use]
pub fn run(config: Option<&str>, args: &EditArgs, mode: OutputMode) -> i32 {
    match apply(config, args, mode) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// Read `--config`, edit the named block, and replace the file unless this is `--dry-run`.
///
/// # Errors
///
/// Returns an error when the path, fields, or edit is refused. Nothing is written.
fn apply(config: Option<&str>, args: &EditArgs, mode: OutputMode) -> Result<String, Error> {
    let path = configured_path(config)?;
    let slug = requested_slug(&args.slug)?;
    let changes = field_changes(args)?;
    let original = read_config_text(&path)?;
    let edited = config_edit::edit_model(&original, &slug, &changes)
        .map_err(|refused| edit_failure(refused.reason))?;
    let unchanged = edited == original;
    let dry_run = args.dry_run;
    let wrote = if unchanged || dry_run {
        false
    } else {
        config_edit::refuse_named_secrets(&edited, &[])?;
        config_edit::replace_file(&path, Backup::Sibling, edited.as_bytes(), "")
            .map_err(|err| Error::EditReplace { source: err })?;
        true
    };
    let report = EditReport {
        slug,
        wrote,
        dry_run,
        unchanged,
    };
    render(mode, &report, &path)
}

/// `--config` from the global flag. Never a home path.
///
/// # Errors
///
/// Returns an error when `--config` is omitted or empty.
fn configured_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err(Error::EditConfigEmpty),
        None => Err(Error::EditConfigOmitted),
    }
}

/// The required slug. An empty string is a usage failure.
///
/// # Errors
///
/// Returns an error when the slug is empty.
fn requested_slug(slug: &str) -> Result<String, Error> {
    if slug.is_empty() {
        Err(Error::EditSlugEmpty)
    } else {
        Ok(slug.to_owned())
    }
}

/// The fields this invocation sets or clears.
///
/// # Errors
///
/// Returns an error when no field was given, a boolean is not true or false,
/// or one key is both set and cleared.
fn field_changes(args: &EditArgs) -> Result<Vec<FieldChange>, Error> {
    let mut changes = Vec::new();
    push_text(&mut changes, "model", args.model.as_deref());
    push_text(&mut changes, "base_url", args.base_url.as_deref());
    push_text(&mut changes, "name", args.name.as_deref());
    push_text(&mut changes, "description", args.description.as_deref());
    push_text(&mut changes, "env_key", args.env_key.as_deref());
    push_text(&mut changes, "api_key", args.api_key.as_deref());
    push_text(&mut changes, "api_backend", args.api_backend.as_deref());
    push_text(
        &mut changes,
        "reasoning_effort",
        args.reasoning_effort.as_deref(),
    );
    push_int(&mut changes, "context_window", args.context_window);
    push_int(
        &mut changes,
        "max_completion_tokens",
        args.max_completion_tokens,
    );
    push_bool(&mut changes, "vision", args.vision.as_deref())?;
    push_bool(
        &mut changes,
        "stream_tool_calls",
        args.stream_tool_calls.as_deref(),
    )?;
    push_bool(
        &mut changes,
        "supports_reasoning_effort",
        args.supports_reasoning_effort.as_deref(),
    )?;
    for key in &args.clear {
        if !config_edit::is_editable_field(key) {
            return Err(Error::EditClearUnknown { key: key.clone() });
        }
        if changes.iter().any(|change| change.key == *key) {
            return Err(Error::EditClearAndSet { key: key.clone() });
        }
        changes.push(FieldChange {
            key: key.clone(),
            action: FieldAction::Clear,
        });
    }
    if changes.is_empty() {
        return Err(Error::EditNoFields);
    }
    Ok(changes)
}

/// Append a text field when the flag was passed.
fn push_text(changes: &mut Vec<FieldChange>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        changes.push(FieldChange {
            key: key.to_owned(),
            action: FieldAction::SetText(value.to_owned()),
        });
    }
}

/// Append an integer field when the flag was passed.
fn push_int(changes: &mut Vec<FieldChange>, key: &str, value: Option<i64>) {
    if let Some(value) = value {
        changes.push(FieldChange {
            key: key.to_owned(),
            action: FieldAction::SetInt(value),
        });
    }
}

/// Append a boolean field when the flag was passed.
///
/// # Errors
///
/// Returns an error when the text is not `true` or `false`.
fn push_bool(changes: &mut Vec<FieldChange>, key: &str, value: Option<&str>) -> Result<(), Error> {
    let Some(value) = value else {
        return Ok(());
    };
    let parsed = match value {
        "true" => true,
        "false" => false,
        _ => {
            return Err(Error::EditBool {
                flag: flag_name(key),
                raw: value.to_owned(),
            });
        }
    };
    changes.push(FieldChange {
        key: key.to_owned(),
        action: FieldAction::SetBool(parsed),
    });
    Ok(())
}

/// The command-line flag for a boolean field.
fn flag_name(key: &str) -> &'static str {
    match key {
        "stream_tool_calls" => "stream-tool-calls",
        "supports_reasoning_effort" => "supports-reasoning-effort",
        _ => "vision",
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
///
/// # Errors
///
/// Returns an error when the file cannot be read or is not UTF-8.
fn read_config_text(path: &Path) -> Result<String, Error> {
    let bytes = fs::read(path).map_err(|err| Error::EditConfigIo {
        path: path.to_path_buf(),
        source: err,
    })?;
    String::from_utf8(bytes).map_err(|_| Error::EditConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// What failed, why, and what to do next. The file was not written.
fn edit_failure(reason: RefuseReason) -> Error {
    Error::EditEdit { reason }
}

/// Human text, the slug, or one JSON object.
///
/// # Errors
///
/// Returns [`Error::JsonEncode`] when the report cannot be encoded.
fn render(mode: OutputMode, report: &EditReport, path: &Path) -> Result<String, Error> {
    if mode == OutputMode::Json {
        return serde_json::to_string(report).map_err(|err| Error::JsonEncode {
            what: "the edit result",
            source: err,
            retry: "Retry the edit; report this if it persists.",
        });
    }
    if mode == OutputMode::Quiet {
        return Ok(report.slug.clone());
    }
    if report.dry_run {
        if report.unchanged {
            return Ok(format!("{}\nwould write nothing", report.slug));
        }
        return Ok(format!("{}\nwould write {}", report.slug, path.display()));
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
                what: "the edit result",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "edit", err)
}

/// One JSON object for a finished `edit`.
#[derive(serde::Serialize)]
struct EditReport {
    /// Slug whose block was edited.
    slug: String,
    /// Whether [`config_edit::replace_file`] ran.
    wrote: bool,
    /// Whether `--dry-run` was set.
    dry_run: bool,
    /// Whether the spliced text matched the file.
    unchanged: bool,
}
