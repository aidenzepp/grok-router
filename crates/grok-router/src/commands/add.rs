//! `add` subcommand.
//!
//! Add writes one OpenRouter model into the file named by `--config`.
//! That file's OpenRouter base is resolved before any catalog load, and the
//! new block is spliced with the same base. `--catalog-file` reads a local
//! catalog and does not use the network. Without that flag, a missing API key
//! fails before any config write. A missing `--config` fails before any home
//! path is opened.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clap::{ArgAction, Args};

use crate::catalog::{self, CachedOrFetch, CatalogRow};
use crate::config_edit::{self, Backup, RefuseReason};

/// Add one OpenRouter model to the config. This command writes unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router add --catalog-file models.json --config config.toml z-ai/glm-5.3"
)]
pub struct AddArgs {
    /// Provider id or terms. Repeat to require every term. Optional when the catalog has one row.
    #[arg(value_name = "QUERY", action = ArgAction::Append, verbatim_doc_comment)]
    query: Vec<String>,

    /// Slug to write instead of the derived or- slug.
    #[arg(long = "as", value_name = "SLUG", verbatim_doc_comment)]
    as_slug: Option<String>,

    /// Set this slug as [models].default in the same write.
    #[arg(long = "default", verbatim_doc_comment)]
    set_default: bool,

    /// Read this catalog JSON instead of the network or the cache.
    #[arg(long, value_name = "PATH", verbatim_doc_comment)]
    catalog_file: Option<PathBuf>,

    /// Directory for the models.json cache. Used only when --catalog-file is absent.
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    cache_dir: Option<PathBuf>,

    /// Download a new catalog even when the cache is younger than one hour.
    #[arg(long, verbatim_doc_comment)]
    refresh: bool,

    /// Print the slug and that it would write, then do not change the file.
    #[arg(long, verbatim_doc_comment)]
    dry_run: bool,

    /// Print one JSON object and nothing else.
    #[arg(long, conflicts_with = "quiet", verbatim_doc_comment)]
    json: bool,

    /// Print the slug only.
    #[arg(long, conflicts_with = "json", verbatim_doc_comment)]
    quiet: bool,
}

/// Runs `add`.
///
/// Returns `0` when the model is present in the config (or would be, under `--dry-run`).
/// Returns `1` when the add did not apply.
#[must_use]
pub fn run(config: Option<&str>, args: &AddArgs) -> i32 {
    match run_add(config, args) {
        Ok(body) => write_stdout(&body),
        Err(message) => fail(&message),
    }
}

/// Resolve one catalog row, splice it, and write once unless this is a no-op or `--dry-run`.
fn run_add(config: Option<&str>, args: &AddArgs) -> Result<String, String> {
    let config_path = configured_path(config)?;
    let terms = query_terms(&args.query)?;
    let slug_override = args.as_slug.as_deref();
    let original = read_config_text(&config_path)?;
    let base_url = resolved_base(&original)?;
    let rows = load_rows(args, &base_url)?;
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let row = catalog::resolve(&rows, &term_refs)
        .map_err(|err| format!("{err} Nothing was written."))?;
    let slug = planned_slug(row.id(), slug_override)?;
    let edited = edited_config(&original, row, slug_override, &slug, args.set_default)?;
    let unchanged = edited == original;
    let dry_run = args.dry_run;
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
        default: args.set_default,
    };
    render(args, &report, &config_path)
}

/// `--config` from the global flag. Never a home path.
fn configured_path(config: Option<&str>) -> Result<PathBuf, String> {
    match config {
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
fn query_terms(values: &[String]) -> Result<Vec<String>, String> {
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

/// OpenRouter base from `config`.
///
/// Called after the config text is read and before any catalog load. An
/// unusable setting returns here, so the cache helper and the file replace
/// never run.
///
/// # Errors
///
/// Returns an error when `[grok_router]` cannot be resolved. The config is
/// left unchanged and nothing is downloaded.
fn resolved_base(config: &str) -> Result<String, String> {
    match config_edit::router_settings(config) {
        Ok(settings) => Ok(settings.base_url),
        Err(reason) => Err(settings_failure(&reason)),
    }
}

/// What failed, why nothing was downloaded, and what to do next.
fn settings_failure(reason: &RefuseReason) -> String {
    let repair = match reason {
        RefuseReason::InvalidSetting { key, .. } => {
            format!("Fix {key} in the config and retry.")
        }
        RefuseReason::Parse { .. } => {
            "Fix the config so it parses as TOML and retry.".to_owned()
        }
        _ => "Fix the config and retry.".to_owned(),
    };
    format!("{reason}. The file was left unchanged. Nothing was downloaded. {repair}")
}

/// Load rows from `--catalog-file`, or from the cache for `base_url`.
///
/// `--catalog-file` returns before the cache helper. `base_url` is the base
/// already resolved from `--config`.
fn load_rows(args: &AddArgs, base_url: &str) -> Result<Vec<CatalogRow>, String> {
    if let Some(path) = &args.catalog_file {
        let bytes = fs::read(path).map_err(|err| {
            format!(
                "could not read catalog file {}. {err}. Nothing was written. Pass a readable --catalog-file, or omit it to use the cache.",
                path.display()
            )
        })?;
        return catalog::parse_catalog(&bytes)
            .map_err(|err| format!("{err} Nothing was written."));
    }

    let cache_dir = match &args.cache_dir {
        Some(path) => path.clone(),
        None => default_cache_dir()?,
    };
    let api_key = live_api_key()?;
    let now = SystemTime::now();
    match catalog::prepare_cached_load(&cache_dir, now, args.refresh, &api_key, base_url)
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
fn render(args: &AddArgs, report: &AddReport, path: &Path) -> Result<String, String> {
    if args.json {
        return serde_json::to_string(report).map_err(|err| {
            format!(
                "could not encode the add result as JSON: {err}. Retry the add; report this if it persists."
            )
        });
    }
    if args.quiet {
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
