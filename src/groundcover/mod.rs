use std::{
    io::{self, Write},
    path::PathBuf,
};

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
pub(crate) const GENERATED_PLUGIN_AUTHOR: &str = "greenmote";
pub(crate) const GENERATED_PLUGIN_DESCRIPTION: &str = "Generated groundcover plugin created by greenmote convert.\nThis is a generated plugin.\nWhy are you reading this?\nBuy me coffee for moar tools.\nhttps://ko-fi.com/magicaldave";
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

/// Loads the effective `greenmote.toml` location and editable groundcover config for GUI settings.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, TOML parse, or regex validation errors.
pub(crate) fn load_config_for_edit(
    args: &GroundcoverArgs,
) -> io::Result<(PathBuf, GroundcoverConfig)> {
    let openmw_config = openmw::load_config(args)?;
    let config_path = openmw::greenmote_config_path(args, &openmw_config);
    let default_output_directory = openmw::default_output_directory(&openmw_config);
    let config = GroundcoverConfig::load_for_edit(&config_path, default_output_directory)?;

    Ok((config_path, config))
}

/// Saves editable GUI settings through the same TOML schema used by the CLI.
///
/// # Errors
///
/// Returns regex validation or filesystem errors.
pub(crate) fn save_config_for_edit(
    config: &GroundcoverConfig,
    path: &std::path::Path,
) -> io::Result<GroundcoverConfig> {
    config.save_for_edit(path)
}
