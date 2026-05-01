use std::io;

use clap::Parser;

mod cli;
pub mod groundcover;

pub use cli::{Cli, Command};

/// Runs the `greenmote` command-line application.
///
/// # Errors
///
/// Returns errors from the selected subcommand, including filesystem, configuration, plugin parse,
/// and generated output errors.
pub fn run() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Convert(args) => groundcover::run(args),
    }
}
