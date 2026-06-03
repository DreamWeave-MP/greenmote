// SPDX-License-Identifier: GPL-3.0-only

use std::{
    fs::read_to_string,
    io::{self, BufRead, Write},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    args::{
        DEFAULT_ORIENTATION_EPSILON_DEGREES, DEFAULT_RELOCATION_STEP, DEFAULT_RELOCATION_STEPS,
        WriteActionArg, default_write_actions,
    },
    model::ORIGIN_TERRAIN_EPSILON,
};

#[derive(Clone, Debug)]
pub(crate) struct UnclipConfig {
    pub(crate) openmw_cfg: Option<PathBuf>,
    pub(crate) plugin: PathBuf,
    pub(crate) output_plugin: Option<PathBuf>,
    pub(crate) meshgenerator_ini: Option<PathBuf>,
    pub(crate) verbose: bool,
    pub(crate) structured: bool,
    pub(crate) dry_run: bool,
    pub(crate) write_actions: Vec<WriteActionArg>,
    pub(crate) origin_epsilon: f32,
    pub(crate) relocation_step: f32,
    pub(crate) relocation_steps: u16,
    pub(crate) orientation_epsilon: f32,
    pub(crate) include_grass_ids: Vec<String>,
    pub(crate) exclude_grass_ids: Vec<String>,
    pub(crate) include_occluder_ids: Vec<String>,
    pub(crate) exclude_occluder_ids: Vec<String>,
    pub(crate) include_road_texture_paths: Vec<String>,
    pub(crate) exclude_road_texture_paths: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct PersistedUnclipConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plugin: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) meshgenerator_ini: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) instances: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) verbose: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) structured: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) dry_run: Option<bool>,

    #[serde(default, rename = "write", skip_serializing)]
    pub(crate) legacy_write: Option<bool>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) write_actions: Option<Vec<WriteActionArg>>,

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

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) include_road_texture_paths: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) exclude_road_texture_paths: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct UnclipConfigRoot {
    #[serde(default)]
    unclip: PersistedUnclipConfig,
}

impl UnclipConfig {
    pub(crate) fn get(
        cli_openmw_cfg: Option<&std::path::Path>,
        config_path_override: Option<&std::path::Path>,
        args: &UnclipArgs,
        stdin: &mut dyn BufRead,
        stderr: &mut dyn Write,
    ) -> io::Result<Self> {
        let runtime_config =
            openmw::load_config_with_prompt(cli_openmw_cfg, "unclip", stdin, stderr)?;
        let config_path = openmw::greenmote_config_path(config_path_override, &runtime_config);
        let persisted_openmw_cfg = openmw::persisted_config_path(&runtime_config);
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
            output_plugin: args.output_plugin.clone(),
            meshgenerator_ini: if args.ignore_meshgenerator_ini {
                None
            } else {
                args.meshgenerator_ini
                    .clone()
                    .or(persisted.meshgenerator_ini)
            },
            verbose: args
                .verbose
                .or(args.instances)
                .or(persisted.verbose)
                .or(persisted.instances)
                .unwrap_or(false),
            structured: args.structured.or(persisted.structured).unwrap_or(false),
            dry_run: args.dry_run.or(persisted.dry_run).unwrap_or(false),
            write_actions: if args.write_actions.is_empty() {
                persisted
                    .write_actions
                    .unwrap_or_else(default_write_actions)
            } else {
                args.write_actions.clone()
            },
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
            exclude_occluder_ids: merge_list_with_default(
                &args.exclude_occluder_ids,
                persisted.exclude_occluder_ids,
                default_tree_occluder_exclude_ids(),
            ),
            include_road_texture_paths: merge_list(
                &args.include_road_texture_paths,
                persisted.include_road_texture_paths,
            ),
            exclude_road_texture_paths: merge_list(
                &args.exclude_road_texture_paths,
                persisted.exclude_road_texture_paths,
            ),
        })
    }

    fn validate(&self) -> io::Result<()> {
        validate_non_negative_f32("origin_epsilon", self.origin_epsilon)?;
        validate_non_negative_f32("orientation_epsilon", self.orientation_epsilon)?;
        validate_positive_f32("relocation_step", self.relocation_step)?;
        if !(1..=256).contains(&self.relocation_steps) {
            return Err(invalid_config("relocation_steps must be in 1..=256"));
        }
        if let Some(output_plugin) = &self.output_plugin {
            validate_output_plugin(output_plugin)?;
        }
        self.policy().map_err(invalid_config)?;

        Ok(())
    }
}

impl PersistedUnclipConfig {
    pub(crate) fn generated_default() -> Self {
        Self {
            verbose: Some(false),
            structured: Some(false),
            dry_run: Some(false),
            write_actions: Some(default_write_actions()),
            origin_epsilon: Some(ORIGIN_TERRAIN_EPSILON),
            relocation_step: Some(DEFAULT_RELOCATION_STEP),
            relocation_steps: Some(DEFAULT_RELOCATION_STEPS),
            orientation_epsilon: Some(DEFAULT_ORIENTATION_EPSILON_DEGREES),
            include_grass_ids: Some(Vec::new()),
            exclude_grass_ids: Some(Vec::new()),
            include_occluder_ids: Some(Vec::new()),
            exclude_occluder_ids: Some(default_tree_occluder_exclude_ids()),
            include_road_texture_paths: Some(Vec::new()),
            exclude_road_texture_paths: Some(Vec::new()),
            ..Self::default()
        }
    }

    pub(crate) fn is_generated_default(&self) -> bool {
        self == &Self::generated_default()
    }

    pub(crate) fn from_toml(contents: &str) -> io::Result<Self> {
        let root = toml::from_str::<UnclipConfigRoot>(contents).map_err(invalid_config)?;
        root.unclip.normalize_legacy_write()
    }

    pub(crate) fn normalize_legacy_write(mut self) -> io::Result<Self> {
        let Some(write) = self.legacy_write.take() else {
            return Ok(self);
        };
        let legacy_dry_run = !write;
        if let Some(dry_run) = self.dry_run {
            if dry_run != legacy_dry_run {
                return Err(invalid_config(
                    "[unclip].write conflicts with [unclip].dry_run; remove the deprecated write key",
                ));
            }
        } else {
            self.dry_run = Some(legacy_dry_run);
        }

        Ok(self)
    }
}

fn merge_list(cli: &[String], persisted: Option<Vec<String>>) -> Vec<String> {
    if cli.is_empty() {
        persisted.unwrap_or_default()
    } else {
        cli.to_vec()
    }
}

fn merge_list_with_default(
    cli: &[String],
    persisted: Option<Vec<String>>,
    default: Vec<String>,
) -> Vec<String> {
    if cli.is_empty() {
        persisted.unwrap_or(default)
    } else {
        cli.to_vec()
    }
}

pub(crate) fn default_tree_occluder_exclude_ids() -> Vec<String> {
    vec![
        "flora_(tree|ashtree|treestump|treedead|root)_.*".to_owned(),
        "flora_(ash_)?log_.*".to_owned(),
        "flora_bm_(treebranch|treestump|snowbranch|snowstump|(snow_)?log)_.*".to_owned(),
        "flora_bc_(tree|knee|log)_.*".to_owned(),
        "ex_t_(bigroot|root).*".to_owned(),
        "t_.*flora.*(tree|branch|root|stump|log|palm).*".to_owned(),
        "t_cyr_flora(gc|str)_bush_.*".to_owned(),
    ]
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

fn validate_output_plugin(path: &std::path::Path) -> io::Result<()> {
    if path.file_name().is_none() {
        return Err(invalid_config(format!(
            "output_plugin {} must include a filename",
            path.display()
        )));
    }
    if path.is_dir() {
        return Err(invalid_config(format!(
            "output_plugin {} must not be an existing directory",
            path.display()
        )));
    }

    Ok(())
}

fn invalid_config<E: std::fmt::Display>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use clap::Parser;

    use crate::Cli;

    use super::*;
    use crate::Command;

    fn unclip_args(args: &[&str]) -> UnclipArgs {
        let cli = Cli::parse_from(args);
        let Some(Command::Unclip(args)) = cli.command else {
            panic!("expected unclip command");
        };
        *args
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
    fn cli_output_plugin_merges_into_runtime_config_only() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--output-plugin",
            "patched/cli.omwaddon",
        ]);
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();

        assert_eq!(
            config.output_plugin,
            Some(PathBuf::from("patched/cli.omwaddon"))
        );
        assert!(
            !toml::to_string(&PersistedUnclipConfig::generated_default())
                .unwrap()
                .contains("output_plugin")
        );
    }

    #[test]
    fn output_plugin_validation_rejects_existing_directory() {
        let output_dir = std::env::temp_dir().join(format!(
            "greenmote-output-plugin-dir-{}",
            std::process::id()
        ));
        fs::create_dir_all(&output_dir).unwrap();

        let mut args = unclip_args(&["greenmote", "unclip", "--plugin", "input.omwaddon"]);
        args.output_plugin = Some(output_dir.clone());
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();

        let error = config.validate().unwrap_err();

        fs::remove_dir(&output_dir).unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .contains("must not be an existing directory")
        );
    }

    #[test]
    fn output_plugin_validation_rejects_path_without_filename() {
        let mut args = unclip_args(&["greenmote", "unclip", "--plugin", "input.omwaddon"]);
        args.output_plugin = Some(PathBuf::new());
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();

        let error = config.validate().unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("must include a filename"));
    }

    #[test]
    fn cli_meshgenerator_ini_overrides_persisted_path() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--meshgenerator-ini",
            "cli.ini",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                meshgenerator_ini: Some(PathBuf::from("persisted.ini")),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(config.meshgenerator_ini, Some(PathBuf::from("cli.ini")));
    }

    #[test]
    fn persisted_meshgenerator_ini_satisfies_missing_cli_path() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                meshgenerator_ini: Some(PathBuf::from("persisted.ini")),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(
            config.meshgenerator_ini,
            Some(PathBuf::from("persisted.ini"))
        );
    }

    #[test]
    fn internal_ignore_meshgenerator_ini_suppresses_persisted_path() {
        let mut args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        args.ignore_meshgenerator_ini = true;
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                meshgenerator_ini: Some(PathBuf::from("persisted.ini")),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_eq!(config.meshgenerator_ini, None);
    }

    #[test]
    fn placement_model_cli_flag_is_unknown() {
        let result = Cli::try_parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--placement-model",
            "contact",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn stale_unknown_keys_are_ignored() {
        let result = PersistedUnclipConfig::from_toml(
            r#"
[unclip]
placement_model = "contact"
contact_epsilon = 99.0
origin_epsilon = 2.5
"#,
        )
        .unwrap();

        assert_close(result.origin_epsilon.unwrap(), 2.5);
    }

    #[test]
    fn cli_dry_run_false_overrides_persisted_dry_run_true() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--dry-run=false",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                dry_run: Some(true),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(!config.dry_run);
    }

    #[test]
    fn cli_dry_run_defaults_to_false_and_flag_enables_it() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();

        assert!(!config.dry_run);

        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--dry-run",
        ]);
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();

        assert!(config.dry_run);
    }

    #[test]
    fn persisted_dry_run_true_applies_when_cli_omits_it() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                dry_run: Some(true),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(config.dry_run);
    }

    #[test]
    fn generated_default_toml_uses_dry_run_not_write() {
        let contents = toml::to_string(&PersistedUnclipConfig::generated_default()).unwrap();

        assert!(contents.contains("dry_run = false"));
        assert!(!contents.contains("write = false"));
    }

    #[test]
    fn legacy_persisted_write_maps_to_dry_run() {
        let inspect = PersistedUnclipConfig::from_toml(
            r"
[unclip]
write = false
",
        )
        .unwrap();
        let write = PersistedUnclipConfig::from_toml(
            r"
[unclip]
write = true
",
        )
        .unwrap();

        assert_eq!(inspect.dry_run, Some(true));
        assert_eq!(write.dry_run, Some(false));
        assert_eq!(inspect.legacy_write, None);
        assert_eq!(write.legacy_write, None);
    }

    #[test]
    fn conflicting_legacy_write_and_dry_run_fails() {
        let error = PersistedUnclipConfig::from_toml(
            r"
[unclip]
write = false
dry_run = false
",
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("conflicts"));
    }

    #[test]
    fn old_write_cli_flag_is_unknown() {
        let result =
            Cli::try_parse_from(["greenmote", "unclip", "--plugin", "cli.omwaddon", "--write"]);

        assert!(result.is_err());
    }

    #[test]
    fn cli_verbose_overrides_persisted_instances_alias() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--verbose=false",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                instances: Some(true),
                verbose: Some(true),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(!config.verbose);
    }

    #[test]
    fn deprecated_instances_alias_sets_verbose() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "cli.omwaddon",
            "--instances",
        ]);
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();

        assert!(config.verbose);
    }

    #[test]
    fn persisted_instances_alias_sets_verbose_for_compatibility() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                instances: Some(true),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(config.verbose);
    }

    #[test]
    fn persisted_numeric_values_apply_when_cli_omits_them() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                origin_epsilon: Some(2.5),
                relocation_step: Some(64.0),
                relocation_steps: Some(12),
                orientation_epsilon: Some(3.5),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

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
            "--origin-epsilon",
            "1.25",
        ]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                origin_epsilon: Some(4.0),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert_close(config.origin_epsilon, 1.25);
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
    fn default_occluder_filter_excludes_tree_like_statics() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();
        let policy = config.policy().unwrap();

        assert_eq!(
            config.exclude_occluder_ids,
            default_tree_occluder_exclude_ids()
        );
        assert!(!policy.occluder_filter.includes("flora_tree_wg_01"));
        assert!(!policy.occluder_filter.includes("T_Sky_Flora_TreePine1_01"));
        assert!(!policy.occluder_filter.includes("T_Cyr_FloraGC_Bush_01"));
        assert!(policy.occluder_filter.includes("ex_common_rock_01"));
    }

    #[test]
    fn persisted_empty_occluder_filter_overrides_default_tree_exclusions() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                exclude_occluder_ids: Some(Vec::new()),
                ..PersistedUnclipConfig::default()
            },
            None,
        )
        .unwrap();

        assert!(config.exclude_occluder_ids.is_empty());
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
    fn invalid_persisted_road_texture_regex_fails_validation() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "cli.omwaddon"]);
        let config = UnclipConfig::merge(
            &args,
            PersistedUnclipConfig {
                include_road_texture_paths: Some(vec!["(".to_owned()]),
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
    fn unknown_unclip_key_is_ignored() {
        let result = PersistedUnclipConfig::from_toml(
            r"
[unclip]
definitely_not_real = true
origin_epsilon = 1.25
",
        )
        .unwrap();

        assert_close(result.origin_epsilon.unwrap(), 1.25);
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
