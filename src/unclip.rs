// SPDX-License-Identifier: GPL-3.0-only

//! Public entry points for the `greenmote unclip` workflow.

use std::{io, io::Write, path::Path};

use crate::groundcover::CancellationToken;

mod app;
mod args;
mod cells;
pub(crate) mod config;
mod decide;
mod measure;
mod mesh;
mod occlusion;
mod orientation;
mod physics;
mod report;
mod setup;
mod static_occluders;
mod target;
mod terrain;
mod transform;
mod write;

pub use app::UNCLIP_LOG_NAME;
pub use args::{ActionArg, UnclipArgs};

/// Runs the groundcover unclipping subcommand.
///
/// The command discovers `OpenMW` configuration, merges CLI and `[unclip]` TOML settings, measures
/// every target reference, and only writes when `--write` is passed.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or terrain lookup errors.
pub fn run(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: &UnclipArgs,
) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    run_with_output_and_prompt(openmw_cfg, config_path, args, &mut stdout, &mut stderr)
}

/// Runs the groundcover unclipping subcommand with an explicit output stream.
///
/// Stderr still uses the process stream.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or terrain lookup errors.
pub fn run_with_output(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: &UnclipArgs,
    stdout: &mut dyn Write,
) -> io::Result<()> {
    run_with_output_and_cancel(
        openmw_cfg,
        config_path,
        args,
        stdout,
        &CancellationToken::default(),
    )
}

pub(crate) fn run_with_output_and_cancel(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: &UnclipArgs,
    stdout: &mut dyn Write,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    run_with_output_and_prompt_and_cancel(
        openmw_cfg,
        config_path,
        args,
        stdout,
        &mut stderr,
        cancellation,
    )
}

pub(crate) fn run_with_output_and_prompt(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: &UnclipArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> io::Result<()> {
    run_with_output_and_prompt_and_cancel(
        openmw_cfg,
        config_path,
        args,
        stdout,
        stderr,
        &CancellationToken::default(),
    )
}

pub(crate) fn run_with_output_and_prompt_and_cancel(
    openmw_cfg: Option<&Path>,
    config_path: Option<&Path>,
    args: &UnclipArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    check_cancellation(cancellation)?;
    let mut stdin = io::stdin().lock();
    let config = config::UnclipConfig::get(openmw_cfg, config_path, args, &mut stdin, stderr)?;
    app::run(&config, stdout, cancellation)
}

pub(crate) fn check_cancellation(cancellation: &CancellationToken) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

pub(crate) fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "unclip cancelled")
}
