//! `show` subcommand.
//!
//! Show reads one model block and prints it. It does not modify the config.
//! A built-in with no block prints the slug and no body.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use clap::Args;

use crate::OutputMode;
use crate::config_edit::{self, ShownModel};
use crate::error::Error;

/// Show one configured model, or a built-in that has no block. This command does not modify the config.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router show kimi-k3\n  grok-router show --config config.toml kimi-k3"
)]
pub struct ShowArgs {
    /// Configured or built-in slug to print.
    #[arg(value_name = "SLUG", verbatim_doc_comment)]
    slug: String,
}

/// Runs `show`.
///
/// Returns `0` when the model was printed and `1` when it could not be read.
#[must_use]
pub fn run(config: Option<&str>, args: &ShowArgs, mode: OutputMode) -> i32 {
    match show_model(config, args, mode) {
        Ok(body) => write_stdout(mode, &body),
        Err(err) => fail(mode, &err),
    }
}

/// Read the config and render one model. Does not write the file.
///
/// # Errors
///
/// Returns an error when the path, slug, or config text cannot be used.
fn show_model(config: Option<&str>, args: &ShowArgs, mode: OutputMode) -> Result<String, Error> {
    let path = config_path(config)?;
    let slug = requested_slug(&args.slug)?;
    let text = read_config_text(&path)?;
    let shown =
        config_edit::show_model(&text, &slug).map_err(|refused| Error::ShowConfigRefused {
            reason: refused.reason,
        })?;
    render(mode, &shown)
}

/// `--config` when it was passed. Otherwise `${GROK_HOME:-$HOME/.grok}/config.toml`.
///
/// # Errors
///
/// Returns an error when `--config` is empty or the home path cannot be built.
fn config_path(config: Option<&str>) -> Result<PathBuf, Error> {
    match config {
        Some("") => Err(Error::ShowConfigEmpty),
        Some(path) => Ok(PathBuf::from(path)),
        None => default_config_path(),
    }
}

/// `${GROK_HOME:-$HOME/.grok}/config.toml`. An empty `GROK_HOME` falls through to `HOME`.
///
/// # Errors
///
/// Returns an error when `GROK_HOME` is empty and `HOME` is unset, or `HOME` itself is unset.
fn default_config_path() -> Result<PathBuf, Error> {
    if let Some(grok_home) = env::var_os("GROK_HOME")
        && !grok_home.is_empty()
    {
        return Ok(PathBuf::from(grok_home).join("config.toml"));
    }
    match env::var_os("HOME") {
        Some(home) if !home.is_empty() => Ok(PathBuf::from(home).join(".grok").join("config.toml")),
        Some(_) => Err(Error::ShowHomeEmpty),
        None => Err(Error::ShowHomeUnset),
    }
}

/// The required slug. An empty string is a usage failure.
///
/// # Errors
///
/// Returns an error when the slug is empty.
fn requested_slug(slug: &str) -> Result<String, Error> {
    if slug.is_empty() {
        Err(Error::ShowSlugEmpty)
    } else {
        Ok(slug.to_owned())
    }
}

/// Read the named config. A missing file is a failure and not a reason to look elsewhere.
///
/// # Errors
///
/// Returns an error when the file is missing, unreadable, or not UTF-8.
fn read_config_text(path: &Path) -> Result<String, Error> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return Err(Error::ShowConfigMissing {
                path: path.to_path_buf(),
            });
        }
        Err(err) => {
            return Err(Error::ShowConfigIo {
                path: path.to_path_buf(),
                source: err,
            });
        }
    };
    String::from_utf8(bytes).map_err(|_| Error::ShowConfigUtf8 {
        path: path.to_path_buf(),
    })
}

/// Human text, the slug, or one JSON object.
///
/// # Errors
///
/// Returns [`Error::JsonEncode`] when the model cannot be encoded.
fn render(mode: OutputMode, shown: &ShownModel) -> Result<String, Error> {
    if mode == OutputMode::Json {
        return serde_json::to_string(shown).map_err(|err| Error::JsonEncode {
            what: "the shown model",
            source: err,
            retry: "Retry show; report this if it persists.",
        });
    }
    if mode == OutputMode::Quiet {
        return Ok(shown.slug.clone());
    }
    Ok(status_text(shown))
}

/// The slug, whether it is a built-in or the saved default, then the stored block.
fn status_text(shown: &ShownModel) -> String {
    let mut line = shown.slug.clone();
    if shown.builtin {
        line.push_str("  built-in");
    }
    if shown.is_default {
        line.push_str("  default");
    }
    if shown.body.is_empty() {
        return line;
    }
    format!("{line}\n{}", shown.body)
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
                what: "the shown model",
                source: err,
            },
        ),
    }
}

/// Print a failure to stderr and return the failure status.
fn fail(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "show", err)
}
