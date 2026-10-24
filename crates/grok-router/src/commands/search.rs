//! `search` subcommand.
//!
//! Search reads the OpenRouter catalog from `--catalog-file` or from a cache.
//! It does not modify the config. The only write is the catalog cache, and only
//! when the command itself downloads a new body into `--cache-dir`.

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::catalog::{self, CachedOrFetch, CatalogRow};

/// Adds the `search` subcommand and its flags.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(
        Command::new("search")
            .about("Search the OpenRouter catalog. This command does not modify the config.")
            .after_help("Examples:\n  grok-router search --catalog-file models.json glm")
            .arg(
                Arg::new("terms")
                    .value_name("TERM")
                    .action(ArgAction::Append)
                    .help("Case-insensitive term. Repeat to require every term. Each term must match the id, name, or description."),
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
                Arg::new("limit")
                    .long("limit")
                    .value_name("N")
                    .default_value("30")
                    .value_parser(parse_limit)
                    .help("Maximum rows to print."),
            )
            .arg(
                Arg::new("json")
                    .long("json")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("quiet")
                    .help("Print a JSON array of id, name, and context_length."),
            )
            .arg(
                Arg::new("quiet")
                    .long("quiet")
                    .action(ArgAction::SetTrue)
                    .conflicts_with("json")
                    .help("Print one model id per line and no header."),
            ),
    )
}

/// Runs `search` from the root matches.
///
/// Returns `0` when at least one row was printed and `1` when the search failed.
#[must_use]
pub fn run(matches: &ArgMatches) -> i32 {
    let Some(matches) = matches.subcommand_matches("search") else {
        return fail(
            "search was not selected. Run `grok-router search --help` for the flags and an example.",
        );
    };
    run_search(matches)
}

/// Collect flags, load rows, and print the matches.
fn run_search(matches: &ArgMatches) -> i32 {
    let terms = match collect_terms(matches) {
        Ok(terms) => terms,
        Err(message) => return fail(&message),
    };
    let Some(limit) = matches.get_one::<usize>("limit").copied() else {
        return fail("--limit is missing. Pass --limit 30.");
    };
    let rows = match load_rows(matches) {
        Ok(rows) => rows,
        Err(message) => return fail(&message),
    };
    let term_refs: Vec<&str> = terms.iter().map(String::as_str).collect();
    let matched = catalog::matching(&rows, &term_refs);
    if matched.is_empty() {
        return fail(&empty_message(&terms));
    }
    let shown = take_rows(&matched, limit);
    let color = allow_color(io::stdout().is_terminal(), no_color_is_set());
    let body = if matches.get_flag("json") {
        match format_json(&shown) {
            Ok(body) => body,
            Err(message) => return fail(&message),
        }
    } else if matches.get_flag("quiet") {
        format_quiet(&shown, color)
    } else {
        format_table(&shown, color)
    };
    write_stdout(&body)
}

/// Terms from the positional arguments. An empty term is a usage failure.
fn collect_terms(matches: &ArgMatches) -> Result<Vec<String>, String> {
    let Some(values) = matches.get_many::<String>("terms") else {
        return Ok(Vec::new());
    };
    let mut terms = Vec::new();
    for term in values {
        if term.is_empty() {
            return Err(
                "an empty search term was given. Remove the empty argument and pass words or an exact model id."
                    .to_owned(),
            );
        }
        terms.push(term.clone());
    }
    Ok(terms)
}

/// Load rows from `--catalog-file`, or from the cache and the network.
fn load_rows(matches: &ArgMatches) -> Result<Vec<CatalogRow>, String> {
    if let Some(path) = matches.get_one::<PathBuf>("catalog-file") {
        let bytes = fs::read(path).map_err(|err| {
            format!(
                "could not read catalog file {}. {err}. Pass a readable --catalog-file, or omit it to use the cache.",
                path.display()
            )
        })?;
        return catalog::parse_catalog(&bytes).map_err(|err| err.to_string());
    }

    let cache_dir = match matches.get_one::<PathBuf>("cache-dir") {
        Some(path) => path.clone(),
        None => default_cache_dir()?,
    };
    let api_key = live_api_key()?;
    let now = SystemTime::now();
    match catalog::prepare_cached_load(&cache_dir, now, matches.get_flag("refresh"), &api_key)
        .map_err(|err| err.to_string())?
    {
        CachedOrFetch::Cached(rows) => Ok(rows),
        CachedOrFetch::Fetch(request) => {
            let bytes = catalog::fetch_catalog_bytes(&request).map_err(|err| err.to_string())?;
            let stored = catalog::write_cache(&cache_dir, &bytes).map_err(|err| err.to_string())?;
            if stored != catalog::cache_file(&cache_dir) {
                return Err(format!(
                    "the catalog cache was written to {}, outside {}. Pass --cache-dir.",
                    stored.display(),
                    cache_dir.display()
                ));
            }
            catalog::parse_catalog(&bytes).map_err(|err| err.to_string())
        }
    }
}

/// Default cache directory `~/.cache/grok-router`.
fn default_cache_dir() -> Result<PathBuf, String> {
    let Some(home) = env::var_os("HOME") else {
        return Err(
            "HOME is not set, so the catalog cache has no default directory. Pass --cache-dir or --catalog-file."
                .to_owned(),
        );
    };
    if home.is_empty() {
        return Err(
            "HOME is empty, so the catalog cache has no default directory. Pass --cache-dir or --catalog-file."
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
            "OPENROUTER_API_KEY is not valid Unicode, so the catalog was not downloaded. Export a Unicode key or pass --catalog-file."
                .to_owned(),
        ),
    }
}

/// Reject zero. Clap prints the returned string and exits.
fn parse_limit(raw: &str) -> Result<usize, String> {
    let limit: usize = raw.parse().map_err(|_| {
        format!("--limit {raw} is not a whole number. Pass --limit 30.")
    })?;
    if limit == 0 {
        return Err(
            "--limit 0 would hide every row. Pass --limit 30 or another number greater than zero."
                .to_owned(),
        );
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

/// What failed, why, and what to do next when nothing matched.
fn empty_message(terms: &[String]) -> String {
    let quoted = if terms.is_empty() {
        "the catalog".to_owned()
    } else {
        terms
            .iter()
            .map(|term| format!("{term:?}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!(
        "no models matched {quoted}. 0 matches. The id, name, and description did not contain every term. Try fewer terms or pass --catalog-file with the catalog you meant."
    )
}

/// JSON array of id, name, and context_length. Nothing else.
fn format_json(rows: &[&CatalogRow]) -> Result<String, String> {
    let hits = rows
        .iter()
        .map(|row| JsonHit {
            id: row.id(),
            name: row.display_name(),
            context_length: row.context_length(),
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&hits).map_err(|err| {
        format!(
            "could not encode search results as JSON: {err}. Retry the search; report this if it persists."
        )
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
            "could not write search results: {err}. Retry when stdout accepts output."
        )),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
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
