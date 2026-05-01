use std::{
    fs::copy,
    io,
    path::{Path, PathBuf},
};

use openmw_config::OpenMWConfiguration;

use crate::groundcover::GroundcoverConfig;

pub fn validate_output_directory(
    config: &OpenMWConfiguration,
    groundcover_config: &GroundcoverConfig,
) -> io::Result<()> {
    if output_directory_is_visible(config, &groundcover_config.output_directory) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to auto-enable outputs in {} because it is not data-local or a configured data directory",
                groundcover_config.output_directory.display()
            ),
        ))
    }
}

pub fn outputs(
    config: &mut OpenMWConfiguration,
    groundcover_config: &GroundcoverConfig,
) -> io::Result<PathBuf> {
    let user_openmw_cfg = config.user_config_path().join("openmw.cfg");
    let backup = backup_openmw_cfg(&user_openmw_cfg)?;

    if !config.has_groundcover_file(&groundcover_config.groundcover_output) {
        config
            .add_groundcover_file(&groundcover_config.groundcover_output)
            .map_err(to_io_error)?;
    }

    if !config.has_content_file(&groundcover_config.deleted_output) {
        config
            .add_content_file(&groundcover_config.deleted_output)
            .map_err(to_io_error)?;
    }

    config.save_user().map_err(to_io_error)?;
    Ok(backup)
}

fn backup_openmw_cfg(openmw_cfg: &Path) -> io::Result<PathBuf> {
    let file_name = openmw_cfg.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "OpenMW user config path has no file name",
        )
    })?;
    let backup_name = format!("{}.greenmote.bak", file_name.to_string_lossy());
    let backup_path = openmw_cfg.with_file_name(backup_name);

    copy(openmw_cfg, &backup_path)?;

    Ok(backup_path)
}

fn output_directory_is_visible(config: &OpenMWConfiguration, output_directory: &Path) -> bool {
    config
        .data_local()
        .is_some_and(|data_local| paths_equal(data_local.parsed(), output_directory))
        || config
            .data_directories_iter()
            .any(|data_dir| paths_equal(data_dir.parsed(), output_directory))
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn to_io_error<E: std::fmt::Display>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
