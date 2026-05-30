#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

use std::io;

use clap::Parser;

mod cli;
/// Groundcover conversion command support.
pub mod groundcover;
#[cfg(feature = "gui")]
mod gui;
/// Groundcover clipping inspection and repair command support.
pub mod unclip;

pub use cli::{Cli, Command};

/// Runs the `greenmote` application.
///
/// With the `gui` feature enabled, launching without arguments opens the graphical interface.
/// Otherwise, the command-line parser handles the invocation and defaults to `convert` when no
/// subcommand is provided.
///
/// # Errors
///
/// Returns errors from the selected subcommand, including filesystem, configuration, plugin parse,
/// and generated output errors.
pub fn run() -> io::Result<()> {
    #[cfg(feature = "gui")]
    if should_launch_gui(std::env::args_os()) {
        return gui::run();
    }

    let cli = Cli::parse();

    if cli.handle_generated_output(&mut io::stdout())? {
        return Ok(());
    }

    let openmw_cfg = cli.openmw_cfg.clone();
    let config_path = cli.config.clone();
    match cli.command_or_default() {
        Command::Convert(args) => {
            groundcover::run(openmw_cfg.as_deref(), config_path.as_deref(), args)
        }
        Command::Unclip(args) => unclip::run(openmw_cfg.as_deref(), config_path.as_deref(), &args),
    }
}

#[cfg(feature = "gui")]
fn should_launch_gui<I, T>(args: I) -> bool
where
    I: IntoIterator<Item = T>,
{
    args.into_iter().nth(1).is_none()
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "gui")]
    use super::should_launch_gui;

    #[cfg(feature = "gui")]
    #[test]
    fn gui_feature_launches_gui_only_for_empty_invocation() {
        assert!(should_launch_gui(["greenmote"]));
        assert!(!should_launch_gui(["greenmote", "convert"]));
        assert!(!should_launch_gui(["greenmote", "--help"]));
        assert!(!should_launch_gui(["greenmote", "--generate-manpage"]));
    }
}
