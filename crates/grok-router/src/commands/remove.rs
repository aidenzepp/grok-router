//! `remove` subcommand.
//!
//! Remove deletes one model block from the file named by `--config`.
//! `--yes` is required to delete. `--dry-run` prints the slug and does not write.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::config_edit::{self, Backup, RefuseReason};

/// Adds the `remove` subcommand and its flags.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(
        Command::new("remove")
            .about("Delete one model block from the config. This command deletes a model block.")
            .after_help(
                "Examples:\n  grok-router remove --yes --config config.toml or-glm-5-3",
            )
            .arg(
                Arg::new("slug")
                    .value_name("SLUG")
                    .required(true)
                    .help("Slug of the model block to delete."),
            )
            .arg(
                Arg::new("yes")
                    .long("yes")
                    .visible_alias("force")
                    .action(ArgAction::SetTrue)
                    .help("Delete the model block. Required unless --dry-run is set. --force is the same flag."),
            )
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue)
                    .help("Print the slug that would be deleted and do not change the file."),
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

/// Runs `remove` from the root matches.
///
/// Returns `0` when the block was deleted, or when `--dry-run` would delete it.
/// Returns `1` when the file was left unchanged.
#[must_use]
pub fn run(matches: &ArgMatches) -> i32 {
    let Some(sub) = matches.subcommand_matches("remove") else {
        return fail(
            "remove was not selected. Run `grok-router remove --help` for the flags and an example.",
        );
    };
    match run_remove(matches, sub) {
        Ok(body) => write_stdout(&body),
        Err(message) => fail(&message),
    }
}

/// Delete one slug, or preview that deletion. Does not prompt.
fn run_remove(root: &ArgMatches, matches: &ArgMatches) -> Result<String, String> {
    let slug = required_slug(matches)?;
    let config_path = configured_path(root, matches)?;
    let original = read_config_text(&config_path)?;
    let edited = config_edit::remove_model(&original, &slug)
        .map_err(|refused| edit_failure(&refused.reason))?;
    let dry_run = matches.get_flag("dry-run");
    let confirmed = matches.get_flag("yes");
    if !dry_run && !confirmed {
        return Err(format!(
            "{slug} was not deleted. Nothing was written. To delete that model block, pass --yes. To preview, pass --dry-run."
        ));
    }
    let removed = if dry_run {
        false
    } else {
        let secret = live_api_key()?;
        config_edit::replace_file(&config_path, Backup::Sibling, edited.as_bytes(), &secret)
            .map_err(|err| {
                format!(
                    "{err}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the deletion."
                )
            })?;
        true
    };
    let report = RemoveReport {
        slug,
        removed,
        dry_run,
    };
    render(matches, &report)
}

/// The positional slug. An empty slug is a usage failure and deletes nothing.
fn required_slug(matches: &ArgMatches) -> Result<String, String> {
    match matches.get_one::<String>("slug").map(String::as_str) {
        Some(slug) if !slug.is_empty() => Ok(slug.to_owned()),
        Some(_) => Err(
            "an empty slug was given. Nothing was deleted. Pass the slug of a model block."
                .to_owned(),
        ),
        None => Err(
            "no slug was given. Nothing was deleted. Pass the slug of the model block to delete."
                .to_owned(),
        ),
    }
}

/// `--config` from the subcommand or the global flag. Never a home path.
fn configured_path(root: &ArgMatches, sub: &ArgMatches) -> Result<PathBuf, String> {
    let raw = sub
        .get_one::<String>("config")
        .or_else(|| root.get_one::<String>("config"));
    match raw {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err(
            "--config is empty, so nothing was deleted. Pass --config with the file to edit."
                .to_owned(),
        ),
        None => Err(
            "no --config path was given, so nothing was deleted. Pass --config with the file to edit."
                .to_owned(),
        ),
    }
}

/// The API key, or an empty string when the variable is unset.
///
/// An empty string is not searched. A non-empty key that occurs in the replacement refuses the write.
fn live_api_key() -> Result<String, String> {
    match env::var("OPENROUTER_API_KEY") {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(String::new()),
        Err(env::VarError::NotUnicode(_)) => Err(
            "OPENROUTER_API_KEY is not valid Unicode, so the config was not written. Export a Unicode key or unset it."
                .to_owned(),
        ),
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
fn read_config_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "could not read config {}. {err}. Nothing was deleted. Pass --config pointing at an existing file.",
            path.display()
        )
    })?;
    String::from_utf8(bytes).map_err(|_| {
        format!(
            "config {} is not UTF-8. Nothing was deleted. Save it as UTF-8 and pass that --config again.",
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

/// The next action for a refused removal.
fn next_step(reason: &RefuseReason) -> String {
    match reason {
        RefuseReason::IsDefault { .. } => {
            "Set another default, then remove this slug.".to_owned()
        }
        RefuseReason::BuiltIn { .. } => {
            "Pass a configured slug. Built-ins have no config block to delete.".to_owned()
        }
        RefuseReason::NotConfigured { .. } => {
            "Pass a slug that appears as a [model.<slug>] block in this file.".to_owned()
        }
        RefuseReason::InvalidSlug { .. } => {
            "Pass a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens.".to_owned()
        }
        RefuseReason::MultipleModelsTables => {
            "Leave one [models] table in the config and retry.".to_owned()
        }
        RefuseReason::DuplicateSlugHeader { .. } => {
            "Leave one header for that slug and retry.".to_owned()
        }
        RefuseReason::Parse { .. } => "Fix the config so it parses as TOML and retry.".to_owned(),
        RefuseReason::EmptyProviderId
        | RefuseReason::EmptySlugLeaf { .. }
        | RefuseReason::NonPositiveContext { .. }
        | RefuseReason::DuplicateProvider { .. }
        | RefuseReason::SlugInUse { .. }
        | RefuseReason::UnknownSlug { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
                .to_owned()
        }
    }
}

/// Human text, the slug, or one JSON object.
fn render(matches: &ArgMatches, report: &RemoveReport) -> Result<String, String> {
    if matches.get_flag("json") {
        return serde_json::to_string(report).map_err(|err| {
            format!(
                "could not encode the remove result as JSON: {err}. Retry the remove; report this if it persists."
            )
        });
    }
    if matches.get_flag("quiet") {
        return Ok(report.slug.clone());
    }
    if report.dry_run {
        return Ok(format!("would delete {}", report.slug));
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
            "could not write the remove result: {err}. Retry when stdout accepts output."
        )),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
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
