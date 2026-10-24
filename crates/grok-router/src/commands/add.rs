//! `add` subcommand.
//!
//! Add writes one OpenRouter model into the file named by `--config`.
//! `--catalog-file` reads a local catalog and does not use the network.
//! Without that flag, a missing API key fails before any config write.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::catalog::{self, CachedOrFetch, CatalogRow};
use crate::config_edit::{self, Backup, RefuseReason};

/// Adds the `add` subcommand and its flags.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(
        Command::new("add")
            .about("Add one OpenRouter model to the config. This command writes unless --dry-run is set.")
            .after_help(
                "Examples:\n  grok-router add --catalog-file models.json --config config.toml z-ai/glm-5.3",
            )
            .arg(
                Arg::new("query")
                    .value_name("QUERY")
                    .action(ArgAction::Append)
                    .help("Provider id or terms. Repeat to require every term. Optional when the catalog has one row."),
            )
            .arg(
                Arg::new("as")
                    .long("as")
                    .value_name("SLUG")
                    .help("Slug to write instead of the derived or- slug."),
            )
            .arg(
                Arg::new("default")
                    .long("default")
                    .action(ArgAction::SetTrue)
                    .help("Set this slug as [models].default in the same write."),
            )
            .arg(
                Arg::new("catalog-file")
                    .long("catalog-file")
                    .value_name("PATH")
                    .value_parser(clap::value_parser!(PathBuf))
                    .help("Read this catalog JSON instead of the network or the cache."),
            )
            .arg(
                Arg::new("cache-dir")
                    .long("cache-dir")
                    .value_name("DIR")
                    .value_parser(clap::value_parser!(PathBuf))
                    .help("Directory for the models.json cache. Used only when --catalog-file is absent."),
            )
            .arg(
                Arg::new("refresh")
                    .long("refresh")
                    .action(ArgAction::SetTrue)
                    .help("Download a new catalog even when the cache is younger than one hour."),
            )
            .arg(
                Arg::new("dry-run")
                    .long("dry-run")
                    .action(ArgAction::SetTrue)
                    .help("Print the slug and that it would write, then do not change the file."),
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

/// Runs `add` from the root matches.
///
/// Returns `0` when the model is present in the config (or would be, under `--dry-run`).
/// Returns `1` when the add did not apply.
#[must_use]
pub fn run(matches: &ArgMatches) -> i32 {
    let Some(sub) = matches.subcommand_matches("add") else {
        return fail(
            "add was not selected. Run `grok-router add --help` for the flags and an example.",
        );
    };
    match run_add(matches, sub) {
        Ok(body) => write_stdout(&body),
        Err(message) => fail(&message),
    }
}

/// Resolve one catalog row, splice it, and write once unless this is a no-op or `--dry-run`.
fn run_add(root: &ArgMatches, matches: &ArgMatches) -> Result<String, String> {
    let config_path = configured_path(root, matches)?;
    let terms = query_terms(matches)?;
    let slug_override = matches.get_one::<String>("as").map(String::as_str);
    let rows = load_rows(matches)?;
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let row = catalog::resolve(&rows, &term_refs).map_err(|err| {
        format!("{err} Nothing was written.")
    })?;
    let slug = planned_slug(row.id(), slug_override)?;
    let original = read_config_text(&config_path)?;
    let edited = edited_config(&original, row, slug_override, &slug, matches.get_flag("default"))?;
    let unchanged = edited == original;
    let dry_run = matches.get_flag("dry-run");
    let wrote = if unchanged || dry_run {
        false
    } else {
        let secret = live_api_key()?;
        config_edit::replace_file(&config_path, Backup::Sibling, edited.as_bytes(), &secret)
            .map_err(|err| {
                format!(
                    "{err}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the edit."
                )
            })?;
        true
    };
    let report = AddReport {
        slug,
        provider_id: row.id().to_owned(),
        wrote,
        dry_run,
        unchanged,
        default: matches.get_flag("default"),
    };
    render(matches, &report, &config_path)
}

/// `--config` from the subcommand or the global flag. Never a home path.
fn configured_path(root: &ArgMatches, sub: &ArgMatches) -> Result<PathBuf, String> {
    let raw = sub
        .get_one::<String>("config")
        .or_else(|| root.get_one::<String>("config"));
    match raw {
        Some(path) if !path.is_empty() => Ok(PathBuf::from(path)),
        Some(_) => Err(
            "--config is empty, so nothing was written. Pass --config with the file to edit."
                .to_owned(),
        ),
        None => Err(
            "no --config path was given, so nothing was written. Pass --config with the file to edit."
                .to_owned(),
        ),
    }
}

/// Positional query terms. An empty term is a usage failure.
fn query_terms(matches: &ArgMatches) -> Result<Vec<String>, String> {
    let Some(values) = matches.get_many::<String>("query") else {
        return Ok(Vec::new());
    };
    let mut terms = Vec::new();
    for term in values {
        if term.is_empty() {
            return Err(
                "an empty query was given. Nothing was written. Pass a provider id or terms."
                    .to_owned(),
            );
        }
        terms.push(term.clone());
    }
    Ok(terms)
}

/// Slug `--as` requested, or the slug derived from the provider id.
fn planned_slug(provider_id: &str, slug_override: Option<&str>) -> Result<String, String> {
    match slug_override {
        Some(slug) => Ok(slug.to_owned()),
        None => config_edit::derive_slug(provider_id).map_err(|reason| edit_failure(&reason)),
    }
}

/// Load rows from `--catalog-file`, or from the cache and the network.
fn load_rows(matches: &ArgMatches) -> Result<Vec<CatalogRow>, String> {
    if let Some(path) = matches.get_one::<PathBuf>("catalog-file") {
        let bytes = fs::read(path).map_err(|err| {
            format!(
                "could not read catalog file {}. {err}. Nothing was written. Pass a readable --catalog-file, or omit it to use the cache.",
                path.display()
            )
        })?;
        return catalog::parse_catalog(&bytes)
            .map_err(|err| format!("{err} Nothing was written."));
    }

    let cache_dir = match matches.get_one::<PathBuf>("cache-dir") {
        Some(path) => path.clone(),
        None => default_cache_dir()?,
    };
    let api_key = live_api_key()?;
    let now = SystemTime::now();
    let base_url = config_edit::OPENROUTER_BASE_URL;
    match catalog::prepare_cached_load(
        &cache_dir,
        now,
        matches.get_flag("refresh"),
        &api_key,
        base_url,
    )
    .map_err(|err| format!("{err} Nothing was written."))?
    {
        CachedOrFetch::Cached(rows) => Ok(rows),
        CachedOrFetch::Fetch(request) => {
            let bytes = catalog::fetch_catalog_bytes(&request)
                .map_err(|err| format!("{err} Nothing was written."))?;
            let stored = catalog::write_cache(&cache_dir, base_url, &bytes)
                .map_err(|err| format!("{err} Nothing was written."))?;
            if stored != catalog::cache_file_for(&cache_dir, &catalog::models_url(base_url)) {
                return Err(format!(
                    "the catalog cache was written to {}, outside {}. The config was not written. Pass --cache-dir.",
                    stored.display(),
                    cache_dir.display()
                ));
            }
            catalog::parse_catalog(&bytes).map_err(|err| format!("{err} The config was not written."))
        }
    }
}

/// Default cache directory `~/.cache/grok-router`. This is not the config file.
fn default_cache_dir() -> Result<PathBuf, String> {
    let Some(home) = env::var_os("HOME") else {
        return Err(
            "HOME is not set, so the catalog cache has no default directory. Nothing was written. Pass --cache-dir or --catalog-file."
                .to_owned(),
        );
    };
    if home.is_empty() {
        return Err(
            "HOME is empty, so the catalog cache has no default directory. Nothing was written. Pass --cache-dir or --catalog-file."
                .to_owned(),
        );
    }
    Ok(PathBuf::from(home).join(".cache").join("grok-router"))
}

/// The API key, or an empty string when the variable is unset.
fn live_api_key() -> Result<String, String> {
    match env::var("OPENROUTER_API_KEY") {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(String::new()),
        Err(env::VarError::NotUnicode(_)) => Err(
            "OPENROUTER_API_KEY is not valid Unicode, so the config was not written. Export a Unicode key or pass --catalog-file."
                .to_owned(),
        ),
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
fn read_config_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "could not read config {}. {err}. Nothing was written. Pass --config pointing at an existing file.",
            path.display()
        )
    })?;
    String::from_utf8(bytes).map_err(|_| {
        format!(
            "config {} is not UTF-8. Nothing was written. Save it as UTF-8 and pass that --config again.",
            path.display()
        )
    })
}

/// Insert the row, then set the default on that text when `--default` is set.
fn edited_config(
    original: &str,
    row: &CatalogRow,
    slug_override: Option<&str>,
    slug: &str,
    set_default: bool,
) -> Result<String, String> {
    let inserted = config_edit::insert_model(original, row.as_catalog_model(), slug_override)
        .map_err(|refused| edit_failure(&refused.reason))?;
    if !set_default {
        return Ok(inserted);
    }
    config_edit::set_default(&inserted, slug).map_err(|refused| edit_failure(&refused.reason))
}

/// What failed, why, and what to do next. The file was not written.
fn edit_failure(reason: &RefuseReason) -> String {
    format!(
        "{reason}. The file was left unchanged. {}",
        next_step(reason)
    )
}

/// The next action for a refused edit.
fn next_step(reason: &RefuseReason) -> String {
    match reason {
        RefuseReason::EmptyProviderId => "Choose a catalog row that has an id.".to_owned(),
        RefuseReason::EmptySlugLeaf { .. } => "Pass --as with a slug.".to_owned(),
        RefuseReason::InvalidSlug { .. } => {
            "Pass --as with a slug that starts with a letter or digit and contains only letters, digits, dots, underscores, and hyphens.".to_owned()
        }
        RefuseReason::NonPositiveContext { .. } => {
            "Choose a catalog row whose context_length is greater than zero.".to_owned()
        }
        RefuseReason::DuplicateProvider { existing_slug, .. } => format!(
            "Keep {existing_slug}, or remove it before adding another slug for that provider."
        ),
        RefuseReason::SlugInUse { .. } => {
            "Pass --as with a slug that is not already used.".to_owned()
        }
        RefuseReason::UnknownSlug { .. } => {
            "Add the model before passing --default, or choose a built-in or configured slug."
                .to_owned()
        }
        RefuseReason::MultipleModelsTables => {
            "Leave one [models] table in the config and retry.".to_owned()
        }
        RefuseReason::DuplicateSlugHeader { .. } => {
            "Leave one header for that slug and retry.".to_owned()
        }
        RefuseReason::Parse { .. } => "Fix the config so it parses as TOML and retry.".to_owned(),
        RefuseReason::NotConfigured { .. }
        | RefuseReason::BuiltIn { .. }
        | RefuseReason::IsDefault { .. }
        | RefuseReason::DroppedTables { .. }
        | RefuseReason::ChangedProvider { .. }
        | RefuseReason::AddMissing { .. }
        | RefuseReason::RemoveFailed { .. }
        | RefuseReason::DefaultMismatch { .. }
        | RefuseReason::InvalidSetting { .. } => {
            "Retry with --dry-run after fixing the config. This command did not replace the file."
                .to_owned()
        }
    }
}

/// Human text, the slug, or one JSON object.
fn render(matches: &ArgMatches, report: &AddReport, path: &Path) -> Result<String, String> {
    if matches.get_flag("json") {
        return serde_json::to_string(report).map_err(|err| {
            format!(
                "could not encode the add result as JSON: {err}. Retry the add; report this if it persists."
            )
        });
    }
    if matches.get_flag("quiet") {
        return Ok(report.slug.clone());
    }
    if report.dry_run {
        if report.unchanged {
            return Ok(format!("{}\nwould write nothing", report.slug));
        }
        return Ok(format!(
            "{}\nwould write {}",
            report.slug,
            path.display()
        ));
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
            "could not write the add result: {err}. Retry when stdout accepts output."
        )),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
}

/// One JSON object for a finished add.
#[derive(serde::Serialize)]
struct AddReport {
    slug: String,
    provider_id: String,
    wrote: bool,
    dry_run: bool,
    unchanged: bool,
    default: bool,
}
