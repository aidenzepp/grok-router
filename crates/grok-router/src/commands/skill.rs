//! `skill` and `__internal` commands.
//!
//! `skill setup` and `skill remove` write only the stub under `--skill-root`.
//! `skill show` and `__internal skill` print the same guide. Neither command
//! reads the home directory, and neither installs a hook or `settings.json`.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::error::Error;
use crate::skill::{
    Confirmation, Effect, Presence, Report, SkillFailure, WriteMode, guide, inspect, remove, setup,
};
use crate::OutputMode;

/// Install, check, or remove the skill stub, or print its guide. setup and remove write; status and show do not.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    subcommand_required = true,
    arg_required_else_help = true,
    after_help = "Examples:\n  grok-router skill setup --skill-root /tmp/skills\n  grok-router skill status --skill-root /tmp/skills\n  grok-router skill show\n  grok-router skill remove --yes --skill-root /tmp/skills"
)]
pub struct SkillArgs {
    /// Selected skill command.
    #[command(subcommand)]
    command: SkillCommand,
}

/// `skill` subcommands.
#[derive(Debug, Subcommand)]
enum SkillCommand {
    /// Write the grok-router skill stub under --skill-root. This command writes unless --dry-run is set.
    #[command(verbatim_doc_comment)]
    Setup(SetupArgs),
    /// Report whether the skill stub is current. This command does not modify anything.
    #[command(verbatim_doc_comment)]
    Status(StatusArgs),
    /// Delete the grok-router stub directory when it carries the marker. This command writes unless --dry-run is set. Refuses without --yes.
    #[command(verbatim_doc_comment)]
    Remove(RemoveArgs),
    /// Print the skill guide. This command does not modify anything.
    #[command(verbatim_doc_comment)]
    Show(ShowArgs),
}

/// Write the grok-router skill stub under --skill-root. This command writes unless --dry-run is set.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router skill setup --skill-root /tmp/skills\n  grok-router skill setup --skill-root /tmp/skills --dry-run --json"
)]
struct SetupArgs {
    /// Flags shared by the writing skill commands.
    #[command(flatten)]
    flags: WriteFlags,
}

/// Report whether the skill stub is current. This command does not modify anything.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router skill status --skill-root /tmp/skills\n  grok-router skill status --skill-root /tmp/skills --json"
)]
struct StatusArgs {
    /// Flags shared by the reading skill commands.
    #[command(flatten)]
    flags: ReadFlags,
}

/// Delete the grok-router stub directory when it carries the marker. This command writes unless --dry-run is set. Refuses without --yes.
#[derive(Debug, Args)]
#[command(
    verbatim_doc_comment,
    after_help = "Examples:\n  grok-router skill remove --skill-root /tmp/skills --dry-run --yes\n  grok-router skill remove --skill-root /tmp/skills --yes --json"
)]
struct RemoveArgs {
    /// Flags shared by the writing skill commands.
    #[command(flatten)]
    flags: WriteFlags,

    /// Required to delete. Without it, the command refuses and writes nothing. Does not prompt.
    #[arg(long, alias = "force", verbatim_doc_comment)]
    yes: bool,
}

/// Print the skill guide. This command does not modify anything.
#[derive(Debug, Args)]
#[command(verbatim_doc_comment, after_help = "Examples:\n  grok-router skill show")]
struct ShowArgs {}

/// Print the built-in skill guide. Not a human command. This command does not modify anything.
#[derive(Debug, Args)]
#[command(
    hide = true,
    verbatim_doc_comment,
    subcommand_required = true,
    arg_required_else_help = true,
    after_help = "Examples:\n  grok-router __internal skill"
)]
pub struct InternalArgs {
    /// Selected hidden command.
    #[command(subcommand)]
    command: InternalCommand,
}

/// Hidden commands under `__internal`.
#[derive(Debug, Subcommand)]
enum InternalCommand {
    /// Print the skill guide. This command does not modify anything.
    #[command(
        verbatim_doc_comment,
        after_help = "Examples:\n  grok-router __internal skill"
    )]
    Skill,
}

/// Flags for `skill status` and the read half of setup and remove.
#[derive(Debug, Args)]
struct ReadFlags {
    /// Directory that will contain the grok-router stub. Required. Does not read or write the home directory.
    #[arg(long, value_name = "DIR", verbatim_doc_comment)]
    skill_root: Option<PathBuf>,
}

/// Flags for `skill setup` and `skill remove`, including [`ReadFlags`].
#[derive(Debug, Args)]
struct WriteFlags {
    /// Read flags, plus `--dry-run`.
    #[command(flatten)]
    read: ReadFlags,

    /// Validate and report the change, then do not write.
    #[arg(long, verbatim_doc_comment)]
    dry_run: bool,
}

impl SkillArgs {
    /// Top-level command for a failure. Every skill action reports `skill`.
    #[must_use]
    pub const fn command_name(&self) -> &'static str {
        match self.command {
            SkillCommand::Setup(_)
            | SkillCommand::Status(_)
            | SkillCommand::Remove(_)
            | SkillCommand::Show(_) => "skill",
        }
    }
}

/// Runs `skill`.
///
/// Returns `0` when the command finished and `1` when it refused or failed.
#[must_use]
pub fn run(args: &SkillArgs, mode: OutputMode) -> i32 {
    match &args.command {
        SkillCommand::Setup(setup) => run_setup(setup, mode),
        SkillCommand::Status(status) => run_status(status, mode),
        SkillCommand::Remove(remove_args) => run_remove(remove_args, mode),
        SkillCommand::Show(_) => write_guide(mode),
    }
}

/// Runs `__internal`.
///
/// Returns `0` when the guide was printed.
#[must_use]
pub fn run_internal(args: &InternalArgs, mode: OutputMode) -> i32 {
    match args.command {
        InternalCommand::Skill => write_guide(mode),
    }
}

fn run_setup(args: &SetupArgs, mode: OutputMode) -> i32 {
    let flags = &args.flags.read;
    let root = match skill_root(flags) {
        Ok(root) => root,
        Err(err) => return fail_command(mode, &err),
    };
    finish(mode, "setup", setup(&root, write_mode(args.flags.dry_run)))
}

fn run_status(args: &StatusArgs, mode: OutputMode) -> i32 {
    let root = match skill_root(&args.flags) {
        Ok(root) => root,
        Err(err) => return fail_command(mode, &err),
    };
    finish(mode, "status", inspect(&root))
}

fn run_remove(args: &RemoveArgs, mode: OutputMode) -> i32 {
    let flags = &args.flags.read;
    let confirmation = confirmation(args.yes);
    if confirmation == Confirmation::No {
        return fail_command(mode, &Error::from(SkillFailure::NeedsYes));
    }
    let root = match skill_root(flags) {
        Ok(root) => root,
        Err(err) => return fail_command(mode, &err),
    };
    finish(
        mode,
        "remove",
        remove(&root, confirmation, write_mode(args.flags.dry_run)),
    )
}

/// `skill show` and `__internal skill` print the guide. JSON modes print that text as one JSON string.
fn write_guide(mode: OutputMode) -> i32 {
    let body = match mode {
        OutputMode::Json => match serde_json::to_string(guide()) {
            Ok(json) => json,
            Err(err) => {
                return fail_command(
                    mode,
                    &Error::JsonEncode {
                        what: "the skill guide",
                        source: err,
                        retry: "Retry the command.",
                    },
                );
            }
        },
        OutputMode::Text | OutputMode::Quiet => guide().to_owned(),
    };
    write_stdout(mode, &body)
}

fn finish(mode: OutputMode, command: &str, result: Result<Report, SkillFailure>) -> i32 {
    match result {
        Ok(report) => match render(command, &report, mode) {
            Ok(body) => write_stdout(mode, &body),
            Err(err) => fail_command(mode, &err),
        },
        Err(err) => fail_command(mode, &Error::from(err)),
    }
}

fn skill_root(flags: &ReadFlags) -> Result<PathBuf, Error> {
    let Some(path) = &flags.skill_root else {
        return Err(Error::SkillRootOmitted);
    };
    if path.as_os_str().is_empty() {
        return Err(Error::SkillRootEmpty);
    }
    Ok(path.clone())
}

fn write_mode(dry_run: bool) -> WriteMode {
    if dry_run {
        WriteMode::DryRun
    } else {
        WriteMode::Apply
    }
}

fn confirmation(yes: bool) -> Confirmation {
    if yes {
        Confirmation::Yes
    } else {
        Confirmation::No
    }
}

fn render(command: &str, report: &Report, mode: OutputMode) -> Result<String, Error> {
    match mode {
        OutputMode::Json => render_json(command, report),
        OutputMode::Quiet => Ok(render_quiet(command, report)),
        OutputMode::Text => Ok(render_text(command, report)),
    }
}

fn render_json(command: &str, report: &Report) -> Result<String, Error> {
    let body = JsonReport {
        command,
        status: report.disk.as_str(),
        effect: report.effect.as_str(),
        dry_run: report.mode.is_dry_run(),
        path: report.path.display().to_string(),
    };
    serde_json::to_string(&body).map_err(|err| Error::JsonEncode {
        what: "the skill result",
        source: err,
        retry: "Retry the command; report this if it persists.",
    })
}

fn render_quiet(command: &str, report: &Report) -> String {
    match (command, report.effect) {
        ("status", _) => report.disk.as_str().to_owned(),
        ("remove", Effect::Removed | Effect::WouldRemove) => report.path.display().to_string(),
        ("remove", Effect::Left) => "left".to_owned(),
        ("remove", Effect::Unchanged) => "missing".to_owned(),
        _ => report.path.display().to_string(),
    }
}

fn render_text(command: &str, report: &Report) -> String {
    let path = report.path.display().to_string();
    match command {
        "status" => match report.disk {
            Presence::Current => format!("current\n{path}"),
            Presence::Missing => format!("missing\n{path}"),
            Presence::Stale => format!("stale\n{path}"),
            Presence::Foreign => format!(
                "foreign\n{path} lacks the grok-router-skill-stub marker, so it was left in place"
            ),
        },
        "setup" => match report.effect {
            Effect::Wrote => format!("current\nwrote {path}"),
            Effect::WouldWrite => format!("dry-run\nwould write {path}"),
            Effect::Unchanged if report.mode.is_dry_run() => {
                format!("dry-run\nwould write nothing\ncurrent\n{path}")
            }
            Effect::Unchanged => format!("current\nleft {path} unchanged"),
            Effect::Removed | Effect::WouldRemove | Effect::Left => {
                format!("{}\n{path}", report.disk.as_str())
            }
        },
        _ => match report.effect {
            Effect::Removed => format!("removed\n{path}"),
            Effect::WouldRemove => format!("dry-run\nwould remove {path}"),
            Effect::Left => format!(
                "left\n{path} lacks the grok-router-skill-stub marker, so it was not removed"
            ),
            Effect::Unchanged => format!("missing\nnothing to remove at {path}"),
            Effect::Wrote | Effect::WouldWrite => format!("{}\n{path}", report.disk.as_str()),
        },
    }
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
        Err(err) => fail_command(
            mode,
            &Error::Stdout {
                what: "the skill result",
                source: err,
            },
        ),
    }
}

fn fail_command(mode: OutputMode, err: &Error) -> i32 {
    crate::fail_message(mode, "skill", err)
}

/// One JSON object for setup, status, or remove.
#[derive(serde::Serialize)]
struct JsonReport<'a> {
    /// `setup`, `status`, or `remove`.
    command: &'a str,
    /// `missing`, `current`, `foreign`, or `stale`.
    status: &'a str,
    /// `unchanged`, `wrote`, `would_write`, `removed`, `would_remove`, or `left`.
    effect: &'a str,
    /// Whether the command was asked not to write.
    dry_run: bool,
    /// Stub directory, whether or not it exists.
    path: String,
}

#[cfg(test)]
mod tests {
    #[test]
    fn command_registers_skill_and_internal_but_not_session_start() {
        let cmd = crate::command();
        let skill = cmd
            .find_subcommand("skill")
            .expect("command should register skill");
        for name in ["setup", "status", "remove", "show"] {
            assert!(
                skill.find_subcommand(name).is_some(),
                "skill should register {name}"
            );
        }
        assert!(
            skill.find_subcommand("session-start").is_none(),
            "session-start was dropped and must not be registered on skill"
        );
        let internal = cmd
            .find_subcommand("__internal")
            .expect("command should register __internal");
        assert!(
            internal.find_subcommand("skill").is_some(),
            "__internal should register skill"
        );
        assert!(
            internal.find_subcommand("session-start").is_none(),
            "__internal must not register session-start"
        );
    }
}
