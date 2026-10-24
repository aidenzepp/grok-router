#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! Library for the `grok-router` command.

use clap::{Arg, Command};

pub mod catalog;
pub mod commands;
pub mod config_edit;
pub mod skill;

/// Builds the root command named `grok-router`.
#[must_use]
pub fn command() -> Command {
    Command::new("grok-router")
        .version(env!("CARGO_PKG_VERSION"))
        .disable_help_subcommand(true)
        .arg(
            Arg::new("config")
                .long("config")
                .global(true)
                .required(false)
                .value_name("PATH")
                .help("Path to the config file"),
        )
}
