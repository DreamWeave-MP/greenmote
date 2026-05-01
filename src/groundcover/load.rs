use std::{
    fs::metadata,
    io,
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use tes3::esp::{Cell, Header, Plugin, Static};
use vfstool_lib::VFS;

use crate::groundcover::{GroundcoverConfig, plan::LoadedPlugin};

#[derive(Clone, Debug)]
pub struct SourcePlugin {
    pub load_index: usize,
    pub plugin_name: String,
    pub plugin_path: PathBuf,
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

pub fn load_plugins(sources: Vec<SourcePlugin>) -> Vec<LoadedPlugin> {
    load_plugins_matching(sources, PluginLoadMode::Cells)
}

pub fn load_plugins_for_static_planning(sources: Vec<SourcePlugin>) -> Vec<LoadedPlugin> {
    load_plugins_matching(sources, PluginLoadMode::Statics)
}

#[derive(Clone, Copy)]
enum PluginLoadMode {
    Statics,
    Cells,
}

fn load_plugins_matching(sources: Vec<SourcePlugin>, mode: PluginLoadMode) -> Vec<LoadedPlugin> {
    let mut loaded = sources
        .into_par_iter()
        .filter_map(|source| match load_one_plugin(&source, mode) {
            Ok(plugin) => Some(LoadedPlugin {
                load_index: source.load_index,
                plugin_name: source.plugin_name,
                plugin_path: source.plugin_path,
                plugin,
            }),
            Err(error) => {
                eprintln!(
                    "[ WARNING ]: Plugin {} could not be loaded: {error}. Continuing.",
                    source.plugin_path.display()
                );
                None
            }
        })
        .collect::<Vec<_>>();

    loaded.sort_by_key(|plugin| plugin.load_index);
    loaded
}

fn load_one_plugin(source: &SourcePlugin, mode: PluginLoadMode) -> io::Result<Plugin> {
    Plugin::from_path_filtered(&source.plugin_path, |tag| {
        if &tag == Header::TAG {
            return true;
        }

        match mode {
            PluginLoadMode::Statics => &tag == Static::TAG,
            PluginLoadMode::Cells => &tag == Cell::TAG,
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

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
}
