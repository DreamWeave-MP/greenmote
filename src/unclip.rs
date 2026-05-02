use std::{io, io::Write};

mod app;
mod args;
mod cells;
mod mesh;
mod terrain;

pub use args::{UnclipArgs, UnclipOutputFormat};

/// Runs the groundcover unclipping subcommand.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or terrain lookup errors.
pub fn run(args: &UnclipArgs) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    run_with_output(args, &mut stdout)
}

/// Runs the groundcover unclipping subcommand with an explicit output stream.
///
/// # Errors
///
/// Returns filesystem, `OpenMW` configuration, plugin parse, VFS lookup, or terrain lookup errors.
pub fn run_with_output(args: &UnclipArgs, stdout: &mut dyn Write) -> io::Result<()> {
    app::run(args, stdout)
}
