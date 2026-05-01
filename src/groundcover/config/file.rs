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

    #[serde(default)]
    convert: ConvertConfigFile,
}

#[derive(Debug, Default, Deserialize, Serialize)]
// These are persisted/CLI-facing runtime toggles. Hiding them behind enums would make the Rust
// type prettier and the TOML schema worse. That is not a trade.
#[allow(clippy::struct_excessive_bools)]
struct ConvertConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groundcover_output: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    deleted_output: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    grass_ids: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    exclude: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    ignored_plugins: Option<Vec<String>>,

    #[serde(default)]
    dry_run: bool,

    #[serde(default)]
    validate_config: bool,

    #[serde(default)]
    debug: bool,

    #[serde(default)]
    auto_enable: bool,
}

impl GroundcoverConfigFile {
    pub(super) fn from_runtime(config: &GroundcoverConfig) -> Self {
        Self {
            output_directory: Some(config.output_directory.clone()),
            convert: ConvertConfigFile {
                groundcover_output: Some(config.groundcover_output.clone()),
                deleted_output: Some(config.deleted_output.clone()),
                grass_ids: Some(config.grass_ids.clone()),
                exclude: Some(config.exclude.clone()),
                ignored_plugins: Some(config.ignored_plugins.clone()),
                dry_run: config.dry_run,
                validate_config: config.validate_config,
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
        let convert = file.convert;

        Ok(GroundcoverConfig {
            output_directory: resolved_output_directory(
                file.output_directory,
                default_output_directory,
            ),
            groundcover_output: convert
                .groundcover_output
                .unwrap_or_else(default::groundcover_output),
            deleted_output: convert
                .deleted_output
                .unwrap_or_else(default::deleted_output),
            grass_ids: convert.grass_ids.unwrap_or_else(default::grass_ids),
            exclude: convert.exclude.unwrap_or_else(default::exclude),
            ignored_plugins: convert
                .ignored_plugins
                .unwrap_or_else(default::ignored_plugins),
            dry_run: convert.dry_run,
            validate_config: convert.validate_config,
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
