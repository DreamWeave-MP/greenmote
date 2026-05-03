use std::path::{Path, PathBuf};

use regex::RegexSet;
use serde::{Deserialize, Serialize};

use crate::groundcover::{GroundcoverConfig, default};

use super::to_io_error;

#[derive(Debug, Deserialize, Serialize)]
// Mirrors the public TOML schema so we can distinguish an omitted output directory from one the user
// intentionally set. Convert-specific knobs live under `[convert]`; root-level command knobs are
// not supported because this tool is still wet paint, not a museum.
pub(super) struct GroundcoverConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_directory: Option<PathBuf>,

    #[serde(default, skip_serializing)]
    validate_config: Option<bool>,

    #[serde(default)]
    convert: ConvertConfigFile,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
// Mirrors persisted convert options. CLI-only switches do not belong here; writing transient
// command mode into TOML is how a config file starts lying to its owner.
#[allow(clippy::struct_excessive_bools)]
struct ConvertConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    grass_ids: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    exclude: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    ignored_plugins: Option<Vec<String>>,

    #[serde(default)]
    dry_run: bool,

    #[serde(default)]
    debug: bool,

    #[serde(default)]
    auto_enable: bool,
}

impl GroundcoverConfigFile {
    pub(super) fn from_runtime(config: &GroundcoverConfig) -> Self {
        Self {
            output_directory: Some(config.output_directory.clone()),
            validate_config: None,
            convert: ConvertConfigFile {
                grass_ids: Some(config.grass_ids.clone()),
                exclude: Some(config.exclude.clone()),
                ignored_plugins: Some(config.ignored_plugins.clone()),
                dry_run: config.dry_run,
                debug: config.debug,
                auto_enable: config.auto_enable,
            },
        }
    }

    pub(super) fn from_toml(
        contents: &str,
        default_output_directory: PathBuf,
    ) -> std::io::Result<GroundcoverConfig> {
        let file = toml::from_str::<Self>(contents).map_err(to_io_error)?;
        if file.validate_config.is_some() {
            return Err(to_io_error("validate_config is a CLI-only option"));
        }
        let convert = file.convert;

        Ok(GroundcoverConfig {
            output_directory: resolved_output_directory(
                file.output_directory,
                default_output_directory,
            ),
            grass_ids: convert.grass_ids.unwrap_or_else(default::grass_ids),
            exclude: convert.exclude.unwrap_or_else(default::exclude),
            ignored_plugins: convert
                .ignored_plugins
                .unwrap_or_else(default::ignored_plugins),
            dry_run: convert.dry_run,
            validate_config: false,
            debug: convert.debug,
            auto_enable: convert.auto_enable,
            include_set: RegexSet::empty(),
            exclude_set: RegexSet::empty(),
            ignored_plugin_set: RegexSet::empty(),
        })
    }
}

fn resolved_output_directory(
    configured_output_directory: Option<PathBuf>,
    default_output_directory: PathBuf,
) -> PathBuf {
    let Some(configured_output_directory) = configured_output_directory else {
        return default_output_directory;
    };

    // Early versions of greenmote wrote the platform default data-local path into generated TOML.
    // Treat that exact value as a generated default, not a user override, so profile-specific
    // `data-local=` keeps owning the output directory.
    if paths_equal(
        &configured_output_directory,
        &openmw_config::default_data_local_path(),
    ) && !paths_equal(&configured_output_directory, &default_output_directory)
    {
        default_output_directory
    } else {
        configured_output_directory
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}
