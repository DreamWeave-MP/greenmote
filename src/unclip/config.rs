use std::{fs::read_to_string, io, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::groundcover::{DEFAULT_CONFIG_NAME, openmw};

use super::{
    UnclipArgs,
    args::{
        DEFAULT_ORIENTATION_EPSILON_DEGREES, DEFAULT_RELOCATION_STEP, DEFAULT_RELOCATION_STEPS,
        WriteActionArg, default_write_actions,
    },
    model::{CONTACT_TERRAIN_EPSILON, ORIGIN_TERRAIN_EPSILON},
};

#[derive(Clone, Debug)]
pub(crate) struct UnclipConfig {
    pub(crate) openmw_cfg: Option<PathBuf>,
    pub(crate) plugin: PathBuf,
    pub(crate) instances: bool,
    pub(crate) structured: bool,
    pub(crate) write: bool,
    pub(crate) write_actions: Vec<WriteActionArg>,
    pub(crate) contact_epsilon: f32,
    pub(crate) origin_epsilon: f32,
    pub(crate) relocation_step: f32,
    pub(crate) relocation_steps: u16,
    pub(crate) orientation_epsilon: f32,
    pub(crate) include_grass_ids: Vec<String>,
    pub(crate) exclude_grass_ids: Vec<String>,
    pub(crate) include_occluder_ids: Vec<String>,
    pub(crate) exclude_occluder_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersistedUnclipConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plugin: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instances: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) structured: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) write: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) write_actions: Option<Vec<WriteActionArg>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) contact_epsilon: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) origin_epsilon: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) relocation_step: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) relocation_steps: Option<u16>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) orientation_epsilon: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) include_grass_ids: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exclude_grass_ids: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) include_occluder_ids: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exclude_occluder_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct UnclipConfigRoot {
    #[serde(default)]
    unclip: PersistedUnclipConfig,
}

impl UnclipConfig {
    pub(crate) fn get(
        cli_openmw_cfg: Option<&std::path::Path>,
        args: &UnclipArgs,
    ) -> io::Result<Self> {
        let discovery_config = openmw::load_config_from_path(cli_openmw_cfg)?;
        let config_path = discovery_config
            .user_config_path()
            .join(DEFAULT_CONFIG_NAME);
        let configured_openmw_cfg = crate::groundcover::configured_openmw_cfg(&config_path)?;
        let runtime_openmw_cfg = cli_openmw_cfg.or(configured_openmw_cfg.as_deref());
        let persisted_openmw_cfg = openmw::resolved_config_path(runtime_openmw_cfg)?;
        let persisted = match std::fs::symlink_metadata(&config_path) {
            Ok(_) => PersistedUnclipConfig::from_toml(&read_to_string(config_path)?)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                PersistedUnclipConfig::default()
            }
            Err(error) => return Err(error),
        };

        let config = Self::merge(args, persisted, Some(persisted_openmw_cfg))?;
        config.validate()?;

        Ok(config)
    }

    pub(crate) fn merge(
        args: &UnclipArgs,
        persisted: PersistedUnclipConfig,
        openmw_cfg: Option<PathBuf>,
    ) -> io::Result<Self> {
        let plugin = args.plugin.clone().or(persisted.plugin).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "unclip requires --plugin or [unclip].plugin in greenmote.toml",
            )
        })?;

        Ok(Self {
            openmw_cfg,
            plugin,
            instances: args.instances.or(persisted.instances).unwrap_or(false),
            structured: args.structured.or(persisted.structured).unwrap_or(false),
            write: args.write.or(persisted.write).unwrap_or(false),
            write_actions: if args.write_actions.is_empty() {
                persisted
                    .write_actions
                    .unwrap_or_else(default_write_actions)
            } else {
                args.write_actions.clone()
            },
            contact_epsilon: args
                .contact_epsilon
                .or(persisted.contact_epsilon)
                .unwrap_or(CONTACT_TERRAIN_EPSILON),
            origin_epsilon: args
                .origin_epsilon
                .or(persisted.origin_epsilon)
                .unwrap_or(ORIGIN_TERRAIN_EPSILON),
            relocation_step: args
                .relocation_step
                .or(persisted.relocation_step)
                .unwrap_or(DEFAULT_RELOCATION_STEP),
            relocation_steps: args
                .relocation_steps
                .or(persisted.relocation_steps)
                .unwrap_or(DEFAULT_RELOCATION_STEPS),
            orientation_epsilon: args
                .orientation_epsilon
                .or(persisted.orientation_epsilon)
                .unwrap_or(DEFAULT_ORIENTATION_EPSILON_DEGREES),
            include_grass_ids: merge_list(&args.include_grass_ids, persisted.include_grass_ids),
            exclude_grass_ids: merge_list(&args.exclude_grass_ids, persisted.exclude_grass_ids),
            include_occluder_ids: merge_list(
                &args.include_occluder_ids,
                persisted.include_occluder_ids,
            ),
            exclude_occluder_ids: merge_list(
                &args.exclude_occluder_ids,
                persisted.exclude_occluder_ids,
            ),
        })
    }

    fn validate(&self) -> io::Result<()> {
        validate_non_negative_f32("contact_epsilon", self.contact_epsilon)?;
        validate_non_negative_f32("origin_epsilon", self.origin_epsilon)?;
        validate_non_negative_f32("orientation_epsilon", self.orientation_epsilon)?;
        validate_positive_f32("relocation_step", self.relocation_step)?;
        if !(1..=256).contains(&self.relocation_steps) {
            return Err(invalid_config("relocation_steps must be in 1..=256"));
        }
        self.policy().map_err(invalid_config)?;

        Ok(())
    }
}

impl PersistedUnclipConfig {
    pub(crate) fn generated_default() -> Self {
        Self {
            instances: Some(false),
            structured: Some(false),
            write: Some(false),
            write_actions: Some(default_write_actions()),
            contact_epsilon: Some(CONTACT_TERRAIN_EPSILON),
            origin_epsilon: Some(ORIGIN_TERRAIN_EPSILON),
            relocation_step: Some(DEFAULT_RELOCATION_STEP),
            relocation_steps: Some(DEFAULT_RELOCATION_STEPS),
            orientation_epsilon: Some(DEFAULT_ORIENTATION_EPSILON_DEGREES),
            include_grass_ids: Some(Vec::new()),
            exclude_grass_ids: Some(Vec::new()),
            include_occluder_ids: Some(Vec::new()),
            exclude_occluder_ids: Some(Vec::new()),
            ..Self::default()
        }
    }

    pub(crate) fn from_toml(contents: &str) -> io::Result<Self> {
        let root = toml::from_str::<UnclipConfigRoot>(contents).map_err(invalid_config)?;
        Ok(root.unclip)
    }
}

fn merge_list(cli: &[String], persisted: Option<Vec<String>>) -> Vec<String> {
    if cli.is_empty() {
        persisted.unwrap_or_default()
    } else {
        cli.to_vec()
    }
}

fn validate_non_negative_f32(name: &str, value: f32) -> io::Result<()> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(invalid_config(format!(
            "{name} must be a finite non-negative number"
        )))
    }
}

fn validate_positive_f32(name: &str, value: f32) -> io::Result<()> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(invalid_config(format!(
            "{name} must be a finite positive number"
        )))
    }
}

fn invalid_config<E: std::fmt::Display>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::Cli;

    use super::*;
    use crate::Command;

    fn unclip_args(args: &[&str]) -> UnclipArgs {
        let cli = Cli::parse_from(args);
        let Some(Command::Unclip(args)) = cli.command else {
            panic!("expected unclip command");
        };
        args
    }

    #[test]
    fn persisted_plugin_satisfies_missing_cli_plugin() {
        let args = unclip_args(&["greenmote", "unclip"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                plugin: Some(PathBuf::from("configured.omwaddon")),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(config.plugin, PathBuf::from("configured.omwaddon"));
    }

    #[test]
    fn cli_plugin_overrides_persisted_plugin() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                plugin: Some(PathBuf::from("configured.omwaddon")),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(config.plugin, PathBuf::from("cli.omwaddon"));
    }

    #[test]
    fn cli_bool_false_overrides_persisted_write_true() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--write=false",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                write: Some(true),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(!config.write);
    }

    #[test]
    fn persisted_numeric_values_apply_when_cli_omits_them() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                contact_epsilon: Some(1.25),
                origin_epsilon: Some(2.5),
                relocation_step: Some(64.0),
                relocation_steps: Some(12),
                orientation_epsilon: Some(3.5),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_close(config.contact_epsilon, 1.25);
        assert_close(config.origin_epsilon, 2.5);
        assert_close(config.relocation_step, 64.0);
        assert_eq!(config.relocation_steps, 12);
        assert_close(config.orientation_epsilon, 3.5);
    }

    #[test]
    fn cli_numeric_values_override_persisted_values() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--contact-epsilon",
            "1.25",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                contact_epsilon: Some(4.0),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_close(config.contact_epsilon, 1.25);
    }

    #[test]
    fn cli_filter_list_replaces_persisted_list() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--include-grass-id",
            "cli_.*",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                include_grass_ids: Some(vec!["persisted_.*".to_owned()]),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(config.include_grass_ids, vec!["cli_.*"]);
    }

    #[test]
    fn invalid_persisted_regex_fails_validation() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                include_grass_ids: Some(vec!["(".to_owned()]),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(config.validate().is_err());
    }

    #[test]
    fn invalid_persisted_write_action_mix_fails_validation() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                write_actions: Some(vec![WriteActionArg::All, WriteActionArg::TerrainZ]),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(config.validate().is_err());
    }

    #[test]
    fn unknown_unclip_key_fails_toml_parse() {
        let result = PersistedUnclipConfig::from_toml(
            r"
[unclip]
definitely_not_real = true
",
        );

        assert!(result.is_err());
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
