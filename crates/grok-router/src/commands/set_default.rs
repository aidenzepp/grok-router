//! `set-default` subcommand.
//!
//! Sets `[models].default` for the next session. It does not switch the running
//! session. The text edit is [`config_edit::set_default`]; the file edit is
//! [`config_edit::replace_file`]. `--dry-run` does not replace the file.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config_edit::{self, Backup, RefuseReason};

/// Adds the `set-default` subcommand and its flags.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(
        Command::new("set-default")
            .about(
                "Set [models].default for the next session. This does not switch the running session. This command writes the saved default unless --dry-run is set.",
            )
            .after_help(
                "Examples:\n  grok-router set-default --config config.toml grok-4.6",
            )
            .arg(
                Arg::new("slug")
                    .value_name("SLUG")
                    .required(true)
                    .help("Built-in or configured slug to save as the default."),
            )
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue)
                    .help("Print the slug that would be saved, then do not change the file."),
            )
            .arg(
                Arg::new("json")
                    .long("json")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("quiet")
                    .help("Print one JSON object and nothing else."),
            )
            .arg(
                Arg::new("quiet")
                    .long("quiet")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("json")
                    .help("Print the slug only."),
            ),
    )
}

/// Runs `set-default` from the root matches.
///
/// Returns `0` when the saved default is the requested slug, or would be under `--dry-run`.
/// Returns `1` when the default was not changed.
#[must_use]
pub fn run(matches: &ArgMatches) -> i32 {
    let Some(sub) = matches.subcommand_matches("set-default") else {
        return fail(
            "set-default was not selected. Run `grok-router set-default --help` for the flags and an example.",
        );
    };
    match apply(matches, sub) {
        Ok(body) => write_stdout(&body),
        Err(message) => fail(&message),
    }
}

/// Read `--config`, set the default in that text, and replace the file unless this is `--dry-run`.
fn apply(root: &ArgMatches, matches: &ArgMatches) -> Result<String, String> {
    let path = configured_path(root, matches)?;
    let slug = requested_slug(matches)?;
    let original = read_config_text(&path)?;
    let edited =
        config_edit::set_default(&original, &slug).map_err(|refused| edit_failure(&refused.reason))?;
    let unchanged = edited == original;
    let dry_run = matches.get_flag("dry-run");
    let wrote = if unchanged || dry_run {
        false
    } else {
        let secret = api_key_or_empty()?;
        config_edit::replace_file(&path, Backup::Sibling, edited.as_bytes(), &secret).map_err(
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
    render(matches, &report)
}

/// `--config` from the subcommand or the global flag. Never a home path.
fn configured_path(root: &ArgMatches, sub: &ArgMatches) -> Result<PathBuf, String> {
    let raw = sub
        .get_one::<String>("config")
        .or_else(|| root.get_one::<String>("config"));
    match raw {
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
fn requested_slug(matches: &ArgMatches) -> Result<String, String> {
    match matches.get_one::<String>("slug") {
        Some(slug) if !slug.is_empty() => Ok(slug.clone()),
        Some(_) => Err(
            "the slug is empty, so the saved default was not changed. Pass a built-in or configured slug."
                .to_owned(),
        ),
        None => Err(
            "no slug was given, so the saved default was not changed. Pass a built-in or configured slug. Run `grok-router set-default --help` for an example."
                .to_owned(),
        ),
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

/// `OPENROUTER_API_KEY` when it is set, otherwise an empty string that [`config_edit::replace_file`] does not search for.
fn api_key_or_empty() -> Result<String, String> {
    match env::var("OPENROUTER_API_KEY") {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(String::new()),
        Err(env::VarError::NotUnicode(_)) => Err(
            "OPENROUTER_API_KEY is not valid Unicode, so the saved default was not changed. Unset it or export a Unicode value."
                .to_owned(),
        ),
    }
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
fn render(matches: &ArgMatches, report: &SetDefaultReport) -> Result<String, String> {
    if matches.get_flag("json") {
        return serde_json::to_string(report).map_err(|err| {
            format!(
                "could not encode the set-default result as JSON: {err}. Retry the command; report this if it persists."
            )
        });
    }
    if matches.get_flag("quiet") {
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
            "could not write the set-default result: {err}. Retry when stdout accepts output."
        )),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
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
