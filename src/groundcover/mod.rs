//! Public entry points for the `greenmote convert` workflow.

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
};

mod app;
mod args;
mod auto_enable;
mod config;
pub(crate) mod default;
mod load;
pub(crate) mod mesh;
pub(crate) mod openmw;
mod output;
pub(crate) mod plan;
mod progress;
pub(crate) mod records;

pub use args::GroundcoverArgs;
pub use config::GroundcoverConfig;
pub use progress::{CancellationToken, ConversionEvent, ConversionPhase};

/// Default Greenmote TOML configuration filename.
pub const DEFAULT_CONFIG_NAME: &str = "greenmote.toml";
/// Filename for the generated plugin that deletes converted source references.
pub const DELETED_PLUGIN_NAME: &str = "deleted_groundcover.omwaddon";
pub(crate) const GENERATED_PLUGIN_AUTHOR: &str = "greenmote";
pub(crate) const GENERATED_PLUGIN_DESCRIPTION: &str = "Generated groundcover plugin created by greenmote convert.\nThis is a generated plugin.\nWhy are you reading this?\nBuy me coffee for moar tools.\nhttps://ko-fi.com/magicaldave";
/// Filename for the generated `OpenMW` groundcover plugin.
pub const GROUNDCOVER_PLUGIN_NAME: &str = "groundcover.omwaddon";
/// Default run log filename.
pub const LOG_NAME: &str = "greenmote.log";

/// Runs the groundcover conversion subcommand.
///
/// The command discovers `OpenMW` configuration, loads and merges `greenmote.toml`, resolves meshes
/// before writing plugins, and respects dry-run/config-validation modes from the supplied arguments
/// and persisted configuration.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub fn run(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: GroundcoverArgs,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run_with_output(openmw_cfg, config_path, args, &mut stdout, &mut stderr)
}

/// Runs the groundcover conversion subcommand with explicit output streams.
///
/// This is useful for tests and embedding where stdout/stderr must be captured. Runtime semantics
/// are the same as [`run`].
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub fn run_with_output(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: GroundcoverArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    run_with_output_and_events(openmw_cfg, config_path, args, stdout, stderr, &|_event| {})
}

/// Runs the groundcover conversion subcommand with explicit output streams and progress events.
///
/// Event callbacks may be invoked from Rayon worker threads during parallel phases. If cancellation
/// support is required, use the crate-private GUI entry point that accepts a [`CancellationToken`].
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub fn run_with_output_and_events(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: GroundcoverArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &progress::EventSink<'_>,
) -> io::Result<()> {
    app::run(
        openmw_cfg,
        config_path,
        args,
        stdout,
        stderr,
        events,
        &CancellationToken::default(),
    )
}

/// Runs conversion with an already-resolved runtime config.
///
/// This is intentionally separate from `GroundcoverArgs`: GUI run toggles must be able to turn
/// persisted booleans off for one run, while CLI booleans are append/enable-shaped for backwards
/// compatibility. Reusing the CLI shape here would make the GUI lie. Delightful, but no.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or output write errors.
pub(crate) fn run_with_config_events_and_cancel(
    openmw_cfg: Option<&std::path::Path>,
    config: &GroundcoverConfig,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &progress::EventSink<'_>,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    app::run_with_config(openmw_cfg, config, stdout, stderr, events, cancellation)
}

/// Loads the effective `greenmote.toml` location and editable groundcover config for GUI settings.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, TOML parse, or regex validation errors.
pub(crate) fn load_config_for_edit(
    openmw_cfg: Option<&Path>,
    config_path_override: Option<&Path>,
) -> io::Result<(PathBuf, GroundcoverConfig)> {
    let config_path = openmw::resolve_greenmote_config_path(config_path_override, openmw_cfg)?;
    let runtime_config = openmw::load_config_from_path(openmw_cfg)?;
    let persisted_openmw_cfg = openmw::persisted_config_path(&runtime_config);
    let output_directory = openmw::resolve_convert_output_directory(&runtime_config)?;
    let config = GroundcoverConfig::load_for_edit(
        &config_path,
        output_directory,
        Some(persisted_openmw_cfg),
    )?;

    Ok((config_path, config))
}

/// Moves aside the editable `greenmote.toml` and replaces it with a validated generated default
/// config.
///
/// # Errors
///
/// Returns `OpenMW` configuration or filesystem errors.
pub(crate) fn regenerate_config_for_edit(
    openmw_cfg: Option<&Path>,
    config_path_override: Option<&Path>,
) -> io::Result<(PathBuf, GroundcoverConfig)> {
    let runtime_config = openmw::load_config_from_path(openmw_cfg)?;
    let persisted_openmw_cfg = openmw::persisted_config_path(&runtime_config);
    let config_path = openmw::greenmote_config_path(config_path_override, &runtime_config);
    let output_directory = openmw::resolve_convert_output_directory(&runtime_config)?;
    let config =
        config::regenerate_for_edit(&config_path, output_directory, Some(persisted_openmw_cfg))?;

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
