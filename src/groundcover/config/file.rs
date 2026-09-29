// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use regex::RegexSet;
use serde::{Deserialize, Serialize};

use crate::{
    groundcover::{GroundcoverConfig, default, openmw},
    unclip::config::PersistedUnclipConfig,
};

use super::to_io_error;

#[derive(Debug, Deserialize, Serialize)]
// Mirrors the public TOML schema. Convert-specific knobs live under `[convert]`; root-level command
// knobs are not supported because this tool is still wet paint, not a museum.
pub(super) struct GroundcoverConfigFile {
    #[serde(default, rename = "validate_config", skip_serializing)]
    _validate_config: Option<bool>,

    #[serde(default)]
    convert: ConvertConfigFile,

    #[serde(default)]
    unclip: PersistedUnclipConfig,
}

#[derive(Debug, Default, Deserialize, Serialize)]
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
    debug: bool,

    #[serde(default)]
    auto_enable: bool,
}

impl GroundcoverConfigFile {
    pub(super) fn from_runtime(config: &GroundcoverConfig) -> Self {
        Self {
            _validate_config: None,
            convert: ConvertConfigFile {
                grass_ids: Some(config.grass_ids.clone()),
                exclude: Some(config.exclude.clone()),
                ignored_plugins: Some(config.ignored_plugins.clone()),
                debug: config.debug,
                auto_enable: config.auto_enable,
            },
            unclip: config.unclip.clone(),
        }
    }

    pub(super) fn from_toml(
        contents: &str,
        openmw_output_directory: openmw::ConvertOutputDirectory,
        openmw_cfg_override: Option<PathBuf>,
    ) -> std::io::Result<GroundcoverConfig> {
        let file = toml::from_str::<Self>(contents).map_err(to_io_error)?;
        let convert = file.convert;
        let unclip = if is_empty_unclip_config(&file.unclip) {
            PersistedUnclipConfig::generated_default()
        } else {
            file.unclip
        };

        Ok(GroundcoverConfig {
            output_directory: openmw_output_directory.path,
            output_directory_source: openmw_output_directory.source,
            openmw_cfg: openmw_cfg_override,
            grass_ids: convert.grass_ids.unwrap_or_else(default::grass_ids),
            exclude: convert.exclude.unwrap_or_else(default::exclude),
            ignored_plugins: convert
                .ignored_plugins
                .unwrap_or_else(default::ignored_plugins),
            dry_run: false,
            validate_config: false,
            debug: convert.debug,
            auto_enable: convert.auto_enable,
            unclip,
            include_set: RegexSet::empty(),
            exclude_set: RegexSet::empty(),
            ignored_plugin_set: RegexSet::empty(),
        })
    }
}

/// Whether the `[unclip]` table sets nothing this version understands. Every field is named, so a
/// new setting cannot be left out and silently replaced by the generated defaults.
fn is_empty_unclip_config(config: &PersistedUnclipConfig) -> bool {
    let PersistedUnclipConfig {
        plugin,
        actions,
        float_tolerance,
        max_sink,
        max_sink_fraction,
        sink,
        relocation_step,
        relocation_steps,
        orientation_epsilon,
        max_tilt,
        include_grass_ids,
        exclude_grass_ids,
        include_occluder_ids,
        exclude_occluder_ids,
        road_texture_paths,
        unknown: _,
    } = config;

    plugin.is_none()
        && actions.is_none()
        && float_tolerance.is_none()
        && max_sink.is_none()
        && max_sink_fraction.is_none()
        && sink.is_none()
        && relocation_step.is_none()
        && relocation_steps.is_none()
        && orientation_epsilon.is_none()
        && max_tilt.is_none()
        && include_grass_ids.is_none()
        && exclude_grass_ids.is_none()
        && include_occluder_ids.is_none()
        && exclude_occluder_ids.is_none()
        && road_texture_paths.is_none()
}
