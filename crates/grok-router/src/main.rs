#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! Binary for the `grok-router` command.

use std::io::{self, Write};

use clap::error::ErrorKind;
use clap::parser::ValueSource;
use clap::{CommandFactory, FromArgMatches};
use grok_router::commands::{add, init, list, remove, search, set_default, skill};
use grok_router::{Cli, Commands, OutputMode, error};

/// Runs the `grok-router` binary.
fn main() {
    let matches = match Cli::command().try_get_matches() {
        Ok(matches) => matches,
        Err(err) => std::process::exit(clap_exit(err)),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(err) => std::process::exit(clap_exit(err)),
    };
    let mode = match cli.resolve_output(output_source(&matches)) {
        Ok(mode) => mode,
        Err(err) => {
            let mode = if argv_requests_json() {
                OutputMode::Json
            } else {
                OutputMode::Text
            };
            std::process::exit(grok_router::fail_message(mode, cli.command_name(), &err));
        }
    };
    std::process::exit(dispatch(&cli, mode));
}

/// Routes one parsed invocation. No subcommand runs `list`.
fn dispatch(cli: &Cli, mode: OutputMode) -> i32 {
    let config = cli.config.as_deref();
    match &cli.command {
        None => list::run(config, cli.limit.as_deref(), mode),
        Some(Commands::List(args)) => {
            list::run(config, args.limit().or(cli.limit.as_deref()), mode)
        }
        Some(Commands::Search(args)) => search::run(config, args, mode),
        Some(Commands::Add(args)) => add::run(config, args, mode),
        Some(Commands::SetDefault(args)) => set_default::run(config, args, mode),
        Some(Commands::Remove(args)) => remove::run(config, args, mode),
        Some(Commands::Init(args)) => init::run(config, args, mode),
        Some(Commands::Skill(args)) => skill::run(args, mode),
        Some(Commands::Completions(args)) => args.run(mode),
        Some(Commands::Internal(args)) => skill::run_internal(args, mode),
    }
}

/// Where `--output` was set. A command-line value outranks the environment and the default.
fn output_source(matches: &clap::ArgMatches) -> ValueSource {
    let mut best = ValueSource::DefaultValue;
    consider_output(matches, &mut best);
    best
}

/// Walk root and subcommand matches. Keep the strongest `--output` source.
fn consider_output(matches: &clap::ArgMatches, best: &mut ValueSource) {
    if let Some(source) = matches.value_source("output") {
        if source_rank(source) > source_rank(*best) {
            *best = source;
        }
    }
    if let Some((_, child)) = matches.subcommand() {
        consider_output(child, best);
    }
}

/// Command line, then environment, then the default.
fn source_rank(source: ValueSource) -> u8 {
    match source {
        ValueSource::CommandLine => 3,
        ValueSource::EnvVariable => 2,
        _ => 1,
    }
}

/// Print clap's error, or one JSON value when this invocation asked for JSON.
fn clap_exit(err: clap::Error) -> i32 {
    if argv_requests_json() {
        return json_clap_exit(err);
    }
    let code = err.exit_code();
    match err.print() {
        Ok(()) => code,
        Err(print_err) => {
            eprintln!("could not print the command error: {print_err}");
            code
        }
    }
}

/// Write one line to stdout. A closed pipe is success and returns 0.
fn write_stdout_line(body: &str) -> i32 {
    match writeln!(io::stdout(), "{body}") {
        Ok(()) => 0,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(err) => {
            eprintln!("could not write stdout: {err}");
            1
        }
    }
}

/// Help and version are JSON on stdout. Every other clap error is one JSON object on stderr.
fn json_clap_exit(err: clap::Error) -> i32 {
    match err.kind() {
        ErrorKind::DisplayHelp => {
            let body = serde_json::json!({"command": "help", "format": "json"});
            write_stdout_line(&body.to_string())
        }
        ErrorKind::DisplayVersion => {
            let body = serde_json::json!({"version": env!("CARGO_PKG_VERSION")});
            write_stdout_line(&body.to_string())
        }
        _ => {
            let code = err.exit_code();
            let usage = error::Error::Usage { source: err };
            grok_router::write_failure(OutputMode::Json, usage_command_name(), &usage);
            code
        }
    }
}

/// True when this argv, or `GROK_ROUTER_OUTPUT` if no `--output` was passed, asks for JSON.
fn argv_requests_json() -> bool {
    let mut args = std::env::args_os().skip(1);
    let mut saw_output = false;
    let mut output_is_json = false;
    let mut saw_json_flag = false;
    while let Some(arg) = args.next() {
        let Some(text) = arg.to_str() else {
            continue;
        };
        if let Some(value) = text.strip_prefix("--output=") {
            saw_output = true;
            output_is_json = is_json_format(value);
            continue;
        }
        if text == "--output" {
            saw_output = true;
            output_is_json = args
                .next()
                .as_ref()
                .and_then(|value| value.to_str())
                .is_some_and(is_json_format);
            continue;
        }
        if text == "--json" || text == "--jsonl" {
            saw_json_flag = true;
        }
    }
    if saw_output {
        return output_is_json;
    }
    if saw_json_flag {
        return true;
    }
    std::env::var("GROK_ROUTER_OUTPUT")
        .ok()
        .is_some_and(|value| is_json_format(&value))
}

/// Top-level subcommand named in a clap usage error.
///
/// A failure before any of those names uses `grok-router`. `skill` stays `skill`.
fn usage_command_name() -> &'static str {
    let args: Vec<String> = std::env::args().skip(1).map(|arg| arg).collect();
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            index = index.saturating_add(1);
            if takes_value(arg) {
                index = index.saturating_add(1);
            }
            continue;
        }
        return match arg {
            "list" => "list",
            "search" => "search",
            "add" => "add",
            "set-default" => "set-default",
            "remove" => "remove",
            "init" => "init",
            "completions" => "completions",
            "skill" | "__internal" => "skill",
            _ => "grok-router",
        };
    }
    "grok-router"
}

/// Whether this argv token consumes the following token as its value.
fn takes_value(arg: &str) -> bool {
    if arg.contains('=') || arg == "--" {
        return false;
    }
    let name = arg.strip_prefix("--").or_else(|| {
        let short = arg.strip_prefix('-')?;
        if short.len() == 1 { Some(short) } else { None }
    });
    matches!(
        name,
        Some(
            "config"
                | "output"
                | "timeout"
                | "limit"
                | "skill-root"
                | "cache-dir"
                | "catalog-file"
                | "provider"
                | "as"
                | "model"
                | "base-url"
                | "env-key"
                | "api-backend"
                | "name"
                | "context-window"
        )
    )
}

/// `json` and `jsonl`, ignoring ASCII case, are the machine-readable formats.
fn is_json_format(value: &str) -> bool {
    let value = value.trim();
    value.eq_ignore_ascii_case("json") || value.eq_ignore_ascii_case("jsonl")
}
