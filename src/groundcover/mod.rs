use std::io;

mod args;
mod config;
pub mod default;

pub use args::GroundcoverArgs;
pub use config::GroundcoverConfig;

pub const DEFAULT_CONFIG_NAME: &str = "groundcoverify.toml";
pub const DELETED_PLUGIN_NAME: &str = "deleted_groundcover.omwaddon";
pub const GROUNDCOVER_PLUGIN_NAME: &str = "groundcover.omwaddon";
pub const LOG_NAME: &str = "groundcoverify.log";

pub fn run(args: GroundcoverArgs) -> io::Result<()> {
    if args.debug {
        eprintln!("greenmote convert scaffold is wired; implementation follows.");
    }

    Ok(())
}
