//! `set-default` subcommand stub.

use clap::Command;

/// Adds the `set-default` subcommand and nothing else.
#[must_use]
pub fn augment(cmd: Command) -> Command {
    cmd.subcommand(Command::new("set-default"))
}

/// Prints that the command is not implemented and returns the failure status.
#[must_use]
pub fn run() -> i32 {
    eprintln!("not implemented");
    2
}
