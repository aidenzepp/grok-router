//! `skill` subcommand stub.

use clap::Command;

/// Adds the `skill` subcommand and nothing else.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(Command::new("skill"))
}

/// Prints that the command is not implemented and returns the failure status.
#[must_use]
pub fn run() -> i32 {
    eprintln!("not implemented");
    2
}
