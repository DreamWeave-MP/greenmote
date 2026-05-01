use std::io;

mod app;
mod args;
mod auto_enable;
mod config;
pub mod default;
mod load;
pub mod mesh;
mod openmw;
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
