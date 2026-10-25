//! `benchmarks` subcommand.
//!
//! Benchmarks reads published scores from the same provider catalog search
//! uses. The record is resolved, and its cartridge is asked for a benchmark
//! feed, before any catalog parse or download. A cartridge with no feed
//! refuses before a GET and before `--catalog-file` is read. This command
//! does not modify the config. The only write is the catalog cache, and only
//! when the command itself downloads a new body into `--cache-dir`.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use clap::{ArgAction, Args};

use crate::OutputMode;
use crate::benchmarks::{self, BenchmarkParseError, BenchmarkRow};
use crate::catalog::{self, CachedBodyOrFetch, Cartridge, CartridgeError, CatalogCartridge};
use crate::config_edit::{self, ProviderChoice, RefuseReason};
use crate::error::Error;

/// Show published price and benchmark scores from the selected provider catalog. This command does not modify the config.
/// Only an openrouter-models catalog has this feed. Other catalogs refuse before a download.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router benchmarks --catalog-file models.json --config config.toml\n  grok-router benchmarks --catalog-file models.json --config config.toml kimi"
)]
pub struct BenchmarkArgs {
    /// Case-insensitive term. Repeat to require every term. Each term must match the id, name, or description.
    ///
    /// With no terms, print models that have an intelligence index, highest first.
    #[arg(value_name = "TERM", action = ArgAction::Append, verbatim_doc_comment)]
    terms: Vec<String>,

    /// Provider record under [router.provider]. Omitted uses `default_provider`.
    #[arg(long, value_name = "NAME", verbatim_doc_comment)]
    provider: Option<String>,

    /// Read this catalog JSON instead of the network or the cache.
    #[arg(long, value_name = "PATH", verbatim_doc_comment)]
    catalog_file: Option<PathBuf>,

    /// Directory for the models.json cache. Used only when --catalog-file is absent.
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    cache_dir: Option<PathBuf>,

    /// Download a new catalog even when the cache is younger than one hour.
    #[arg(long, verbatim_doc_comment)]
    refresh: bool,

    /// Maximum rows to print.
    #[arg(
        long,
        value_name = "N",
        default_value = "30",
        value_parser = parse_limit,
        verbatim_doc_comment
    )]
    limit: usize,

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

/// Runs `benchmarks`.
///
/// Returns `0` when at least one row was printed and `1` when the command failed.
#[must_use]
pub fn run(config: Option<&str>, args: &BenchmarkArgs, mode: OutputMode) -> i32 {
    let timeout = match catalog::require_timeout(&args.timeout) {
        Ok(timeout) => timeout,
        Err(err) => return fail(mode, &Error::from(err)),
    };
    run_benchmarks(config, args, mode, timeout)
}

/// Collect flags, load rows, and print the scores.
fn run_benchmarks(
    config: Option<&str>,
    args: &BenchmarkArgs,
    mode: OutputMode,
    timeout: Duration,
) -> i32 {
    let terms = match collect_terms(&args.terms) {
        Ok(terms) => terms,
        Err(err) => return fail(mode, &err),
    };
    let rows = match load_rows(config, args, timeout) {
        Ok(rows) => rows,
        Err(err) => return fail(mode, &err),
    };
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let matched = select_rows(&rows, &term_refs);
    if matched.is_empty() {
        return fail(mode, &empty_error(&terms));
    }
    let shown = take_rows(&matched, args.limit);
    let body = match mode {
        OutputMode::Json => match format_json(&shown) {
            Ok(body) => body,
            Err(err) => return fail(mode, &err),
        },
        OutputMode::Quiet => format_quiet(&shown),
        OutputMode::Text => format_table(&shown),
    };
    write_stdout(mode, &body)
}

/// Terms from the positional arguments. An empty term is a usage failure.
///
/// # Errors
///
/// Returns an error when a positional term is empty.
fn collect_terms(values: &[String]) -> Result<Vec<String>, Error> {
    let mut terms = Vec::new();
    for term in values {
        if term.is_empty() {
            return Err(Error::BenchmarkEmptyTerm);
        }
        terms.push(term.clone());
    }
    Ok(terms)
}

/// Default view ranks by intelligence. Terms keep catalog order, including blank scores.
fn select_rows<'a>(rows: &'a [BenchmarkRow], terms: &[&str]) -> Vec<&'a BenchmarkRow> {
    if terms.is_empty() {
        benchmarks::ranked_by_intelligence(rows)
    } else {
        benchmarks::matching(rows, terms)
    }
}

/// No intelligence indexes, or no term match.
fn empty_error(terms: &[String]) -> Error {
    if terms.is_empty() {
        Error::BenchmarkNoIndex
    } else {
        Error::BenchmarkEmpty {
            quoted: quoted_terms(terms),
        }
    }
}

/// Resolve one provider, refuse a cartridge with no feed, then read catalog bytes.
///
/// The feed check happens before `--catalog-file` is read and before any GET.
///
/// # Errors
///
/// Returns an error when the config, the feed, the catalog file, or the cache cannot be used.
fn load_rows(
    config: Option<&str>,
    args: &BenchmarkArgs,
    timeout: Duration,
) -> Result<Vec<BenchmarkRow>, Error> {
    let config_path = benchmark_config_path(config)?;
    let text = read_config_text(&config_path)?;
    let choice = provider_choice(args);
    let provider = record_name(&text, choice);
    let record = config_edit::resolve_provider(&text, choice).map_err(resolve_failure)?;
    let selected = select_feed(&provider, record.catalog.as_deref())?;
    if let Some(path) = &args.catalog_file {
        let bytes = fs::read(path).map_err(|err| Error::BenchmarkCatalogFile {
            path: path.clone(),
            source: err,
        })?;
        return selected
            .benchmarks(&bytes)
            .map_err(|err| map_parse(&provider, err, true));
    }

    let api_key = bearer(&record.env_key)?;
    let cache_dir = match &args.cache_dir {
        Some(path) => path.clone(),
        None => default_cache_dir()?,
    };
    let now = SystemTime::now();
    match catalog::prepare_cached_body(
        &cache_dir,
        now,
        args.refresh,
        &api_key,
        &record.base_url,
        &record.env_key,
    )
    .map_err(|err| cache_failure(err, &record.env_key))?
    {
        CachedBodyOrFetch::Cached(bytes) => selected
            .benchmarks(&bytes)
            .map_err(|err| map_parse(&provider, err, false)),
        CachedBodyOrFetch::Fetch(request) => {
            let bytes = catalog::fetch_catalog_bytes(&request, timeout)
                .map_err(|err| cache_failure(err, &record.env_key))?;
            let stored = catalog::write_cache(&cache_dir, &record.base_url, &bytes)
                .map_err(|err| cache_failure(err, &record.env_key))?;
            if stored != catalog::cache_path_for(&cache_dir, &catalog::models_url(&record.base_url))
            {
                return Err(Error::CacheOutside { stored, cache_dir });
            }
            selected
                .benchmarks(&bytes)
                .map_err(|err| map_parse(&provider, err, false))
        }
    }
}

/// The cartridge that can read scores. A miss refuses before any catalog byte is read.
///
/// # Errors
///
/// Returns [`Error::BenchmarkNoFeed`] when the catalog id is missing, unknown, or has no feed.
fn select_feed(provider: &str, catalog_id: Option<&str>) -> Result<Cartridge, Error> {
    let selected = match catalog::cartridge(catalog_id) {
        Ok(selected) => selected,
        Err(CartridgeError::Missing) => {
            return Err(Error::BenchmarkNoFeed {
                provider: provider.to_owned(),
                catalog: None,
            });
        }
        Err(CartridgeError::Unknown { id }) => {
            return Err(Error::BenchmarkNoFeed {
                provider: provider.to_owned(),
                catalog: Some(id),
            });
        }
    };
    selected
        .benchmark_feed()
        .map_err(|err| map_parse(provider, err, true))?;
    Ok(selected)
}

/// A parse failure. `from_file` means `--catalog-file`, which did not download.
fn map_parse(provider: &str, err: BenchmarkParseError, from_file: bool) -> Error {
    match err {
        BenchmarkParseError::NoFeed { catalog_id } => Error::BenchmarkNoFeed {
            provider: provider.to_owned(),
            catalog: Some(catalog_id.to_owned()),
        },
        other if from_file => Error::BenchmarkCatalogParse { source: other },
        other => Error::BenchmarkCacheParse { source: other },
    }
}

/// `--provider` when present, including an empty name. Omitted means the default record.
fn provider_choice(args: &BenchmarkArgs) -> ProviderChoice<'_> {
    match args.provider.as_deref() {
        Some(name) => ProviderChoice::Explicit(name),
        None => ProviderChoice::Default,
    }
}

/// Name used in a feed refusal. Explicit uses the flag. Default reads the key already resolved.
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
    Error::ProviderResolve { reason }
}

/// Turn a cache or download error into the command's sentence.
fn cache_failure(err: catalog::CatalogError, env_key: &str) -> Error {
    if matches!(err, catalog::CatalogError::MissingKey { .. }) {
        Error::MissingBearer {
            env_key: env_key.to_owned(),
            source: err,
        }
    } else {
        Error::Catalog(err)
    }
}

/// `--config` when it is set. Otherwise [`default_config_path`].
///
/// Does not create the file.
///
/// # Errors
///
/// Returns an error when `--config` is empty, or when no default path can be named.
fn benchmark_config_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some("") => Err(Error::BenchmarkConfigEmpty),
        Some(path) => Ok(PathBuf::from(path)),
        None => default_config_path(),
    }
}

/// `${GROK_HOME:-$HOME/.grok}/config.toml`. An empty `GROK_HOME` falls through to `HOME`.
///
/// # Errors
///
/// Returns an error when `GROK_HOME` is unset or empty and `HOME` cannot name a path.
fn default_config_path() -> Result<PathBuf, Error> {
    if let Some(grok_home) = env::var_os("GROK_HOME")
        && !grok_home.is_empty()
    {
        return Ok(PathBuf::from(grok_home).join("config.toml"));
    }
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".grok").join("config.toml")),
        Some(_) => Err(Error::BenchmarkHomeEmpty),
        None => Err(Error::BenchmarkHomeUnset),
    }
}

/// Read `path`. A missing file is a missing `[router]` record and is not created.
///
/// # Errors
///
/// Returns an error when the file is missing, unreadable, or not UTF-8.
fn read_config_text(path: &Path) -> Result<String, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(resolve_failure(RefuseReason::InvalidSetting {
                key: "router".to_owned(),
                detail: "[router] is missing; write that table".to_owned(),
            }));
        }
        Err(err) => {
            return Err(Error::BenchmarkConfigIo {
                path: path.to_path_buf(),
                source: err,
            });
        }
    };
    String::from_utf8(bytes).map_err(|_| Error::BenchmarkConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// Default cache directory `~/.cache/grok-router`.
///
/// # Errors
///
/// Returns an error when `HOME` is unset or empty.
fn default_cache_dir() -> Result<PathBuf, Error> {
    let Some(home) = env::var_os("HOME") else {
        return Err(Error::BenchmarkCacheHomeUnset);
    };
    if home.is_empty() {
        return Err(Error::BenchmarkCacheHomeEmpty);
    }
    Ok(PathBuf::from(home).join(".cache").join("grok-router"))
}

/// The record's bearer, or an empty string when the variable is unset.
///
/// A blank value is left blank. [`catalog::prepare_cached_body`] accepts that
/// when the cache is fresh and refuses it when a download is required.
///
/// # Errors
///
/// Returns an error when the variable is set to non-Unicode bytes.
fn bearer(env_key: &str) -> Result<String, Error> {
    match env::var(env_key) {
        Ok(value) => Ok(value),
        Err(env::VarError::NotPresent) => Ok(String::new()),
        Err(env::VarError::NotUnicode(_)) => Err(Error::BenchmarkEnvKeyNotUnicode {
            name: env_key.to_owned(),
        }),
    }
}

/// `--limit` rejected by clap before benchmarks runs.
#[derive(Debug, thiserror::Error)]
enum LimitParse {
    /// The value was not a whole number.
    #[error("--limit {raw} is not a whole number. Pass --limit 30.")]
    NotWhole {
        /// The text that was passed.
        raw: String,
    },
    /// Zero would hide every row.
    #[error("--limit 0 would hide every row. Pass --limit 30 or another number greater than zero.")]
    Zero,
}

/// Reject zero. Clap prints the returned error and exits.
///
/// # Errors
///
/// Returns an error when the value is zero or not a whole number.
fn parse_limit(raw: &str) -> Result<usize, LimitParse> {
    let limit: usize = raw.parse().map_err(|_| LimitParse::NotWhole {
        raw: raw.to_owned(),
    })?;
    if limit == 0 {
        return Err(LimitParse::Zero);
    }
    Ok(limit)
}

/// First `limit` rows, in the order they were selected.
fn take_rows<'a>(rows: &[&'a BenchmarkRow], limit: usize) -> Vec<&'a BenchmarkRow> {
    let mut taken = Vec::new();
    for row in rows {
        if taken.len() == limit {
            break;
        }
        taken.push(*row);
    }
    taken
}

/// Terms as the empty-result sentence already quotes them.
fn quoted_terms(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("{term:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// JSON array of id, name, price per million, and the three indexes.
///
/// # Errors
///
/// Returns [`Error::JsonEncode`] when the rows cannot be encoded.
fn format_json(rows: &[&BenchmarkRow]) -> Result<String, Error> {
    let hits = rows
        .iter()
        .map(|row| JsonHit {
            id: row.id(),
            name: row.display_name(),
            prompt_per_million: row.prompt_per_million(),
            completion_per_million: row.completion_per_million(),
            intelligence_index: row.intelligence_index(),
            coding_index: row.coding_index(),
            agentic_index: row.agentic_index(),
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&hits).map_err(|err| Error::JsonEncode {
        what: "benchmark rows",
        source: err,
        retry: "Retry benchmarks; report this if it persists.",
    })
}

/// One id per line and no header.
fn format_quiet(rows: &[&BenchmarkRow]) -> String {
    rows.iter()
        .map(|row| row.id())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain table of id, price per million, and the three indexes.
fn format_table(rows: &[&BenchmarkRow]) -> String {
    let mut id_width = "ID".chars().count();
    let mut prompt_width = "PROMPT$/M".chars().count();
    let mut completion_width = "COMPLETION$/M".chars().count();
    let mut intelligence_width = "INTELLIGENCE".chars().count();
    let mut coding_width = "CODING".chars().count();
    let mut agentic_width = "AGENTIC".chars().count();
    for row in rows {
        id_width = id_width.max(row.id().chars().count());
        prompt_width = prompt_width.max(price_cell(row.prompt_per_million()).chars().count());
        completion_width =
            completion_width.max(price_cell(row.completion_per_million()).chars().count());
        intelligence_width = intelligence_width.max(row.intelligence_text().chars().count());
        coding_width = coding_width.max(row.coding_text().chars().count());
        agentic_width = agentic_width.max(row.agentic_text().chars().count());
    }
    let mut lines = Vec::with_capacity(rows.len().saturating_add(1));
    lines.push(format!(
        "{header_id:<id_width$}  {header_prompt:<prompt_width$}  {header_completion:<completion_width$}  {header_intelligence:<intelligence_width$}  {header_coding:<coding_width$}  {header_agentic:<agentic_width$}",
        header_id = "ID",
        header_prompt = "PROMPT$/M",
        header_completion = "COMPLETION$/M",
        header_intelligence = "INTELLIGENCE",
        header_coding = "CODING",
        header_agentic = "AGENTIC",
    ));
    for row in rows {
        lines.push(format!(
            "{id:<id_width$}  {prompt:<prompt_width$}  {completion:<completion_width$}  {intelligence:<intelligence_width$}  {coding:<coding_width$}  {agentic:<agentic_width$}",
            id = row.id(),
            prompt = price_cell(row.prompt_per_million()),
            completion = price_cell(row.completion_per_million()),
            intelligence = row.intelligence_text(),
            coding = row.coding_text(),
            agentic = row.agentic_text(),
        ));
    }
    lines.join("\n")
}

/// A published price, or `-` when the document left it blank.
fn price_cell(price: Option<&str>) -> String {
    price.unwrap_or("-").to_owned()
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
                what: "benchmark rows",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "benchmarks", err)
}

/// One JSON object in the benchmarks array.
#[derive(serde::Serialize)]
struct JsonHit<'a> {
    /// Provider id.
    id: &'a str,
    /// Display name.
    name: &'a str,
    /// Prompt price in dollars per million tokens.
    prompt_per_million: Option<&'a str>,
    /// Completion price in dollars per million tokens.
    completion_per_million: Option<&'a str>,
    /// Artificial Analysis intelligence index.
    intelligence_index: Option<&'a serde_json::Number>,
    /// Artificial Analysis coding index.
    coding_index: Option<&'a serde_json::Number>,
    /// Artificial Analysis agentic index.
    agentic_index: Option<&'a serde_json::Number>,
}
