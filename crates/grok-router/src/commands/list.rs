//! `list` subcommand stub.

use clap::Command;

/// Adds the `list` subcommand and nothing else.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(Command::new("list"))
}

/// Prints that the command is not implemented and returns the failure status.
#[must_use]
pub fn run(matches: &clap::ArgMatches) -> i32 {
    let _ = matches;
    eprintln!("not implemented");
    2
}
