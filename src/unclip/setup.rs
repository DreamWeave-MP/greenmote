// SPDX-License-Identifier: GPL-3.0-only

use std::{collections::BTreeSet, io, path::PathBuf};

use tes3::esp::{Cell, Landscape, Plugin, Static};

use crate::groundcover::CancellationToken;

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

pub(super) enum ContextPlugin<'a> {
    Borrowed(&'a Plugin),
    Owned(Plugin),
}

impl ContextPlugin<'_> {
    pub(super) fn as_plugin(&self) -> &Plugin {
        match self {
            Self::Borrowed(plugin) => plugin,
            Self::Owned(plugin) => plugin,
        }
    }
}

pub(super) fn load_context_plugins<'a>(
    paths: &[PathBuf],
    target_path: &std::path::Path,
    target_plugin: &'a Plugin,
    cancellation: &CancellationToken,
) -> io::Result<Vec<ContextPlugin<'a>>> {
    paths
        .iter()
        .map(|path| {
            super::check_cancellation(cancellation)?;
            if path_matches(path, target_path) {
                return Ok(ContextPlugin::Borrowed(target_plugin));
            }

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
            .map(ContextPlugin::Owned)
        })
        .collect()
}

pub(super) fn build_static_index<'a>(
    active_plugins: impl IntoIterator<Item = &'a Plugin>,
    extra_target_plugin: Option<&'a Plugin>,
) -> StaticMeshIndex {
    StaticMeshIndex::from_statics(
        active_plugins
            .into_iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Static>)
            .chain(
                extra_target_plugin
                    .into_iter()
                    .flat_map(tes3::esp::Plugin::objects_of_type::<Static>),
            ),
    )
}

pub(super) fn path_matches_any(path: &std::path::Path, candidates: &[PathBuf]) -> bool {
    candidates
        .iter()
        .any(|candidate| path_matches(candidate, path))
}

fn path_matches(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());

    left == right
}

pub(super) fn active_cells(target_cells: &BTreeSet<CellCoord>) -> io::Result<BTreeSet<CellCoord>> {
    let mut cells = BTreeSet::new();

    for cell in target_cells {
        cells.extend(active_grid(*cell)?);
    }

    Ok(cells)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tes3::esp::Plugin;

    use super::{ContextPlugin, load_context_plugins};

    #[test]
    fn active_target_context_slot_borrows_loaded_target_without_reloading() {
        let target_plugin = Plugin::default();
        let paths = ["target.omwaddon".into()];

        let context_plugins = load_context_plugins(
            &paths,
            Path::new("target.omwaddon"),
            &target_plugin,
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(context_plugins.len(), 1);
        assert!(matches!(context_plugins[0], ContextPlugin::Borrowed(_)));
        assert!(std::ptr::eq(
            std::ptr::from_ref(context_plugins[0].as_plugin()),
            std::ptr::from_ref(&target_plugin),
        ));
    }

    #[test]
    fn inactive_context_path_still_loads_owned_plugin() {
        let target_plugin = Plugin::default();
        let paths = ["missing-context.omwaddon".into()];

        let Err(error) = load_context_plugins(
            &paths,
            Path::new("target.omwaddon"),
            &target_plugin,
            &crate::groundcover::CancellationToken::default(),
        ) else {
            panic!("inactive context path should be loaded from disk");
        };

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("missing-context.omwaddon"));
    }
}
