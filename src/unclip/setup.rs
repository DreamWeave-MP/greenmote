use std::{collections::BTreeSet, io, path::PathBuf};

use tes3::esp::{Cell, Landscape, Plugin, Static};

use super::{cells::CellCoord, cells::active_grid, mesh::StaticMeshIndex};

pub(super) fn resolve_content_plugin_paths(
    content_files: &[String],
    vfs: &vfstool_lib::VFS,
) -> io::Result<Vec<PathBuf>> {
    content_files
        .iter()
        .map(|plugin| {
            vfs.get_file(plugin.as_str())
                .map(|file| file.path().to_path_buf())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("active content file {plugin} was not found in the VFS"),
                    )
                })
        })
        .collect()
}

pub(super) struct TargetPluginPath {
    pub(super) source_path: PathBuf,
    pub(super) destination_path: PathBuf,
}

pub(super) fn resolve_target_plugin(
    plugin: &std::path::Path,
    openmw_config: &openmw_config::OpenMWConfiguration,
    vfs: &vfstool_lib::VFS,
) -> io::Result<TargetPluginPath> {
    if plugin.is_file() {
        let path = plugin.to_path_buf();
        return Ok(TargetPluginPath {
            source_path: path.clone(),
            destination_path: path,
        });
    }

    let plugin_name = plugin.to_string_lossy();
    let source_path = vfs
        .get_file(plugin_name.as_ref())
        .map(|file| file.path().to_path_buf())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "target plugin {} was not found as a file or VFS entry",
                    plugin.display()
                ),
            )
        })?;
    let destination_path = vfs_target_destination(plugin, &source_path, openmw_config)?;

    Ok(TargetPluginPath {
        source_path,
        destination_path,
    })
}

fn vfs_target_destination(
    plugin: &std::path::Path,
    source_path: &std::path::Path,
    openmw_config: &openmw_config::OpenMWConfiguration,
) -> io::Result<PathBuf> {
    let file_name = plugin.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("target plugin {} has no filename", plugin.display()),
        )
    })?;
    let directory = openmw_config.data_local().map_or_else(
        || {
            source_path.parent().map_or_else(
                || {
                    Err(io::Error::other(format!(
                        "target plugin {} has no parent directory",
                        source_path.display()
                    )))
                },
                |parent| Ok(parent.to_path_buf()),
            )
        },
        |data_local| Ok(data_local.parsed().to_path_buf()),
    )?;

    Ok(directory.join(file_name))
}

pub(super) fn load_target_plugin(path: &std::path::Path) -> io::Result<Plugin> {
    Plugin::from_path(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to load target plugin {}: {error}", path.display()),
        )
    })
}

pub(super) fn load_context_plugins(paths: &[PathBuf]) -> io::Result<Vec<Plugin>> {
    paths
        .iter()
        .map(|path| {
            Plugin::from_path_filtered(path, |tag| {
                &tag == Landscape::TAG || &tag == Static::TAG || &tag == Cell::TAG
            })
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "failed to load unclip context from {}: {error}",
                        path.display()
                    ),
                )
            })
        })
        .collect()
}

pub(super) fn build_static_index(
    active_plugins: &[Plugin],
    extra_target_plugin: Option<&Plugin>,
) -> StaticMeshIndex {
    StaticMeshIndex::from_statics(
        active_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Static>)
            .chain(
                extra_target_plugin
                    .into_iter()
                    .flat_map(tes3::esp::Plugin::objects_of_type::<Static>),
            ),
    )
}

pub(super) fn path_matches_any(path: &std::path::Path, candidates: &[PathBuf]) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    candidates
        .iter()
        .map(|candidate| {
            candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone())
        })
        .any(|candidate| candidate == path)
}

pub(super) fn active_cells(target_cells: &BTreeSet<CellCoord>) -> io::Result<BTreeSet<CellCoord>> {
    let mut cells = BTreeSet::new();

    for cell in target_cells {
        cells.extend(active_grid(*cell)?);
    }

    Ok(cells)
}
