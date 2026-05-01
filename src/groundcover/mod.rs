use std::io;

use clap::CommandFactory;

mod app;
mod args;
mod config;
pub mod default;
mod load;
pub mod mesh;
mod output;
pub mod plan;
pub mod records;

pub use args::GroundcoverArgs;
pub use config::GroundcoverConfig;

pub const DEFAULT_CONFIG_NAME: &str = "greenmote.toml";
pub const DELETED_PLUGIN_NAME: &str = "deleted_groundcover.omwaddon";
pub const GROUNDCOVER_PLUGIN_NAME: &str = "groundcover.omwaddon";
pub const LOG_NAME: &str = "greenmote.log";

/// Runs the groundcover conversion subcommand.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub fn run(args: GroundcoverArgs) -> io::Result<()> {
    app::run(args)
}

fn handle_generated_output(args: &GroundcoverArgs, stdout: &mut dyn io::Write) -> io::Result<bool> {
    if let Some(shell) = args.generate_completion {
        let mut command = GroundcoverArgs::command();
        clap_complete::generate(shell, &mut command, "greenmote-convert", stdout);
        return Ok(true);
    }

    if args.generate_manpage {
        clap_mangen::Man::new(GroundcoverArgs::command()).render(stdout)?;
        return Ok(true);
    }

    Ok(false)
}
