//! `skill` and `__internal` commands.
//!
//! `skill setup` and `skill remove` write only the stub under `--skill-root`.
//! `skill show` and `__internal skill` print the same guide. Neither command
//! reads the home directory, and neither installs a hook or `settings.json`.

use std::io::{self, Write};
use std::path::PathBuf;

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::skill::{
    Confirmation, Effect, Presence, Report, SkillFailure, WriteMode, guide, inspect, remove, setup,
};

/// Adds the `skill` command and the hidden `__internal` command.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(skill_command()).subcommand(internal_command())
}

/// Runs `skill` or `__internal` from the root matches.
///
/// Returns `0` when the command finished and `1` when it refused or failed.
#[must_use]
pub fn run(matches: &ArgMatches) -> i32 {
    match matches.subcommand_name() {
        Some("skill") => run_skill(matches),
        Some("__internal") => run_internal(matches),
        _ => fail_text(
            "skill was not selected. Run `grok-router skill --help` for the flags and an example.",
        ),
    }
}

fn skill_command() -> Command {
    Command::new("skill")
        .about("Install, check, or remove the skill stub, or print its guide. setup and remove write; status and show do not.")
        .after_help(
            "Examples:\n  grok-router skill setup --skill-root /tmp/skills\n  grok-router skill status --skill-root /tmp/skills\n  grok-router skill show\n  grok-router skill remove --yes --skill-root /tmp/skills",
        )
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(setup_command())
        .subcommand(status_command())
        .subcommand(remove_command())
        .subcommand(show_command())
}

fn setup_command() -> Command {
    with_write_flags(
        Command::new("setup")
            .about("Write the grok-router skill stub under --skill-root. This command writes unless --dry-run is set.")
            .after_help(
                "Examples:\n  grok-router skill setup --skill-root /tmp/skills\n  grok-router skill setup --skill-root /tmp/skills --dry-run --json",
            ),
    )
}

fn status_command() -> Command {
    with_read_flags(
        Command::new("status")
            .about("Report whether the skill stub is current. This command does not modify anything.")
            .after_help(
                "Examples:\n  grok-router skill status --skill-root /tmp/skills\n  grok-router skill status --skill-root /tmp/skills --json",
            ),
    )
}

fn remove_command() -> Command {
    with_write_flags(
        Command::new("remove")
            .about("Delete the grok-router stub directory when it carries the marker. This command writes unless --dry-run is set. Refuses without --yes.")
            .after_help(
                "Examples:\n  grok-router skill remove --skill-root /tmp/skills --dry-run --yes\n  grok-router skill remove --skill-root /tmp/skills --yes --json",
            ),
    )
    .arg(yes_arg())
}

fn show_command() -> Command {
    Command::new("show")
        .about("Print the skill guide. This command does not modify anything.")
        .after_help("Examples:\n  grok-router skill show")
}

fn internal_command() -> Command {
    Command::new("__internal")
        .hide(true)
        .about("Print the built-in skill guide. Not a human command. This command does not modify anything.")
        .after_help("Examples:\n  grok-router __internal skill")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("skill")
                .about("Print the skill guide. This command does not modify anything.")
                .after_help("Examples:\n  grok-router __internal skill"),
        )
}

fn with_read_flags(cmd: Command) -> Command {
    cmd.arg(skill_root_arg())
        .arg(output_arg())
        .arg(json_arg())
        .arg(quiet_arg())
}

fn with_write_flags(cmd: Command) -> Command {
    with_read_flags(cmd).arg(dry_run_arg())
}

fn skill_root_arg() -> Arg {
    Arg::new("skill-root")
        .long("skill-root")
        .value_name("DIR")
        .value_parser(clap::value_parser!(PathBuf))
        .help("Directory that will contain the grok-router stub. Required. Does not read or write the home directory.")
}

fn dry_run_arg() -> Arg {
    Arg::new("dry-run")
        .long("dry-run")
        .action(ArgAction::SetTrue)
        .help("Validate and report the change, then do not write.")
}

fn json_arg() -> Arg {
    Arg::new("json")
        .long("json")
        .action(ArgAction::SetTrue)
        .conflicts_with("quiet")
        .conflicts_with("output")
        .help("Print one JSON object and nothing else. Same as --output json.")
}

fn quiet_arg() -> Arg {
    Arg::new("quiet")
        .long("quiet")
        .action(ArgAction::SetTrue)
        .conflicts_with("json")
        .conflicts_with("output")
        .help("Print only the answer line.")
}

fn output_arg() -> Arg {
    Arg::new("output")
        .long("output")
        .value_name("FORMAT")
        .value_parser(["text", "json", "jsonl"])
        .conflicts_with("json")
        .conflicts_with("quiet")
        .help("text, json, or jsonl. json and jsonl print the same one JSON object. --json selects json.")
}

fn yes_arg() -> Arg {
    Arg::new("yes")
        .long("yes")
        .alias("force")
        .action(ArgAction::SetTrue)
        .help("Required to delete. Without it, the command refuses and writes nothing. Does not prompt.")
}

fn run_skill(root: &ArgMatches) -> i32 {
    let Some(skill) = root.subcommand_matches("skill") else {
        return fail_text(
            "skill was not selected. Run `grok-router skill --help` for the flags and an example.",
        );
    };
    match skill.subcommand_name() {
        Some("setup") => run_setup(skill),
        Some("status") => run_status(skill),
        Some("remove") => run_remove(skill),
        Some("show") => write_stdout(guide()),
        Some(other) => fail_text(&format!(
            "unknown skill command {other}. Nothing was written. Run `grok-router skill --help` for setup, status, remove, and show."
        )),
        None => fail_text(
            "no skill command was selected. Nothing was written. Run `grok-router skill --help` for setup, status, remove, and show.",
        ),
    }
}

fn run_internal(root: &ArgMatches) -> i32 {
    let Some(internal) = root.subcommand_matches("__internal") else {
        return fail_text(
            "`__internal` was not selected. Run `grok-router __internal skill` to print the guide.",
        );
    };
    match internal.subcommand_name() {
        Some("skill") => write_stdout(guide()),
        Some(other) => fail_text(&format!(
            "`__internal {other}` is not a command. Nothing was written. Run `grok-router __internal skill` to print the guide."
        )),
        None => fail_text(
            "no __internal command was selected. Nothing was written. Run `grok-router __internal skill` to print the guide.",
        ),
    }
}

fn run_setup(skill: &ArgMatches) -> i32 {
    let Some(matches) = skill.subcommand_matches("setup") else {
        return fail_text(
            "setup was not selected. Run `grok-router skill setup --help` for the flags and an example.",
        );
    };
    let mode = match output_mode(matches) {
        Ok(mode) => mode,
        Err(message) => return fail_text(&message),
    };
    let root = match skill_root(matches) {
        Ok(root) => root,
        Err(message) => return fail_for(mode, "usage", &message),
    };
    finish(mode, "setup", setup(&root, write_mode(matches)))
}

fn run_status(skill: &ArgMatches) -> i32 {
    let Some(matches) = skill.subcommand_matches("status") else {
        return fail_text(
            "status was not selected. Run `grok-router skill status --help` for the flags and an example.",
        );
    };
    let mode = match output_mode(matches) {
        Ok(mode) => mode,
        Err(message) => return fail_text(&message),
    };
    let root = match skill_root(matches) {
        Ok(root) => root,
        Err(message) => return fail_for(mode, "usage", &message),
    };
    finish(mode, "status", inspect(&root))
}

fn run_remove(skill: &ArgMatches) -> i32 {
    let Some(matches) = skill.subcommand_matches("remove") else {
        return fail_text(
            "remove was not selected. Run `grok-router skill remove --help` for the flags and an example.",
        );
    };
    let mode = match output_mode(matches) {
        Ok(mode) => mode,
        Err(message) => return fail_text(&message),
    };
    let confirmation = confirmation(matches);
    if confirmation == Confirmation::No {
        return fail_for(mode, SkillFailure::NeedsYes.kind(), &SkillFailure::NeedsYes.to_string());
    }
    let root = match skill_root(matches) {
        Ok(root) => root,
        Err(message) => return fail_for(mode, "usage", &message),
    };
    finish(
        mode,
        "remove",
        remove(&root, confirmation, write_mode(matches)),
    )
}

fn finish(mode: OutputMode, command: &str, result: Result<Report, SkillFailure>) -> i32 {
    match result {
        Ok(report) => match render(command, &report, mode) {
            Ok(body) => write_stdout(&body),
            Err(message) => fail_for(mode, "io", &message),
        },
        Err(err) => fail_for(mode, err.kind(), &err.to_string()),
    }
}

fn skill_root(matches: &ArgMatches) -> Result<PathBuf, String> {
    let Some(path) = matches.get_one::<PathBuf>("skill-root") else {
        return Err(
            "no --skill-root was given, so nothing was written. Pass --skill-root with the directory that should hold the grok-router stub."
                .to_owned(),
        );
    };
    if path.as_os_str().is_empty() {
        return Err(
            "--skill-root is empty, so nothing was written. Pass --skill-root with a directory."
                .to_owned(),
        );
    }
    Ok(path.clone())
}

fn write_mode(matches: &ArgMatches) -> WriteMode {
    if matches.get_flag("dry-run") {
        WriteMode::DryRun
    } else {
        WriteMode::Apply
    }
}

fn confirmation(matches: &ArgMatches) -> Confirmation {
    if matches.get_flag("yes") {
        Confirmation::Yes
    } else {
        Confirmation::No
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    /// Human sentences.
    Text,
    /// One answer line.
    Quiet,
    /// One JSON object.
    Json,
}

fn output_mode(matches: &ArgMatches) -> Result<OutputMode, String> {
    if matches.get_flag("json") {
        return Ok(OutputMode::Json);
    }
    if matches.get_flag("quiet") {
        return Ok(OutputMode::Quiet);
    }
    match matches.get_one::<String>("output").map(String::as_str) {
        None | Some("text") => Ok(OutputMode::Text),
        Some("json" | "jsonl") => Ok(OutputMode::Json),
        Some(other) => Err(format!(
            "--output {other} is not text, json, or jsonl, so nothing was written. Pass --output json or --json."
        )),
    }
}

fn render(command: &str, report: &Report, mode: OutputMode) -> Result<String, String> {
    match mode {
        OutputMode::Json => render_json(command, report),
        OutputMode::Quiet => Ok(render_quiet(command, report)),
        OutputMode::Text => Ok(render_text(command, report)),
    }
}

fn render_json(command: &str, report: &Report) -> Result<String, String> {
    let body = JsonReport {
        command,
        status: report.disk.as_str(),
        effect: report.effect.as_str(),
        dry_run: report.mode.is_dry_run(),
        path: report.path.display().to_string(),
    };
    serde_json::to_string(&body).map_err(|err| {
        format!(
            "could not encode the skill result as JSON: {err}. Retry the command; report this if it persists."
        )
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
fn write_stdout(body: &str) -> i32 {
    let mut stdout = io::stdout().lock();
    let mut bytes = body.as_bytes().to_vec();
    if !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    match stdout.write_all(&bytes) {
        Ok(()) => 0,
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => 0,
        Err(err) => fail_text(&format!(
            "could not write the skill result: {err}. Retry when stdout accepts output."
        )),
    }
}

fn fail_text(message: &str) -> i32 {
    eprintln!("{message}");
    1
}

fn fail_for(mode: OutputMode, kind: &str, message: &str) -> i32 {
    match mode {
        OutputMode::Json => {
            let body = serde_json::json!({
                "error": kind,
                "kind": kind,
                "message": message,
            });
            eprintln!("{body}");
        }
        OutputMode::Text | OutputMode::Quiet => eprintln!("{message}"),
    }
    1
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
    use super::augment;

    #[test]
    fn augment_registers_skill_and_internal_but_not_session_start() {
        let cmd = augment(crate::command());
        let skill = cmd
            .find_subcommand("skill")
            .expect("augment should register skill");
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
            .expect("augment should register __internal");
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
