use std::io;

use clap::Parser;

mod cli;
pub mod groundcover;
#[cfg(feature = "gui")]
mod gui;

pub use cli::{Cli, Command};

/// Runs the `greenmote` command-line application.
///
/// # Errors
///
/// Returns errors from the selected subcommand, including filesystem, configuration, plugin parse,
/// and generated output errors.
pub fn run() -> io::Result<()> {
    #[cfg(feature = "gui")]
    if std::env::args_os().len() == 1 {
        return gui::run();
    }

    let cli = Cli::parse();

    if cli.handle_generated_output(&mut io::stdout())? {
        return Ok(());
    }

    match cli.command_or_default() {
        Command::Convert(args) => groundcover::run(args),
    }
}
