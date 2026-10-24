//! `add` subcommand.
//!
//! Add writes one model from the selected `[router]` provider record into the
//! file named by `--config`. The record is resolved, and its cartridge is
//! selected, before any catalog parse or download. `--catalog-file` reads
//! local bytes and does not use the network. Without that flag, a missing or
//! blank bearer fails before any config write and does not write the cache.
//! A missing `--config` fails before any home path is opened.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use clap::{ArgAction, Args};

use crate::catalog::{self, CachedBodyOrFetch, Cartridge, CartridgeError, CatalogCartridge};
use crate::config_edit::{self, Backup, ProviderChoice, RefuseReason};

/// Add one model from the selected provider record. This command writes unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router add --catalog-file models.json --config config.toml z-ai/glm-5.3\n  grok-router add --provider deepseek --catalog-file deepseek.json --config config.toml deepseek-v4-pro"
)]
pub struct AddArgs {
    /// Provider id or terms. Repeat to require every term. Optional when the catalog has one row.
    #[arg(value_name = "QUERY", action = ArgAction::Append, verbatim_doc_comment)]
    query: Vec<String>,

    /// Provider record under [router.provider]. Omitted uses default_provider.
    #[arg(long, value_name = "NAME", verbatim_doc_comment)]
    provider: Option<String>,

    /// Slug to write instead of the cartridge slug.
    #[arg(long = "as", value_name = "SLUG", verbatim_doc_comment)]
    as_slug: Option<String>,

    /// Set this slug as [models].default in the same write.
    #[arg(long = "default", verbatim_doc_comment)]
    set_default: bool,

    /// Read this catalog JSON instead of the network or the cache.
    #[arg(long, value_name = "PATH", verbatim_doc_comment)]
    catalog_file: Option<PathBuf>,

    /// Directory for the catalog cache. Used only when --catalog-file is absent.
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

/// Resolve one provider record, parse with its cartridge, and write once.
fn run_add(config: Option<&str>, args: &AddArgs) -> Result<String, String> {
    let config_path = configured_path(config)?;
    let original = read_config_text(&config_path)?;
    let choice = provider_choice(args);
    let record = config_edit::resolve_provider(&original, choice)
        .map_err(|reason| resolve_failure(&reason))?;
    let selected = catalog::cartridge(record.catalog.as_deref())
        .map_err(|err| cartridge_failure(&record_name(&original, choice), &err))?;
    let _builtins =
        config_edit::builtins_from(&original).map_err(|reason| early_refusal(&reason))?;
    let terms = query_terms(&args.query)?;
    let rows = load_rows(args, &selected, &record.base_url, &record.env_key)?;
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let row =
        catalog::resolve(&rows, &term_refs).map_err(|err| format!("{err} Nothing was written."))?;
    let slug = selected
        .slug(row.id(), args.as_slug.as_deref())
        .map_err(|reason| edit_failure(&reason))?;
    let edited = edited_config(
        &original,
        row,
        &slug,
        &record.base_url,
        &record.env_key,
        args.set_default,
    )?;
    let unchanged = edited == original;
    let dry_run = args.dry_run;
    let wrote = if unchanged || dry_run {
        false
    } else {
        pasted_secret(&edited)?;
        config_edit::replace_file(&config_path, Backup::Sibling, edited.as_bytes(), "")
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

/// `--provider` when present, including an empty name. Omitted means the default record.
fn provider_choice(args: &AddArgs) -> ProviderChoice<'_> {
    match args.provider.as_deref() {
        Some(name) => ProviderChoice::Explicit(name),
        None => ProviderChoice::Default,
    }
}

/// Name used in a cartridge refusal. Explicit uses the flag. Default reads the key already resolved.
fn record_name(config: &str, choice: ProviderChoice<'_>) -> String {
    match choice {
        ProviderChoice::Explicit(name) => name.to_owned(),
        ProviderChoice::Default => config
            .parse::<toml::Table>()
            .ok()
            .as_ref()
            .and_then(|root| root.get("router"))
            .and_then(toml::Value::as_table)
            .and_then(|router| router.get("default_provider"))
            .and_then(toml::Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or("the default provider")
            .to_owned(),
    }
}

/// What failed, why the record was not used, and that nothing was downloaded or written.
fn resolve_failure(reason: &RefuseReason) -> String {
    let preserved = "The file was left unchanged. Nothing was downloaded. Nothing was written.";
    let must_write = match reason {
        RefuseReason::InvalidSetting { key, detail }
            if key == "router"
                || key == "default_provider"
                || key == "provider"
                || detail.contains("[router]") =>
        {
            " The operator must write that record."
        }
        _ => "",
    };
    format!("{reason}.{must_write} {preserved}")
}

/// A cartridge refusal names the provider and does not parse, download, or write.
fn cartridge_failure(provider: &str, err: &CartridgeError) -> String {
    let next = "Set catalog to openrouter-models or deepseek-list-models, or pass --provider for a record that already has one.";
    let preserved = "Nothing was downloaded. Nothing was written.";
    match err {
        CartridgeError::Missing => format!(
            "{provider} has no catalog cartridge configured. {preserved} {next}"
        ),
        CartridgeError::Unknown { id } => format!(
            "catalog {id} on {provider} is not a cartridge, so no catalog cartridge is configured. {preserved} {next}"
        ),
    }
}

/// Built-in membership failed before any catalog parse or download.
fn early_refusal(reason: &RefuseReason) -> String {
    format!("{reason}. The file was left unchanged. Nothing was downloaded. Nothing was written.")
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

/// Load rows from `--catalog-file`, or from the cache for the record base.
///
/// `--catalog-file` calls [`CatalogCartridge::parse`] and does not call
/// [`catalog::parse_catalog`] or the network. The cache path calls
/// [`catalog::prepare_cached_body`] and then the same parse. It does not call
/// [`catalog::prepare_cached_load`].
fn load_rows(
    args: &AddArgs,
    cartridge: &Cartridge,
    base_url: &str,
    env_key: &str,
) -> Result<Vec<crate::catalog::CatalogRow>, String> {
    if let Some(path) = &args.catalog_file {
        let bytes = fs::read(path).map_err(|err| {
            format!(
                "could not read catalog file {}. {err}. Nothing was written. Pass a readable --catalog-file, or omit it to use the cache.",
                path.display()
            )
        })?;
        return cartridge
            .parse(&bytes)
            .map_err(|err| format!("{err} Nothing was written."));
    }

    let api_key = required_bearer(env_key)?;
    let cache_dir = match &args.cache_dir {
        Some(path) => path.clone(),
        None => default_cache_dir()?,
    };
    let now = SystemTime::now();
    match catalog::prepare_cached_body(&cache_dir, now, args.refresh, &api_key, base_url)
        .map_err(|err| format!("{err} Nothing was written."))?
    {
        CachedBodyOrFetch::Cached(bytes) => cartridge
            .parse(&bytes)
            .map_err(|err| format!("{err} Nothing was written.")),
        CachedBodyOrFetch::Fetch(request) => {
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
            cartridge
                .parse(&bytes)
                .map_err(|err| format!("{err} The config was not written."))
        }
    }
}

/// The record's bearer. A missing or blank value fails before a download or a cache write.
fn required_bearer(env_key: &str) -> Result<String, String> {
    match env::var(env_key) {
        Ok(value) if !value.is_empty() && !value.chars().all(char::is_whitespace) => Ok(value),
        Ok(_) | Err(env::VarError::NotPresent) => Err(missing_bearer(env_key)),
        Err(env::VarError::NotUnicode(_)) => Err(format!(
            "{env_key} is not valid Unicode, so the config was not written. Export a Unicode key or pass --catalog-file."
        )),
    }
}

/// Missing-key sentence. `OPENROUTER_API_KEY` keeps the sentence the cache helper already uses.
fn missing_bearer(env_key: &str) -> String {
    if env_key == "OPENROUTER_API_KEY" {
        "OPENROUTER_API_KEY is missing, so the catalog was not downloaded. Export OPENROUTER_API_KEY or pass --catalog-file. The config was not written and an empty cache was not written."
            .to_owned()
    } else {
        format!(
            "{env_key} is missing, so the catalog was not downloaded. Export {env_key} or pass --catalog-file. The config was not written and an empty cache was not written."
        )
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

/// Insert the row with the record's base and env name, then set the default when asked.
fn edited_config(
    original: &str,
    row: &crate::catalog::CatalogRow,
    slug: &str,
    base_url: &str,
    env_key: &str,
    set_default: bool,
) -> Result<String, String> {
    let inserted =
        config_edit::insert_model(original, row.as_catalog_model(), slug, base_url, env_key)
            .map_err(|refused| edit_failure(&refused.reason))?;
    if !set_default {
        return Ok(inserted);
    }
    config_edit::set_default(&inserted, slug).map_err(|refused| edit_failure(&refused.reason))
}

/// Refuse when either watched secret value would be written. The message names the variable, not the value.
fn pasted_secret(text: &str) -> Result<(), String> {
    for name in ["DEEPSEEK_API_KEY", "OPENROUTER_API_KEY"] {
        match env::var(name) {
            Ok(value) if !value.is_empty() && text.contains(&value) => {
                return Err(format!(
                    "the new config contains the value of {name}. The file was left unchanged. Remove that value from the catalog fields before adding."
                ));
            }
            Ok(_) | Err(env::VarError::NotPresent) => {}
            Err(env::VarError::NotUnicode(_)) => {
                return Err(format!(
                    "{name} is not valid Unicode, so the config was not written. Export a Unicode value or unset {name}."
                ));
            }
        }
    }
    Ok(())
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
            "Choose a catalog row whose context window is greater than zero.".to_owned()
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
        return Ok(format!("{}\nwould write {}", report.slug, path.display()));
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
