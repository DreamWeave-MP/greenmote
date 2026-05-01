use std::io::{self, Write};

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
mod progress;
pub mod records;

pub use args::GroundcoverArgs;
pub use config::GroundcoverConfig;
pub use progress::{ConversionEvent, ConversionPhase};

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
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run_with_output(args, &mut stdout, &mut stderr)
}

/// Runs the groundcover conversion subcommand with explicit output streams.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub fn run_with_output(
    args: GroundcoverArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    run_with_output_and_events(args, stdout, stderr, &|_event| {})
}

/// Runs the groundcover conversion subcommand with explicit output streams and progress events.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub fn run_with_output_and_events(
    args: GroundcoverArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &progress::EventSink<'_>,
) -> io::Result<()> {
    app::run(args, stdout, stderr, events)
}
