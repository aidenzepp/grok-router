#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! Library for the `grok-router` command.

use clap::{Command, CommandFactory, Parser, Subcommand};

pub mod catalog;
pub mod commands;
pub mod config_edit;
pub mod skill;

/// Root command for `grok-router`.
///
/// No subcommand runs `list`. The doc comment is not the help text: help stays
/// the version, the global config path, and the subcommands.
#[derive(Debug, Parser)]
#[command(
    name = "grok-router",
    version,
    disable_help_subcommand = true,
    about = None,
    long_about = None
)]
pub struct Cli {
    /// Path to the config file
    #[arg(long, global = true, value_name = "PATH", verbatim_doc_comment)]
    pub config: Option<String>,

    /// Selected subcommand. Absent means `list`.
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Subcommands. Each variant names the args struct that owns that command's flags.
///
/// The about text is cleared here so this doc does not become the root help.
#[derive(Debug, Subcommand)]
#[command(about = None, long_about = None)]
pub enum Commands {
    /// List configured models and the saved default. This command does not modify the config.
    #[command(verbatim_doc_comment)]
    List(commands::list::ListArgs),
    /// Search the OpenRouter catalog. This command does not modify the config.
    #[command(verbatim_doc_comment)]
    Search(commands::search::SearchArgs),
    /// Add one OpenRouter model to the config. This command writes unless --dry-run is set.
    #[command(verbatim_doc_comment)]
    Add(commands::add::AddArgs),
    /// Set [models].default for the next session. This does not switch the running session. This command writes the saved default unless --dry-run is set.
    #[command(verbatim_doc_comment)]
    SetDefault(commands::set_default::SetDefaultArgs),
    /// Delete one model block from the config. This command deletes a model block.
    #[command(verbatim_doc_comment)]
    Remove(commands::remove::RemoveArgs),
    /// Install, check, or remove the skill stub, or print its guide. setup and remove write; status and show do not.
    #[command(verbatim_doc_comment)]
    Skill(commands::skill::SkillArgs),
    /// Print the built-in skill guide. Not a human command. This command does not modify anything.
    #[command(name = "__internal", hide = true, verbatim_doc_comment)]
    Internal(commands::skill::InternalArgs),
}

/// Builds the root command named `grok-router`.
#[must_use]
pub fn command() -> Command {
    Cli::command()
}
