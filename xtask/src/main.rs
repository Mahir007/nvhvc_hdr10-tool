//! Release tooling, run as `cargo xtask <command>`.
//!
//! `release-notes TAG REPO` prints the release notes for TAG as Markdown. REPO is `owner/name` on
//! GitHub. Run it from a clone with the tag and its history.

mod notes;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask release-notes TAG REPO";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, rest)) if command == "release-notes" && rest.len() == 2 => {
            notes::from_git(&rest[0], &rest[1])
        }
        _ => Err(USAGE.to_owned()),
    };
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
