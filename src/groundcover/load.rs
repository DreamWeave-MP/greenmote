use std::{
    collections::HashSet,
    fs::metadata,
    hash::BuildHasher,
    io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use rayon::prelude::*;
use tes3::esp::{Cell, Header, Plugin, Static};
use vfstool_lib::VFS;

use crate::groundcover::{
    GENERATED_PLUGIN_AUTHOR, GENERATED_PLUGIN_DESCRIPTION, GroundcoverConfig, plan::LoadedPlugin,
    progress::CancellationToken, records,
};

#[derive(Clone, Debug)]
pub struct SourcePlugin {
    pub load_index: usize,
    pub plugin_name: String,
    pub plugin_path: PathBuf,
}

pub struct PluginLoadResult {
    pub plugins: Vec<LoadedPlugin>,
    pub skipped_generated: Vec<SkippedGeneratedPlugin>,
    pub warnings: Vec<PluginLoadWarning>,
}

pub struct CellScanLoadResult {
    pub loaded_plugins: usize,
    pub loaded_load_indices: Vec<usize>,
    pub cell_plans: Vec<crate::groundcover::plan::PluginCellPlan>,
    pub warnings: Vec<PluginLoadWarning>,
}

#[derive(Debug)]
pub struct SkippedGeneratedPlugin {
    pub plugin_name: String,
    pub plugin_path: PathBuf,
}

#[derive(Debug)]
pub struct PluginLoadWarning {
    pub plugin_path: PathBuf,
    pub error: io::Error,
}

#[must_use]
pub fn is_supported_plugin(path: &Path) -> bool {
    metadata(path).is_ok()
        && path.extension().is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().to_str().unwrap_or_default(),
                "esp" | "esm" | "omwaddon" | "omwgame"
            )
        })
}

pub fn resolve_source_plugins(
    content_files: &[String],
    config: &GroundcoverConfig,
    vfs: &VFS,
) -> Vec<SourcePlugin> {
    content_files
        .iter()
        .enumerate()
        .filter_map(|(load_index, plugin)| {
            if config.is_ignored_plugin_name(plugin) {
                return None;
            }

            let vfs_file = vfs.get_file(plugin.as_str())?;
            let plugin_path = vfs_file.path().to_path_buf();

            if !is_supported_plugin(&plugin_path) {
                return None;
            }

            Some(SourcePlugin {
                load_index,
                plugin_name: plugin.clone(),
                plugin_path,
            })
        })
        .collect()
}

pub fn load_plugins_for_static_planning(
    sources: Vec<SourcePlugin>,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancellation: &CancellationToken,
) -> io::Result<PluginLoadResult> {
    load_plugins_matching(sources, PluginLoadMode::Statics, progress, cancellation)
}

pub fn load_and_scan_plugins_for_cell_planning<S: BuildHasher + Sync>(
    sources: Vec<SourcePlugin>,
    matched_static_ids: &HashSet<String, S>,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancellation: &CancellationToken,
) -> io::Result<CellScanLoadResult> {
    let total = sources.len();
    let completed = AtomicUsize::new(0);

    let mut scanned = sources
        .into_par_iter()
        .map(|source| {
            let result = if cancellation.is_cancelled() {
                Err(PluginLoadError::Cancelled(cancelled_error()))
            } else {
                load_and_scan_one_plugin(&source, matched_static_ids)
            };

            let load_index = source.load_index;
            let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
            progress(current, total);
            (load_index, result)
        })
        .collect::<Vec<_>>();

    scanned.sort_by_key(|(load_index, _result)| *load_index);

    let mut loaded_plugins = 0;
    let mut loaded_load_indices = Vec::new();
    let mut cell_plans = Vec::new();
    let mut warnings = Vec::new();

    for (_load_index, result) in scanned {
        match result {
            Ok(Some(cell_plan)) => {
                loaded_plugins += 1;
                loaded_load_indices.push(cell_plan.load_index);
                cell_plans.push(cell_plan);
            }
            Ok(None) => {}
            Err(PluginLoadError::Warning(warning)) => warnings.push(warning),
            Err(PluginLoadError::Cancelled(error)) => return Err(error),
        }
    }

    Ok(CellScanLoadResult {
        loaded_plugins,
        loaded_load_indices,
        cell_plans,
        warnings,
    })
}

#[derive(Clone, Copy)]
enum PluginLoadMode {
    Statics,
    Cells,
}

fn load_plugins_matching(
    sources: Vec<SourcePlugin>,
    mode: PluginLoadMode,
    progress: &(dyn Fn(usize, usize) + Sync),
    cancellation: &CancellationToken,
) -> io::Result<PluginLoadResult> {
    let total = sources.len();
    let completed = AtomicUsize::new(0);

    let mut loaded = sources
        .into_par_iter()
        .map(|source| {
            let result = if cancellation.is_cancelled() {
                Err(cancelled_error())
            } else {
                load_one_plugin(&source, mode)
            };

            let result = match result {
                Ok(RawPluginLoadOutcome::Loaded(plugin)) => {
                    Ok(PluginLoadOutcome::Loaded(LoadedPlugin {
                        load_index: source.load_index,
                        plugin_name: source.plugin_name,
                        plugin_path: source.plugin_path,
                        plugin,
                    }))
                }
                Ok(RawPluginLoadOutcome::SkippedGenerated) => Ok(
                    PluginLoadOutcome::SkippedGenerated(SkippedGeneratedPlugin {
                        plugin_name: source.plugin_name,
                        plugin_path: source.plugin_path,
                    }),
                ),
                Err(error)
                    if error.kind() == io::ErrorKind::Interrupted
                        && cancellation.is_cancelled() =>
                {
                    Err(PluginLoadError::Cancelled(error))
                }
                Err(error) => Err(PluginLoadError::Warning(PluginLoadWarning {
                    plugin_path: source.plugin_path,
                    error,
                })),
            };

            let load_index = source.load_index;
            let current = completed.fetch_add(1, Ordering::Relaxed) + 1;
            progress(current, total);
            (load_index, result)
        })
        .collect::<Vec<_>>();

    loaded.sort_by_key(|(load_index, _result)| *load_index);

    let mut plugins = Vec::new();
    let mut skipped_generated = Vec::new();
    let mut warnings = Vec::new();

    for (_load_index, result) in loaded {
        match result {
            Ok(PluginLoadOutcome::Loaded(plugin)) => plugins.push(plugin),
            Ok(PluginLoadOutcome::SkippedGenerated(skipped)) => skipped_generated.push(skipped),
            Err(PluginLoadError::Warning(warning)) => warnings.push(warning),
            Err(PluginLoadError::Cancelled(error)) => return Err(error),
        }
    }

    Ok(PluginLoadResult {
        plugins,
        skipped_generated,
        warnings,
    })
}

enum PluginLoadOutcome {
    Loaded(LoadedPlugin),
    SkippedGenerated(SkippedGeneratedPlugin),
}

enum PluginLoadError {
    Warning(PluginLoadWarning),
    Cancelled(io::Error),
}

fn load_and_scan_one_plugin<S: BuildHasher>(
    source: &SourcePlugin,
    matched_static_ids: &HashSet<String, S>,
) -> Result<Option<crate::groundcover::plan::PluginCellPlan>, PluginLoadError> {
    let plugin = match load_one_plugin(source, PluginLoadMode::Cells) {
        Ok(RawPluginLoadOutcome::Loaded(plugin)) => plugin,
        Ok(RawPluginLoadOutcome::SkippedGenerated) => return Ok(None),
        Err(error) => {
            return Err(PluginLoadError::Warning(PluginLoadWarning {
                plugin_path: source.plugin_path.clone(),
                error,
            }));
        }
    };
    let source_master = crate::groundcover::plan::MasterSpec::from_path(&source.plugin_path);
    let header_masters =
        plugin
            .objects_of_type::<Header>()
            .next()
            .map_or_else(Vec::new, |header| {
                header
                    .masters
                    .iter()
                    .map(|(name, size)| crate::groundcover::plan::MasterSpec {
                        name: name.clone(),
                        size: *size,
                    })
                    .collect()
            });
    let (groundcover_cells, touched_refs, used_static_ids) =
        records::process_exterior_cells(&plugin, matched_static_ids);

    Ok(Some(crate::groundcover::plan::PluginCellPlan {
        load_index: source.load_index,
        plugin_name: source.plugin_name.clone(),
        plugin_path: source.plugin_path.clone(),
        source_master,
        header_masters,
        groundcover_cells,
        touched_refs,
        used_static_ids,
    }))
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "conversion cancelled")
}

enum RawPluginLoadOutcome {
    Loaded(Plugin),
    SkippedGenerated,
}

fn load_one_plugin(
    source: &SourcePlugin,
    mode: PluginLoadMode,
) -> io::Result<RawPluginLoadOutcome> {
    if is_greenmote_generated_plugin(&source.plugin_path)? {
        return Ok(RawPluginLoadOutcome::SkippedGenerated);
    }

    Plugin::from_path_filtered(&source.plugin_path, |tag| {
        if &tag == Header::TAG {
            return true;
        }

        match mode {
            PluginLoadMode::Statics => &tag == Static::TAG,
            PluginLoadMode::Cells => &tag == Cell::TAG,
        }
    })
    .map(RawPluginLoadOutcome::Loaded)
}

fn is_greenmote_generated_plugin(path: &Path) -> io::Result<bool> {
    let plugin = Plugin::from_path_filtered(path, |tag| &tag == Header::TAG)?;

    Ok(plugin
        .objects_of_type::<Header>()
        .next()
        .is_some_and(is_greenmote_header))
}

fn is_greenmote_header(header: &Header) -> bool {
    header
        .author
        .0
        .trim()
        .eq_ignore_ascii_case(GENERATED_PLUGIN_AUTHOR)
        && is_greenmote_description(&header.description.0)
}

fn is_greenmote_description(description: &str) -> bool {
    matches!(
        description.trim(),
        GENERATED_PLUGIN_DESCRIPTION
            | "Generated groundcover plugin from vanilla-style static refs"
            | "Generated deleted groundcover plugin"
    )
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use tes3::esp::{FixedString, TES3Object};

    use super::*;

    static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

    struct TempFile {
        path: PathBuf,
    }

    impl TempFile {
        fn new(name: &str) -> Self {
            let (stem, extension) = name.rsplit_once('.').map_or((name, ""), |parts| parts);
            let extension = if extension.is_empty() {
                String::new()
            } else {
                format!(".{extension}")
            };
            let path = std::env::temp_dir().join(format!(
                "greenmote-load-test-{stem}-{}-{}{extension}",
                std::process::id(),
                NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, []).unwrap();

            Self { path }
        }

        fn as_path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    #[test]
    fn supported_plugin_extensions_are_case_insensitive() {
        for extension in ["esp", "ESM", "omwaddon", "OMWGAME"] {
            let file = TempFile::new(&format!("plugin.{extension}"));

            assert!(is_supported_plugin(file.as_path()), "{extension}");
        }
    }

    #[test]
    fn unsupported_or_missing_plugins_are_rejected() {
        let file = TempFile::new("plugin.txt");
        let missing = std::env::temp_dir().join(format!(
            "greenmote-load-test-missing-{}-{}",
            std::process::id(),
            NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed)
        ));

        assert!(!is_supported_plugin(file.as_path()));
        assert!(!is_supported_plugin(&missing));
    }

    #[test]
    fn greenmote_generated_plugins_are_skipped_without_warning() {
        let generated = TempFile::new("generated.omwaddon");
        let source = TempFile::new("source.esp");
        write_plugin_with_description(
            generated.as_path(),
            GENERATED_PLUGIN_AUTHOR,
            GENERATED_PLUGIN_DESCRIPTION,
            "generated_grass",
        );
        write_plugin(source.as_path(), "someone else", "source_grass");

        let result = load_plugins_for_static_planning(
            vec![
                source_plugin(0, "generated.omwaddon", generated.as_path()),
                source_plugin(1, "source.esp", source.as_path()),
            ],
            &|_, _| {},
            &CancellationToken::default(),
        )
        .unwrap();

        assert!(result.warnings.is_empty());
        assert_eq!(result.skipped_generated.len(), 1);
        assert_eq!(
            result.skipped_generated[0].plugin_name,
            "generated.omwaddon"
        );
        assert_eq!(result.plugins.len(), 1);
        assert_eq!(result.plugins[0].plugin_name, "source.esp");
        assert_eq!(
            result.plugins[0]
                .plugin
                .objects_of_type::<Static>()
                .next()
                .unwrap()
                .id,
            "source_grass"
        );
    }

    #[test]
    fn greenmote_author_alone_does_not_skip_plugin() {
        let source = TempFile::new("source.esp");
        write_plugin(source.as_path(), GENERATED_PLUGIN_AUTHOR, "source_grass");

        let result = load_plugins_for_static_planning(
            vec![source_plugin(0, "source.esp", source.as_path())],
            &|_, _| {},
            &CancellationToken::default(),
        )
        .unwrap();

        assert!(result.warnings.is_empty());
        assert!(result.skipped_generated.is_empty());
        assert_eq!(result.plugins.len(), 1);
    }

    fn source_plugin(load_index: usize, plugin_name: &str, path: &Path) -> SourcePlugin {
        SourcePlugin {
            load_index,
            plugin_name: plugin_name.to_owned(),
            plugin_path: path.to_path_buf(),
        }
    }

    fn write_plugin(path: &Path, author: &str, static_id: &str) {
        write_plugin_with_description(path, author, "", static_id);
    }

    fn write_plugin_with_description(
        path: &Path,
        author: &str,
        description: &str,
        static_id: &str,
    ) {
        let mut plugin = Plugin {
            objects: vec![
                TES3Object::Header(Header {
                    author: FixedString(author.to_owned()),
                    description: FixedString(description.to_owned()),
                    num_objects: 1,
                    ..Header::default()
                }),
                TES3Object::Static(Static {
                    id: static_id.to_owned(),
                    ..Static::default()
                }),
            ],
        };
        plugin.save_path(path).unwrap();
    }
}
