use std::{
    io,
    path::{Path, PathBuf},
};

use openmw_config::OpenMWConfiguration;
use vfstool_lib::VFS;

pub fn load_config_from_path(openmw_cfg: Option<&Path>) -> io::Result<OpenMWConfiguration> {
    OpenMWConfiguration::new(Some(resolved_config_path(openmw_cfg)?)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read OpenMW configuration: {error}"),
        )
    })
}

pub fn resolved_config_path(openmw_cfg: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(path) = openmw_cfg {
        let absolute_path = if path.is_relative() {
            path.canonicalize().unwrap_or_else(|_| path.to_owned())
        } else {
            path.to_owned()
        };

        if absolute_path.is_file()
            || (absolute_path.is_dir() && absolute_path.join("openmw.cfg").is_file())
        {
            return Ok(absolute_path);
        }

        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "explicit --openmw-cfg path {} is neither a file nor a directory containing openmw.cfg",
                path.display()
            ),
        ));
    }

    let cwd_cfg = std::env::current_dir()
        .expect("failed to get current directory")
        .join("openmw.cfg");
    if cwd_cfg.is_file() {
        return Ok(cwd_cfg);
    }

    Ok(openmw_config::default_config_path())
}

#[must_use]
pub fn greenmote_config_path(config_path: Option<&Path>, config: &OpenMWConfiguration) -> PathBuf {
    config_path.map_or_else(
        || {
            config
                .user_config_path()
                .join(crate::groundcover::DEFAULT_CONFIG_NAME)
        },
        Path::to_owned,
    )
}

pub fn content_files(config: &OpenMWConfiguration) -> io::Result<Vec<String>> {
    let content_files = config
        .content_files_iter()
        .map(|plugin| plugin.value_str().to_owned())
        .collect::<Vec<_>>();

    if content_files.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "openmw.cfg has no content files",
        ))
    } else {
        Ok(content_files)
    }
}

#[must_use]
pub fn default_output_directory(config: &OpenMWConfiguration) -> PathBuf {
    config
        .data_local()
        .map_or_else(openmw_config::default_data_local_path, |data_local| {
            data_local.parsed().to_owned()
        })
}

#[must_use]
pub fn build_vfs(config: &OpenMWConfiguration) -> VFS {
    let directories = config
        .data_directories_iter()
        .map(openmw_config::DirectorySetting::parsed)
        .collect::<Vec<_>>();
    let fallback_archives = config
        .fallback_archives_iter()
        .map(openmw_config::FileSetting::value)
        .map(String::as_str)
        .collect::<Vec<_>>();

    VFS::from_directories(directories, Some(fallback_archives))
}
