use std::path::PathBuf;

use regex::RegexSet;
use serde::{Deserialize, Serialize};

use crate::{
    groundcover::{GroundcoverConfig, default, openmw},
    unclip::config::PersistedUnclipConfig,
};

use super::to_io_error;

#[derive(Debug, Deserialize, Serialize)]
// Mirrors the public TOML schema so we can distinguish an omitted output directory from one the user
// intentionally set. Convert-specific knobs live under `[convert]`; root-level command knobs are
// not supported because this tool is still wet paint, not a museum.
pub(super) struct GroundcoverConfigFile {
    #[serde(default, skip_serializing)]
    validate_config: Option<bool>,

    #[serde(default)]
    convert: ConvertConfigFile,

    #[serde(default)]
    unclip: PersistedUnclipConfig,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
// Mirrors persisted convert options. CLI-only switches do not belong here; writing transient
// command mode into TOML is how a config file starts lying to its owner.
#[allow(clippy::struct_excessive_bools)]
struct ConvertConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fallback_output_directory: Option<PathBuf>,

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
            validate_config: None,
            convert: ConvertConfigFile {
                fallback_output_directory: config.fallback_output_directory.clone(),
                grass_ids: Some(config.grass_ids.clone()),
                exclude: Some(config.exclude.clone()),
                ignored_plugins: Some(config.ignored_plugins.clone()),
                dry_run: config.dry_run,
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
        if file.validate_config.is_some() {
            return Err(to_io_error("validate_config is a CLI-only option"));
        }
        let convert = file.convert;
        let unclip = if is_empty_unclip_config(&file.unclip) {
            PersistedUnclipConfig::generated_default()
        } else {
            file.unclip
        };

        let fallback_output_directory = convert.fallback_output_directory.clone();
        let output_directory =
            resolved_output_directory(openmw_output_directory, fallback_output_directory.clone());

        Ok(GroundcoverConfig {
            output_directory: output_directory.path.unwrap_or_default(),
            fallback_output_directory,
            output_directory_source: output_directory.source,
            openmw_cfg: openmw_cfg_override,
            grass_ids: convert.grass_ids.unwrap_or_else(default::grass_ids),
            exclude: convert.exclude.unwrap_or_else(default::exclude),
            ignored_plugins: convert
                .ignored_plugins
                .unwrap_or_else(default::ignored_plugins),
            dry_run: convert.dry_run,
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

fn is_empty_unclip_config(config: &PersistedUnclipConfig) -> bool {
    config.plugin.is_none()
        && config.instances.is_none()
        && config.structured.is_none()
        && config.write.is_none()
        && config.write_actions.is_none()
        && config.contact_epsilon.is_none()
        && config.origin_epsilon.is_none()
        && config.relocation_step.is_none()
        && config.relocation_steps.is_none()
        && config.orientation_epsilon.is_none()
        && config.include_grass_ids.is_none()
        && config.exclude_grass_ids.is_none()
        && config.include_occluder_ids.is_none()
        && config.exclude_occluder_ids.is_none()
}

fn resolved_output_directory(
    openmw_output_directory: openmw::ConvertOutputDirectory,
    fallback_output_directory: Option<PathBuf>,
) -> openmw::ConvertOutputDirectory {
    if openmw_output_directory.source != openmw::ConvertOutputDirectorySource::MissingFallback {
        return openmw_output_directory;
    }

    fallback_output_directory.map_or(openmw_output_directory, |path| {
        openmw::ConvertOutputDirectory {
            path: Some(path),
            source: openmw::ConvertOutputDirectorySource::ConfiguredFallback,
        }
    })
}
