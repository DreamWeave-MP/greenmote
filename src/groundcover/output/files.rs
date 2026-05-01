use std::{fs::create_dir_all, io};

use tes3::esp::TES3Object;

use crate::groundcover::{GroundcoverConfig, progress::CancellationToken};

use super::BuiltPlugins;

pub fn save_plugins(
    mut built: BuiltPlugins,
    config: &GroundcoverConfig,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    check_cancelled(cancellation)?;
    create_dir_all(&config.output_directory)?;

    built
        .groundcover_plugin
        .objects
        .insert(0, TES3Object::Header(built.groundcover_header));
    built
        .deleted_plugin
        .objects
        .insert(0, TES3Object::Header(built.deleted_header));

    built.groundcover_plugin.sort_objects();
    built.deleted_plugin.sort_objects();

    check_cancelled(cancellation)?;
    built
        .groundcover_plugin
        .save_path(config.output_directory.join(&config.groundcover_output))?;
    built
        .deleted_plugin
        .save_path(config.output_directory.join(&config.deleted_output))?;

    Ok(())
}

fn check_cancelled(cancellation: &CancellationToken) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "conversion cancelled",
        ))
    } else {
        Ok(())
    }
}
