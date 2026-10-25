//! `add` subcommand.
//!
//! Add writes one model into the file named by `--config`. The catalog path
//! resolves one `[router]` provider record and selects its cartridge before
//! any catalog parse or download. The direct path runs when `--model`,
//! `--base-url`, `--env-key`, and `--api-backend` are all present: it writes
//! one block from those flags and does not resolve a provider. `--default` on
//! that path sets `[models].default` in the same write. A refused default
//! writes nothing. `--catalog-file` reads local bytes and does not use the
//! network. Without that flag, a missing or blank bearer fails before any
//! config write and does not write the cache. A missing `--config` fails
//! before any home path is opened.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use clap::{ArgAction, Args};

use crate::catalog::{self, CachedBodyOrFetch, Cartridge, CartridgeError, CatalogCartridge};
use crate::config_edit::{self, Backup, ProviderChoice, RefuseReason};
use crate::error::Error;
use crate::OutputMode;

/// Add one model from the selected provider record, or write one block from --model, --base-url, --env-key, and --api-backend without resolving a provider. This command writes unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router add --catalog-file models.json --config config.toml z-ai/glm-5.3\n  grok-router add --provider deepseek --catalog-file deepseek.json --config config.toml deepseek-v4-pro\n  grok-router add --config config.toml --as example-model --model example-model --base-url https://example.test/v1 --api-backend messages --env-key EXAMPLE_API_KEY"
)]
pub struct AddArgs {
    /// Provider id or terms. Repeat to require every term. Optional when the catalog has one row.
    #[arg(value_name = "QUERY", action = ArgAction::Append, verbatim_doc_comment)]
    query: Vec<String>,

    /// Provider record under [router.provider]. Omitted uses default_provider.
    #[arg(long, value_name = "NAME", verbatim_doc_comment)]
    provider: Option<String>,

    /// Slug to write instead of the cartridge slug. Required on the direct path.
    #[arg(long = "as", value_name = "SLUG", verbatim_doc_comment)]
    as_slug: Option<String>,

    /// Model id written on the direct path. Not trimmed, and not used as the slug.
    #[arg(long, value_name = "ID", verbatim_doc_comment)]
    model: Option<String>,

    /// Base URL written on the direct path. Not trimmed.
    #[arg(long, value_name = "URL", verbatim_doc_comment)]
    base_url: Option<String>,

    /// Environment variable name written on the direct path. Not trimmed.
    #[arg(long, value_name = "KEY", verbatim_doc_comment)]
    env_key: Option<String>,

    /// api_backend value written on the direct path. Not trimmed.
    #[arg(long, value_name = "BACKEND", verbatim_doc_comment)]
    api_backend: Option<String>,

    /// Display name written on the direct path. Omitted leaves the name line out.
    #[arg(long, value_name = "TEXT", verbatim_doc_comment)]
    name: Option<String>,

    /// context_window written on the direct path when greater than zero.
    #[arg(
        long,
        value_name = "N",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    context_window: Option<i64>,

    /// Write `vision = true` on the direct path. Omitted leaves the line out.
    #[arg(long, action = ArgAction::SetTrue, verbatim_doc_comment)]
    vision: bool,

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

    /// Seconds to wait for a catalog GET. 0 refuses before any download or write.
    #[arg(
        long = "timeout",
        env = "GROK_ROUTER_TIMEOUT",
        value_name = "SECONDS",
        default_value = "30",
        verbatim_doc_comment
    )]
    timeout: String,
}

/// Runs `add`.
///
/// Returns `0` when the model is present in the config (or would be, under `--dry-run`).
/// Returns `1` when the add did not apply.
#[must_use]
pub fn run(config: Option<&str>, args: &AddArgs, mode: OutputMode) -> i32 {
    let timeout = match catalog::require_timeout(&args.timeout) {
        Ok(timeout) => timeout,
        Err(message) => return fail(mode, &Error::command(message)),
    };
    match run_add(config, args, mode, timeout) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// The four flags that select the direct path. Order is the refusal order.
const DIRECT_FLAGS: [&str; 4] = ["--model", "--base-url", "--env-key", "--api-backend"];

/// Resolve one provider record, or write one direct block, and write once.
fn run_add(
    config: Option<&str>,
    args: &AddArgs,
    mode: OutputMode,
    timeout: Duration,
) -> Result<String, Error> {
    let config_path = configured_path(config)?;
    let original = read_config_text(&config_path)?;
    let present = direct_flags_present(args);
    let missing = direct_flags_missing(&present);
    if present.is_empty() {
        let extras = catalog_refused_flags(args);
        if !extras.is_empty() {
            return Err(Error::command(catalog_flag_refusal(&extras)));
        }
        return run_catalog_add(&config_path, &original, args, mode, timeout);
    }
    if !missing.is_empty() {
        return Err(Error::command(partial_direct_refusal(&present, &missing)));
    }
    run_direct_add(&config_path, &original, args, mode)
}

/// Which of [`DIRECT_FLAGS`] were passed, including an empty value.
fn direct_flags_present(args: &AddArgs) -> Vec<&'static str> {
    let passed = [
        args.model.is_some(),
        args.base_url.is_some(),
        args.env_key.is_some(),
        args.api_backend.is_some(),
    ];
    DIRECT_FLAGS
        .into_iter()
        .zip(passed)
        .filter_map(|(name, passed)| passed.then_some(name))
        .collect()
}

/// [`DIRECT_FLAGS`] that [`direct_flags_present`] did not return.
fn direct_flags_missing(present: &[&str]) -> Vec<&'static str> {
    DIRECT_FLAGS
        .into_iter()
        .filter(|name| !present.contains(name))
        .collect()
}

/// Flags that belong only to the direct path.
fn catalog_refused_flags(args: &AddArgs) -> Vec<&'static str> {
    let mut names = Vec::new();
    if args.name.is_some() {
        names.push("--name");
    }
    if args.context_window.is_some() {
        names.push("--context-window");
    }
    if args.vision {
        names.push("--vision");
    }
    names
}

/// Catalog flags that select a provider, a cartridge, or a download.
fn direct_conflicts(args: &AddArgs) -> Vec<&'static str> {
    let mut names = Vec::new();
    if args.provider.is_some() {
        names.push("--provider");
    }
    if !args.query.is_empty() {
        names.push("a query");
    }
    if args.catalog_file.is_some() {
        names.push("--catalog-file");
    }
    if args.cache_dir.is_some() {
        names.push("--cache-dir");
    }
    if args.refresh {
        names.push("--refresh");
    }
    names
}

/// Join flag names as `a`, `a and b`, or `a, b, and c`.
fn english_list(items: &[&str]) -> String {
    let mut listed = String::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            let last = index.saturating_add(1) == items.len();
            if items.len() == 2 {
                listed.push_str(" and ");
            } else if last {
                listed.push_str(", and ");
            } else {
                listed.push_str(", ");
            }
        }
        listed.push_str(item);
    }
    listed
}

/// A mix of the four direct flags. Names what was passed and what was not.
fn partial_direct_refusal(present: &[&str], missing: &[&str]) -> String {
    format!(
        "{} {} set, but {} {} missing. Pass all four of --model, --base-url, --env-key, and --api-backend, or none of them. The file was left unchanged. Nothing was downloaded. Nothing was written.",
        english_list(present),
        if present.len() == 1 { "was" } else { "were" },
        english_list(missing),
        if missing.len() == 1 { "was" } else { "were" },
    )
}

/// `--name`, `--context-window`, or `--vision` without the four direct flags.
fn catalog_flag_refusal(extras: &[&str]) -> String {
    format!(
        "{} {} only used together with --model, --base-url, --env-key, and --api-backend. The file was left unchanged. Nothing was downloaded. Nothing was written.",
        english_list(extras),
        if extras.len() == 1 { "is" } else { "are" },
    )
}

/// Catalog selectors passed to a direct add.
fn direct_conflict_refusal(conflicts: &[&str]) -> String {
    format!(
        "{} cannot be combined with --model, --base-url, --env-key, and --api-backend. The file was left unchanged. Nothing was downloaded. Nothing was written.",
        english_list(conflicts)
    )
}

/// `--as` for a direct add. Empty is not trimmed into absence.
fn required_direct_slug(args: &AddArgs) -> Result<&str, String> {
    match args.as_slug.as_deref() {
        Some(slug) if !slug.is_empty() => Ok(slug),
        Some(_) => Err(
            "--as is empty, so nothing was written. Pass --as with a slug. The file was left unchanged. Nothing was downloaded. Nothing was written."
                .to_owned(),
        ),
        None => Err(
            "--as is required with --model, --base-url, --env-key, and --api-backend. Pass --as with the slug to write. The file was left unchanged. Nothing was downloaded. Nothing was written."
                .to_owned(),
        ),
    }
}

/// Write one block from the four direct flags. Does not resolve a provider or download a catalog.
///
/// `--default` sets `[models].default` in the same `replace_file`. A refused default writes nothing.
fn run_direct_add(
    config_path: &Path,
    original: &str,
    args: &AddArgs,
    mode: OutputMode,
) -> Result<String, Error> {
    let conflicts = direct_conflicts(args);
    if !conflicts.is_empty() {
        return Err(Error::command(direct_conflict_refusal(&conflicts)));
    }
    let slug = required_direct_slug(args)?;
    let (model, base_url, env_key, api_backend) = match (
        args.model.as_deref(),
        args.base_url.as_deref(),
        args.env_key.as_deref(),
        args.api_backend.as_deref(),
    ) {
        (Some(model), Some(base_url), Some(env_key), Some(api_backend)) => {
            (model, base_url, env_key, api_backend)
        }
        _ => {
            return Err(Error::command(partial_direct_refusal(
                &direct_flags_present(args),
                &direct_flags_missing(&direct_flags_present(args)),
            )));
        }
    };
    let inserted = config_edit::insert_direct(
        original,
        config_edit::DirectBlock {
            slug,
            model,
            base_url,
            name: args.name.as_deref(),
            api_backend,
            env_key,
            context_window: args.context_window,
            vision: args.vision,
        },
    )
    .map_err(|refused| edit_failure(refused.reason))?;
    // `Refused.text` is the inserted document. Do not write it: the file keeps `original`.
    let edited = if args.set_default {
        match config_edit::set_default(&inserted, slug) {
            Ok(text) => text,
            Err(refused) => return Err(edit_failure(refused.reason)),
        }
    } else {
        inserted
    };
    let unchanged = edited == original;
    let dry_run = args.dry_run;
    let wrote = if unchanged || dry_run {
        false
    } else {
        config_edit::refuse_named_secrets(&edited, &[env_key])?;
        config_edit::replace_file(config_path, Backup::Sibling, edited.as_bytes(), "").map_err(
            |err| {
                let message = format!(
                    "{err}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the edit."
                );
                Error::replace(err, message)
            },
        )?;
        true
    };
    let report = AddReport {
        slug: slug.to_owned(),
        provider_id: model.to_owned(),
        wrote,
        dry_run,
        unchanged,
        default: args.set_default,
    };
    render(mode, &report, config_path).map_err(Error::from)
}

/// Resolve one provider record, parse with its cartridge, and write once.
fn run_catalog_add(
    config_path: &Path,
    original: &str,
    args: &AddArgs,
    mode: OutputMode,
    timeout: Duration,
) -> Result<String, Error> {
    let choice = provider_choice(args);
    let record =
        config_edit::resolve_provider(original, choice).map_err(resolve_failure)?;
    let selected = catalog::cartridge(record.catalog.as_deref())
        .map_err(|err| cartridge_failure(&record_name(original, choice), err))?;
    let _builtins = config_edit::builtins_from(original).map_err(early_refusal)?;
    let terms = query_terms(&args.query)?;
    let rows = load_rows(args, &selected, &record.base_url, &record.env_key, timeout)?;
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let row =
        catalog::resolve(&rows, &term_refs).map_err(|err| {
            let message = format!("{err} Nothing was written.");
            Error::resolve(err, message)
        })?;
    let slug = selected
        .slug(row.id(), args.as_slug.as_deref())
        .map_err(edit_failure)?;
    let edited = edited_config(
        original,
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
        config_edit::refuse_named_secrets(&edited, &[record.env_key.as_str()])?;
        config_edit::replace_file(config_path, Backup::Sibling, edited.as_bytes(), "").map_err(
            |err| {
                let message = format!(
                    "{err}. The config was not replaced. Retry with a writable --config, or pass --dry-run to preview the edit."
                );
                Error::replace(err, message)
            },
        )?;
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
    render(mode, &report, config_path).map_err(Error::from)
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
fn resolve_failure(reason: RefuseReason) -> Error {
    let preserved = "The file was left unchanged. Nothing was downloaded. Nothing was written.";
    let must_write = match &reason {
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
    let message = format!("{reason}.{must_write} {preserved}");
    Error::refused(reason, message)
}

/// A cartridge refusal names the provider and does not parse, download, or write.
fn cartridge_failure(provider: &str, err: CartridgeError) -> Error {
    let next = "Set catalog to openrouter-models or deepseek-list-models, or pass --provider for a record that already has one.";
    let preserved = "Nothing was downloaded. Nothing was written.";
    let message = match &err {
        CartridgeError::Missing => {
            format!("{provider} has no catalog cartridge configured. {preserved} {next}")
        }
        CartridgeError::Unknown { id } => format!(
            "catalog {id} on {provider} is not a cartridge, so no catalog cartridge is configured. {preserved} {next}"
        ),
    };
    Error::cartridge(err, message)
}

/// Built-in membership failed before any catalog parse or download.
fn early_refusal(reason: RefuseReason) -> Error {
    let message = format!(
        "{reason}. The file was left unchanged. Nothing was downloaded. Nothing was written."
    );
    Error::refused(reason, message)
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
/// [`catalog::prepare_cached_body`] and then the same parse.
fn load_rows(
    args: &AddArgs,
    cartridge: &Cartridge,
    base_url: &str,
    env_key: &str,
    timeout: Duration,
) -> Result<Vec<crate::catalog::CatalogRow>, Error> {
    if let Some(path) = &args.catalog_file {
        let bytes = fs::read(path).map_err(|err| {
            format!(
                "could not read catalog file {}. {err}. Nothing was written. Pass a readable --catalog-file, or omit it to use the cache.",
                path.display()
            )
        })?;
        return cartridge.parse(&bytes).map_err(|err| {
            let message = format!("{err} Nothing was written.");
            Error::catalog(err, message)
        });
    }

    let api_key = required_bearer(env_key)?;
    let cache_dir = match &args.cache_dir {
        Some(path) => path.clone(),
        None => default_cache_dir()?,
    };
    let now = SystemTime::now();
    match catalog::prepare_cached_body(&cache_dir, now, args.refresh, &api_key, base_url, env_key)
        .map_err(|err| {
            let message = format!("{err} Nothing was written.");
            Error::catalog(err, message)
        })?
    {
        CachedBodyOrFetch::Cached(bytes) => cartridge.parse(&bytes).map_err(|err| {
            let message = format!("{err} Nothing was written.");
            Error::catalog(err, message)
        }),
        CachedBodyOrFetch::Fetch(request) => {
            let bytes = catalog::fetch_catalog_bytes(&request, timeout).map_err(|err| {
                let message = format!("{err} Nothing was written.");
                Error::catalog(err, message)
            })?;
            let stored = catalog::write_cache(&cache_dir, base_url, &bytes).map_err(|err| {
                let message = format!("{err} Nothing was written.");
                Error::catalog(err, message)
            })?;
            if stored != catalog::cache_path_for(&cache_dir, &catalog::models_url(base_url)) {
                return Err(Error::command(format!(
                    "the catalog cache was written to {}, outside {}. The config was not written. Pass --cache-dir.",
                    stored.display(),
                    cache_dir.display()
                )));
            }
            cartridge.parse(&bytes).map_err(|err| {
                let message = format!("{err} The config was not written.");
                Error::catalog(err, message)
            })
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

/// Missing-key sentence. The name is the record's `env_key`.
fn missing_bearer(env_key: &str) -> String {
    format!(
        "{env_key} is missing, so the catalog was not downloaded. Export {env_key} or pass --catalog-file. The config was not written and an empty cache was not written."
    )
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
) -> Result<String, Error> {
    let inserted =
        config_edit::insert_model(original, row.as_catalog_model(), slug, base_url, env_key)
            .map_err(|refused| edit_failure(refused.reason))?;
    if !set_default {
        return Ok(inserted);
    }
    config_edit::set_default(&inserted, slug).map_err(|refused| edit_failure(refused.reason))
}

/// What failed, why, and what to do next. The file was not written.
fn edit_failure(reason: RefuseReason) -> Error {
    let message = format!(
        "{reason}. The file was left unchanged. {}",
        next_step(&reason)
    );
    Error::refused(reason, message)
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
fn render(mode: OutputMode, report: &AddReport, path: &Path) -> Result<String, String> {
    if mode == OutputMode::Json {
        return serde_json::to_string(report).map_err(|err| {
            format!(
                "could not encode the add result as JSON: {err}. Retry the add; report this if it persists."
            )
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
            &Error::command(format!(
                "could not write the add result: {err}. Retry when stdout accepts output."
            )),
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "add", err)
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
