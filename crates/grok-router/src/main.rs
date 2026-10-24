#![cfg_attr(
    test,
    allow(
        clippy::missing_errors_doc,
        clippy::missing_panics_doc,
        reason = "tests report failures rather than document callable contracts"
    )
)]

//! Binary for the `grok-router` command.

use grok_router::commands::{add, list, remove, search, set_default, skill};

/// Runs the `grok-router` binary.
fn main() {
    let cmd = list::augment(grok_router::command());
    let cmd = search::augment(cmd);
    let cmd = add::augment(cmd);
    let cmd = set_default::augment(cmd);
    let cmd = remove::augment(cmd);
    let cmd = skill::augment(cmd);
    let matches = cmd.get_matches();
    match matches.subcommand_name() {
        None | Some("list") => std::process::exit(list::run(&matches)),
        Some("search") => std::process::exit(search::run(&matches)),
        Some("add") => std::process::exit(add::run(&matches)),
        Some("set-default") => std::process::exit(set_default::run(&matches)),
        Some("remove") => std::process::exit(remove::run(&matches)),
        Some("skill" | "__internal") => std::process::exit(skill::run(&matches)),
        Some(_) => {}
    }
}
