use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
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

/// Moves aside the editable `greenmote.toml` and replaces it with a validated generated default
/// config.
///
/// # Errors
///
/// Returns `OpenMW` configuration or filesystem errors.
pub(crate) fn regenerate_config_for_edit(
    args: &GroundcoverArgs,
) -> io::Result<(PathBuf, GroundcoverConfig)> {
    let openmw_config = openmw::load_config(args)?;
    let config_path = openmw::greenmote_config_path(args, &openmw_config);
    let default_output_directory = openmw::default_output_directory(&openmw_config);
    let config = GroundcoverConfig::with_output_directory(default_output_directory);
    let temp_path = next_temp_config_path(&config_path);
    let config = config.save_for_edit_new(&temp_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to write temporary config {}: {error}",
                temp_path.display()
            ),
        )
    })?;
    replace_config_with_backup(&config_path, &temp_path)?;

    Ok((config_path, config))
}

fn replace_config_with_backup(config_path: &Path, temp_path: &Path) -> io::Result<()> {
    let backup_path = back_up_existing_config(config_path)?;

    match rename_with_context(temp_path, config_path) {
        Ok(()) => Ok(()),
        Err(error) => {
            if let Some(backup_path) = backup_path
                && let Err(restore_error) = fs::rename(&backup_path, config_path)
            {
                let _ = fs::remove_file(temp_path);
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; additionally failed to restore backup {} to {}: {restore_error}",
                        backup_path.display(),
                        config_path.display()
                    ),
                ));
            }
            let _ = fs::remove_file(temp_path);
            Err(error)
        }
    }
}

fn back_up_existing_config(config_path: &Path) -> io::Result<Option<PathBuf>> {
    if !config_path.exists() {
        return Ok(None);
    }

    let metadata = fs::symlink_metadata(config_path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cannot back up config {}; expected a file",
                config_path.display()
            ),
        ));
    }

    let backup_path = next_backup_path(config_path);
    rename_with_context(config_path, &backup_path)?;
    Ok(Some(backup_path))
}

fn rename_with_context(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to move {} to {}: {error}",
                source.display(),
                destination.display()
            ),
        )
    })
}

fn next_temp_config_path(config_path: &Path) -> PathBuf {
    for index in 0.. {
        let extension = if index == 0 {
            "toml.tmp".to_owned()
        } else {
            format!("toml.tmp.{index}")
        };
        let temp_path = config_path.with_extension(extension);
        if !temp_path.exists() {
            return temp_path;
        }
    }

    unreachable!("unbounded temp suffix search should always find a candidate")
}

fn next_backup_path(config_path: &Path) -> PathBuf {
    let first_backup_path = config_path.with_extension("toml.bak");
    if !first_backup_path.exists() {
        return first_backup_path;
    }

    for index in 1.. {
        let backup_path = config_path.with_extension(format!("toml.bak.{index}"));
        if !backup_path.exists() {
            return backup_path;
        }
    }

    unreachable!("unbounded backup suffix search should always find a candidate")
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
