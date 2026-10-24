#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! Binary for the `grok-router` command.

use clap::Parser;
use grok_router::commands::{add, list, remove, search, set_default, skill};
use grok_router::{Cli, Commands};

/// Runs the `grok-router` binary.
fn main() {
    let cli = Cli::parse();
    std::process::exit(dispatch(&cli));
}

/// Routes one parsed invocation. No subcommand runs `list`.
fn dispatch(cli: &Cli) -> i32 {
    let config = cli.config.as_deref();
    match &cli.command {
        None => list::run(config, None),
        Some(Commands::List(args)) => list::run(config, Some(args)),
        Some(Commands::Search(args)) => search::run(config, args),
        Some(Commands::Add(args)) => add::run(config, args),
        Some(Commands::SetDefault(args)) => set_default::run(config, args),
        Some(Commands::Remove(args)) => remove::run(config, args),
        Some(Commands::Skill(args)) => skill::run(args),
        Some(Commands::Internal(args)) => skill::run_internal(args),
    }
}
