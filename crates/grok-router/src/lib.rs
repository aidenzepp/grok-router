#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]
#![allow(
    clippy::multiple_crate_versions,
    reason = "windows-sys 0.52 and 0.61 are transitive dependencies this crate does not select"
)]
#![allow(clippy::cargo_common_metadata, reason = "no license has been chosen")]

//! Library for the `grok-router` command.

use std::io::{self, Write};

use clap::{ArgAction, Args, Command, CommandFactory, Parser, Subcommand, ValueEnum};

pub mod catalog;
pub mod commands;
pub mod config_edit;
pub mod error;
pub mod skill;

/// Format named by `--output`. `Json` and `Jsonl` render the same one JSON value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human text.
    Text,
    /// One JSON value.
    Json,
    /// One JSON value, the same bytes as [`OutputFormat::Json`].
    Jsonl,
}

/// How a command renders after `--output`, `--json`, `--jsonl`, and `--quiet` resolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    /// Human text.
    Text,
    /// One answer line and no extra labels.
    Quiet,
    /// One JSON value. `json` and `jsonl` both use this mode.
    Json,
}

/// Root command for `grok-router`.
///
/// No subcommand runs `list`.
#[derive(Debug, Parser)]
#[command(
    name = "grok-router",
    version,
    disable_help_subcommand = true,
    about = "Manage the grok model router config.",
    long_about = "Manage the grok model router config.\n\nRead models, search a provider catalog, and edit the saved config.",
    after_help = "Examples:\n  grok-router list --config config.toml\n  grok-router list --config config.toml --output json"
)]
pub struct Cli {
    /// Reads default to `$GROK_HOME/config.toml` or ~/.grok/config.toml.
    /// init writes that file when --config is omitted.
    /// add, set-default, and remove still require --config.
    #[arg(long, global = true, value_name = "PATH", verbatim_doc_comment)]
    pub config: Option<String>,

    /// Rows for a bare invocation, which runs list. Not global. list --limit is the same flag.
    ///
    /// Omitting it lists every row.
    #[arg(long, value_name = "N", hide = true, verbatim_doc_comment)]
    pub limit: Option<String>,

    /// text, json, or jsonl. json and jsonl print the same one JSON value.
    #[arg(
        long = "output",
        global = true,
        env = "GROK_ROUTER_OUTPUT",
        value_enum,
        default_value_t = OutputFormat::Text,
        verbatim_doc_comment
    )]
    pub output: OutputFormat,

    /// Print one JSON value and nothing else.
    #[arg(long = "json", global = true, action = ArgAction::SetTrue, verbatim_doc_comment)]
    pub json: bool,

    /// Print one JSON value and nothing else.
    #[arg(long = "jsonl", global = true, action = ArgAction::SetTrue, verbatim_doc_comment)]
    pub jsonl: bool,

    /// Print only the answer line.
    #[arg(
        long = "quiet",
        global = true,
        env = "GROK_ROUTER_QUIET",
        action = ArgAction::SetTrue,
        verbatim_doc_comment
    )]
    pub quiet: bool,

    /// Selected subcommand. Absent means `list`.
    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    /// Resolve output after clap has recorded where `--output` came from.
    ///
    /// A command-line `--output` wins over `GROK_ROUTER_OUTPUT`. `--json` and
    /// `--jsonl` then win over the environment and over the default. `--json`
    /// with `--output text` is an error. `--quiet` with either JSON value is an error.
    ///
    /// # Errors
    ///
    /// Returns the refusal when the flags disagree. Nothing was written.
    pub fn resolve_output(
        &self,
        source: clap::parser::ValueSource,
    ) -> Result<OutputMode, error::Error> {
        let json_alias = self.json || self.jsonl;
        let format = if source == clap::parser::ValueSource::CommandLine {
            if json_alias && self.output == OutputFormat::Text {
                let flag = if self.json { "--json" } else { "--jsonl" };
                return Err(error::Error::JsonFlagConflicts { flag });
            }
            self.output
        } else if json_alias {
            OutputFormat::Json
        } else {
            self.output
        };
        if self.quiet && matches!(format, OutputFormat::Json | OutputFormat::Jsonl) {
            return Err(error::Error::QuietConflictsJson);
        }
        Ok(match (self.quiet, format) {
            (true, _) => OutputMode::Quiet,
            (_, OutputFormat::Json | OutputFormat::Jsonl) => OutputMode::Json,
            (_, OutputFormat::Text) => OutputMode::Text,
        })
    }

    /// Subcommand name for a failure, including bare invocation as `list`.
    ///
    /// Skill and `__internal` report `skill`.
    #[must_use]
    pub fn command_name(&self) -> &'static str {
        match &self.command {
            None | Some(Commands::List(_)) => "list",
            Some(Commands::Search(_)) => "search",
            Some(Commands::Add(_)) => "add",
            Some(Commands::SetDefault(_)) => "set-default",
            Some(Commands::Remove(_)) => "remove",
            Some(Commands::Init(_)) => "init",
            Some(Commands::Completions(_)) => "completions",
            Some(Commands::Skill(args)) => args.command_name(),
            Some(Commands::Internal(_)) => "skill",
        }
    }
}

/// Subcommands. Each variant names the args struct that owns that command's flags.
#[derive(Debug, Subcommand)]
#[command(about = None, long_about = None)]
pub enum Commands {
    /// List configured models and the saved default. This command does not modify the config.
    #[command(verbatim_doc_comment)]
    List(commands::list::ListArgs),
    /// Search the selected provider catalog. This command does not modify the config.
    #[command(verbatim_doc_comment)]
    Search(commands::search::SearchArgs),
    /// Add one model from the selected provider record, or write one block from --model, --base-url, --env-key, and --api-backend without resolving a provider. This command writes unless --dry-run is set.
    #[command(verbatim_doc_comment)]
    Add(commands::add::AddArgs),
    /// Set [models].default for the next session. This does not switch the running session. This command writes the saved default unless --dry-run is set.
    #[command(verbatim_doc_comment)]
    SetDefault(commands::set_default::SetDefaultArgs),
    /// Delete one model block from the config. This command deletes a model block.
    #[command(verbatim_doc_comment)]
    Remove(commands::remove::RemoveArgs),
    /// Write a default `OpenRouter` [router] table when the file has none. An existing [router] table is left unchanged. This command writes unless --dry-run is set.
    #[command(verbatim_doc_comment)]
    Init(commands::init::InitArgs),
    /// Install, check, or remove the skill stub, or print its guide. setup and remove write; status and show do not.
    #[command(verbatim_doc_comment)]
    Skill(commands::skill::SkillArgs),
    /// Print a shell completion script. This command does not read a config and does not use the network.
    #[command(verbatim_doc_comment)]
    Completions(CompletionsArgs),
    /// Print the built-in skill guide. Not a human command. This command does not modify anything.
    #[command(name = "__internal", hide = true, verbatim_doc_comment)]
    Internal(commands::skill::InternalArgs),
}

/// Print a shell completion script. This command does not read a config and does not use the network.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router completions bash\n  grok-router completions bash --output json"
)]
pub struct CompletionsArgs {
    /// bash, zsh, fish, elvish, or powershell.
    #[arg(value_name = "SHELL", verbatim_doc_comment)]
    shell: String,
}

impl CompletionsArgs {
    /// Print the completion script. Does not read a config and does not use the network.
    #[must_use]
    pub fn run(&self, mode: OutputMode) -> i32 {
        match render_completion(&self.shell, mode) {
            Ok(body) => write_stdout(mode, &body, "the completion script", "completions"),
            Err(err) => fail_message(mode, "completions", &err),
        }
    }
}

/// Builds the root command named `grok-router`.
#[must_use]
pub fn command() -> Command {
    Cli::command()
}

/// Print `err` to stderr and return `1`.
///
/// Text and quiet print the sentence. JSON prints one object with `command`,
/// `format`, `error`, `kind`, and `message`. `format` is `json` for both JSON modes.
#[must_use]
pub fn fail_message(mode: OutputMode, command: &str, err: &error::Error) -> i32 {
    write_failure(mode, command, err);
    1
}

/// Print `err` without choosing the exit code.
///
/// Clap usage keeps clap's exit code and uses this for the JSON body.
pub fn write_failure(mode: OutputMode, command: &str, err: &error::Error) {
    match mode {
        OutputMode::Json => {
            let kind = err.kind();
            let body = serde_json::json!({
                "command": command,
                "format": "json",
                "error": kind,
                "kind": kind,
                "message": err.to_string(),
            });
            eprintln!("{body}");
        }
        OutputMode::Text | OutputMode::Quiet => {
            eprintln!("{err}");
        }
    }
}

/// Completion script for one shell, or the refusal that names the five shells.
///
/// # Errors
///
/// Returns the refusal when `shell` is not one of the five names. The config was not read.
fn render_completion(shell: &str, mode: OutputMode) -> Result<String, error::Error> {
    let script = completion_script(shell)?;
    match mode {
        OutputMode::Json => {
            serde_json::to_string(&script).map_err(|err| error::Error::JsonEncode {
                what: "the completion script",
                source: err,
                retry: "Retry completions.",
            })
        }
        OutputMode::Text | OutputMode::Quiet => Ok(script),
    }
}

/// Raw completion script. Does not read a config and does not use the network.
///
/// # Errors
///
/// Returns the refusal when `shell` is not bash, zsh, fish, elvish, or powershell.
fn completion_script(shell: &str) -> Result<String, error::Error> {
    let clap_shell = match shell {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "elvish" => clap_complete::Shell::Elvish,
        "powershell" => clap_complete::Shell::PowerShell,
        _ => {
            return Err(error::Error::UnknownShell {
                shell: shell.to_owned(),
            });
        }
    };
    let mut cmd = command();
    let mut buf = Vec::new();
    clap_complete::generate(clap_shell, &mut cmd, "grok-router", &mut buf);
    String::from_utf8(buf).map_err(|_| error::Error::CompletionNotUtf8)
}

/// Write `body` and a trailing newline. A closed pipe is success.
fn write_stdout(mode: OutputMode, body: &str, what: &'static str, command: &str) -> i32 {
    let mut stdout = io::stdout().lock();
    let mut bytes = body.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    match stdout.write_all(&bytes) {
        Ok(()) => 0,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(err) => fail_message(mode, command, &error::Error::Stdout { what, source: err }),
    }
}
