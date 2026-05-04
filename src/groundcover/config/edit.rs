use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::GroundcoverConfig;

pub(super) fn regenerate(
    config_path: &Path,
    default_output_directory: PathBuf,
    openmw_cfg: Option<PathBuf>,
) -> io::Result<GroundcoverConfig> {
    let mut config = GroundcoverConfig::with_output_directory(default_output_directory);
    config.openmw_cfg = openmw_cfg;
    let temp_path = next_temp_config_path(config_path);
    let config = config.save_for_edit_new(&temp_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to write temporary config {}: {error}",
                temp_path.display()
            ),
        )
    })?;
    if let Err(error) = replace_config_with_backup(config_path, &temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(config)
}

pub(super) fn replace_config_with_backup(config_path: &Path, temp_path: &Path) -> io::Result<()> {
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

pub(super) fn back_up_existing_config(config_path: &Path) -> io::Result<Option<PathBuf>> {
    let metadata = match fs::symlink_metadata(config_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_file() && !metadata.file_type().is_symlink() {
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
        if !path_entry_exists(&temp_path) {
            return temp_path;
        }
    }

    unreachable!("unbounded temp suffix search should always find a candidate")
}

fn next_backup_path(config_path: &Path) -> PathBuf {
    let first_backup_path = config_path.with_extension("toml.bak");
    if !path_entry_exists(&first_backup_path) {
        return first_backup_path;
    }

    for index in 1.. {
        let backup_path = config_path.with_extension(format!("toml.bak.{index}"));
        if !path_entry_exists(&backup_path) {
            return backup_path;
        }
    }

    unreachable!("unbounded backup suffix search should always find a candidate")
}

fn path_entry_exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}
