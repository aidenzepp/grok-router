//! `search` subcommand.
//!
//! Search reads the selected provider catalog from `--catalog-file` or from a
//! cache. The record is resolved, and its cartridge is selected, before any
//! catalog parse or download. `--catalog-file` reads local bytes and does not
//! use the network. A missing config file is a miss: it is not created, and
//! search does not fall back to a compiled base. Search does not modify the
//! config. The only write is the catalog cache, and only when the command
//! itself downloads a new body into `--cache-dir`.

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use clap::{ArgAction, Args};

use crate::catalog::{
    self, CachedBodyOrFetch, CartridgeError, CatalogCartridge, CatalogError, CatalogRow,
};
use crate::config_edit::{self, ProviderChoice, RefuseReason};
use crate::error::Error;
use crate::OutputMode;

/// Search the selected provider catalog. This command does not modify the config.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router search --catalog-file models.json --config config.toml glm\n  grok-router search --provider deepseek --catalog-file deepseek.json --config config.toml deepseek"
)]
pub struct SearchArgs {
    /// Case-insensitive term. Repeat to require every term. Each term must match the id, name, or description.
    #[arg(value_name = "TERM", action = ArgAction::Append, verbatim_doc_comment)]
    terms: Vec<String>,

    /// Provider record under [router.provider]. Omitted uses default_provider.
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

/// Runs `search`.
///
/// Returns `0` when at least one row was printed and `1` when the search failed.
#[must_use]
pub fn run(config: Option<&str>, args: &SearchArgs, mode: OutputMode) -> i32 {
    let timeout = match catalog::require_timeout(&args.timeout) {
        Ok(timeout) => timeout,
        Err(err) => return fail(mode, &Error::from(err)),
    };
    run_search(config, args, mode, timeout)
}

/// Collect flags, load rows, and print the matches.
fn run_search(config: Option<&str>, args: &SearchArgs, mode: OutputMode, timeout: Duration) -> i32 {
    let terms = match collect_terms(&args.terms) {
        Ok(terms) => terms,
        Err(err) => return fail(mode, &err),
    };
    let rows = match load_rows(config, args, timeout) {
        Ok(rows) => rows,
        Err(err) => return fail(mode, &err),
    };
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let matched = catalog::matching(&rows, &term_refs);
    if matched.is_empty() {
        return fail(
            mode,
            &Error::SearchEmpty {
                quoted: quoted_terms(&terms),
            },
        );
    }
    let shown = take_rows(&matched, args.limit);
    let color = allow_color(io::stdout().is_terminal(), no_color_is_set());
    let body = match mode {
        OutputMode::Json => match format_json(&shown) {
            Ok(body) => body,
            Err(err) => return fail(mode, &err),
        },
        OutputMode::Quiet => format_quiet(&shown, color),
        OutputMode::Text => format_table(&shown, color),
    };
    write_stdout(mode, &body)
}

/// Terms from the positional arguments. An empty term is a usage failure.
fn collect_terms(values: &[String]) -> Result<Vec<String>, Error> {
    let mut terms = Vec::new();
    for term in values {
        if term.is_empty() {
            return Err(Error::SearchEmptyTerm);
        }
        terms.push(term.clone());
    }
    Ok(terms)
}

/// Resolve one provider record, select its cartridge, then read catalog bytes.
///
/// `--catalog-file` does not skip resolution and does not GET. The network path
/// calls [`catalog::prepare_cached_body`] and then [`CatalogCartridge::parse`].
/// This function does not call [`catalog::parse_catalog`] and does not call
/// [`CatalogCartridge::slug`].
///
/// # Errors
///
/// Returns an error when the config, the cartridge, the catalog file, or the cache cannot be used.
fn load_rows(
    config: Option<&str>,
    args: &SearchArgs,
    timeout: Duration,
) -> Result<Vec<CatalogRow>, Error> {
    let config_path = search_config_path(config)?;
    let text = read_config_text(&config_path)?;
    let choice = provider_choice(args);
    let record =
        config_edit::resolve_provider(&text, choice).map_err(resolve_failure)?;
    let selected = catalog::cartridge(record.catalog.as_deref())
        .map_err(|err| cartridge_failure(&record_name(&text, choice), err))?;
    if let Some(path) = &args.catalog_file {
        let bytes = fs::read(path).map_err(|err| Error::SearchCatalogFile {
            path: path.clone(),
            source: err,
        })?;
        return selected
            .parse(&bytes)
            .map_err(|err| Error::SearchCatalogParse { source: err });
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
            .parse(&bytes)
            .map_err(|err| Error::CatalogConfigUnwritten { source: err }),
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
                .parse(&bytes)
                .map_err(|err| Error::CatalogConfigUnwritten { source: err })
        }
    }
}

/// `--provider` when present, including an empty name. Omitted means the default record.
fn provider_choice(args: &SearchArgs) -> ProviderChoice<'_> {
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
    Error::ProviderResolve { reason }
}

/// A cartridge refusal names the provider and does not parse or download.
fn cartridge_failure(provider: &str, err: CartridgeError) -> Error {
    let provider = provider.to_owned();
    match err {
        CartridgeError::Missing => Error::CartridgeMissing {
            provider,
            source: CartridgeError::Missing,
        },
        CartridgeError::Unknown { id } => Error::CartridgeUnknown {
            provider,
            source: CartridgeError::Unknown { id },
        },
    }
}

/// Turn a cache or download error into the command's sentence.
fn cache_failure(err: CatalogError, env_key: &str) -> Error {
    if matches!(err, CatalogError::MissingKey { .. }) {
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
fn search_config_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some("") => Err(Error::SearchConfigEmpty),
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
    if let Some(grok_home) = env::var_os("GROK_HOME") {
        if !grok_home.is_empty() {
            return Ok(PathBuf::from(grok_home).join("config.toml"));
        }
    }
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".grok").join("config.toml")),
        Some(_) => Err(Error::SearchHomeEmpty),
        None => Err(Error::SearchHomeUnset),
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
            return Err(Error::SearchConfigIo {
                path: path.to_path_buf(),
                source: err,
            });
        }
    };
    String::from_utf8(bytes).map_err(|_| Error::SearchConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// Default cache directory `~/.cache/grok-router`.
fn default_cache_dir() -> Result<PathBuf, Error> {
    let Some(home) = env::var_os("HOME") else {
        return Err(Error::SearchCacheHomeUnset);
    };
    if home.is_empty() {
        return Err(Error::SearchCacheHomeEmpty);
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
        Err(env::VarError::NotUnicode(_)) => Err(Error::SearchEnvKeyNotUnicode {
            name: env_key.to_owned(),
        }),
    }
}

/// `--limit` rejected by clap before search runs.
#[derive(Debug, thiserror::Error)]
enum LimitParse {
    /// The value was not a whole number.
    #[error("--limit {raw} is not a whole number. Pass --limit 30.")]
    NotWhole {
        /// The text that was passed.
        raw: String,
    },
    /// Zero would hide every row.
    #[error(
        "--limit 0 would hide every row. Pass --limit 30 or another number greater than zero."
    )]
    Zero,
}

/// Reject zero. Clap prints the returned error and exits.
fn parse_limit(raw: &str) -> Result<usize, LimitParse> {
    let limit: usize = raw.parse().map_err(|_| LimitParse::NotWhole {
        raw: raw.to_owned(),
    })?;
    if limit == 0 {
        return Err(LimitParse::Zero);
    }
    Ok(limit)
}

/// Whether `NO_COLOR` is present and non-empty.
fn no_color_is_set() -> bool {
    env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty())
}

/// Whether the table may use color. This command still prints no color codes.
///
/// Color is allowed only on a terminal, and only when `NO_COLOR` is absent or empty.
fn allow_color(stdout_is_terminal: bool, no_color_is_set: bool) -> bool {
    stdout_is_terminal && !no_color_is_set
}

/// First `limit` matches, in catalog order.
fn take_rows<'a>(rows: &[&'a CatalogRow], limit: usize) -> Vec<&'a CatalogRow> {
    let mut taken = Vec::new();
    for row in rows {
        if taken.len() == limit {
            break;
        }
        taken.push(*row);
    }
    taken
}

/// Terms as the empty-search sentence already quotes them, or `the catalog`.
fn quoted_terms(terms: &[String]) -> String {
    if terms.is_empty() {
        "the catalog".to_owned()
    } else {
        terms
            .iter()
            .map(|term| format!("{term:?}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// JSON array of id, name, and context_length. Nothing else.
fn format_json(rows: &[&CatalogRow]) -> Result<String, Error> {
    let hits = rows
        .iter()
        .map(|row| JsonHit {
            id: row.id(),
            name: row.display_name(),
            context_length: row.context_length(),
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&hits).map_err(|err| Error::JsonEncode {
        what: "search results",
        source: err,
        retry: "Retry the search; report this if it persists.",
    })
}

/// One id per line and no header.
fn format_quiet(rows: &[&CatalogRow], color: bool) -> String {
    let _ = color;
    rows.iter()
        .map(|row| row.id())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Plain table of id, name, and context length.
fn format_table(rows: &[&CatalogRow], color: bool) -> String {
    let _ = color;
    let id_width = column_width(rows, CatalogRow::id, "ID");
    let name_width = column_width(rows, CatalogRow::display_name, "NAME");
    let mut lines = Vec::with_capacity(rows.len().saturating_add(1));
    lines.push(format!(
        "{header_id:<id_width$}  {header_name:<name_width$}  CONTEXT",
        header_id = "ID",
        header_name = "NAME",
        id_width = id_width,
        name_width = name_width,
    ));
    for row in rows {
        let context = match row.context_length() {
            Some(length) => length.to_string(),
            None => "-".to_owned(),
        };
        lines.push(format!(
            "{id:<id_width$}  {name:<name_width$}  {context}",
            id = row.id(),
            name = row.display_name(),
            id_width = id_width,
            name_width = name_width,
            context = context,
        ));
    }
    lines.join("\n")
}

/// Width of a text column, at least the header.
fn column_width(rows: &[&CatalogRow], field: fn(&CatalogRow) -> &str, header: &str) -> usize {
    let mut width = header.len();
    for row in rows {
        let len = field(row).chars().count();
        if len > width {
            width = len;
        }
    }
    width
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
                what: "search results",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "search", err)
}

/// One JSON object in the search array.
#[derive(serde::Serialize)]
struct JsonHit<'a> {
    id: &'a str,
    name: &'a str,
    context_length: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::allow_color;

    #[test]
    fn color_stays_off_when_stdout_is_not_a_terminal_or_no_color_is_set() {
        assert!(
            !allow_color(false, false),
            "piped stdout must not enable color"
        );
        assert!(
            !allow_color(true, true),
            "NO_COLOR must disable color even on a terminal"
        );
        assert!(
            !allow_color(false, true),
            "NO_COLOR and a pipe must both keep color off"
        );
        assert!(
            allow_color(true, false),
            "a terminal without NO_COLOR is the only case that may use color"
        );
    }
}
