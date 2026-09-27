// SPDX-License-Identifier: GPL-3.0-only

//! Persisted `[unclip]` settings and their merge with command-line arguments.

use std::{
    collections::BTreeMap,
    fs::read_to_string,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    args::{
        ActionArg, Actions, DEFAULT_FLOAT_TOLERANCE, DEFAULT_MAX_SINK, DEFAULT_MAX_SINK_FRACTION,
        DEFAULT_ORIENTATION_EPSILON_DEGREES, DEFAULT_RELOCATION_STEP, DEFAULT_RELOCATION_STEPS,
        DEFAULT_SINK, IdFilter, RelocationPolicy, RoadTextureFilter, UnclipPolicy, default_actions,
        default_road_texture_path_patterns, default_tree_occluder_exclude_ids,
    },
};

/// Effective unclip configuration after merging CLI arguments over `greenmote.toml`.
#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct UnclipConfig {
    pub(crate) openmw_cfg: Option<PathBuf>,
    pub(crate) plugin: PathBuf,
    pub(crate) output_plugin: Option<PathBuf>,
    pub(crate) write: bool,
    pub(crate) verbose: bool,
    pub(crate) structured: bool,
    pub(crate) ignore_missing_meshes: bool,
    pub(crate) actions: Vec<ActionArg>,
    pub(crate) float_tolerance: f32,
    pub(crate) max_sink: f32,
    pub(crate) max_sink_fraction: f32,
    pub(crate) sink: f32,
    pub(crate) relocation_step: f32,
    pub(crate) relocation_steps: u16,
    pub(crate) orientation_epsilon: f32,
    pub(crate) include_grass_ids: Vec<String>,
    pub(crate) exclude_grass_ids: Vec<String>,
    pub(crate) include_occluder_ids: Vec<String>,
    pub(crate) exclude_occluder_ids: Vec<String>,
    pub(crate) road_texture_paths: Vec<String>,
}

/// The `[unclip]` table of `greenmote.toml`.
///
/// Only policy values are persisted. Runtime choices such as `--write`,
/// `--output-plugin`, `--verbose`, and `--structured` are never read from the file.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub(crate) struct PersistedUnclipConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) plugin: Option<PathBuf>,

    #[serde(
        default,
        alias = "write_actions",
        skip_serializing_if = "Option::is_none"
    )]
    pub(crate) actions: Option<Vec<ActionArg>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) float_tolerance: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_sink: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) max_sink_fraction: Option<f32>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) sink: Option<f32>,

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
    pub(crate) road_texture_paths: Option<Vec<String>>,

    /// Keys this version does not understand. They are reported as warnings and dropped on save.
    #[serde(default, flatten, skip_serializing)]
    pub(crate) unknown: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize)]
struct UnclipConfigRoot {
    #[serde(default)]
    unclip: PersistedUnclipConfig,
}

impl UnclipConfig {
    pub(crate) fn get(
        cli_openmw_cfg: Option<&Path>,
        config_path_override: Option<&Path>,
        args: &UnclipArgs,
        stdin: &mut dyn BufRead,
        stderr: &mut dyn Write,
    ) -> io::Result<Self> {
        let runtime_config =
            openmw::load_config_with_prompt(cli_openmw_cfg, "unclip", stdin, stderr)?;
        let config_path = openmw::greenmote_config_path(config_path_override, &runtime_config);
        let persisted_openmw_cfg = openmw::persisted_config_path(&runtime_config);
        let persisted = match std::fs::symlink_metadata(&config_path) {
            Ok(_) => PersistedUnclipConfig::from_toml(&read_to_string(&config_path)?)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                PersistedUnclipConfig::default()
            }
            Err(error) => return Err(error),
        };
        if !persisted.unknown.is_empty() {
            writeln!(
                stderr,
                "warning: ignoring unknown [unclip] keys in {}: {}",
                config_path.display(),
                persisted
                    .unknown
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )?;
        }

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
            write: args.write.unwrap_or(false),
            verbose: args.verbose.unwrap_or(false),
            structured: args.structured.unwrap_or(false),
            ignore_missing_meshes: args.ignore_missing_meshes.unwrap_or(false),
            actions: if args.actions.is_empty() {
                persisted.actions.unwrap_or_else(default_actions)
            } else {
                args.actions.clone()
            },
            float_tolerance: args
                .float_tolerance
                .or(persisted.float_tolerance)
                .unwrap_or(DEFAULT_FLOAT_TOLERANCE),
            max_sink: args
                .max_sink
                .or(persisted.max_sink)
                .unwrap_or(DEFAULT_MAX_SINK),
            max_sink_fraction: args
                .max_sink_fraction
                .or(persisted.max_sink_fraction)
                .unwrap_or(DEFAULT_MAX_SINK_FRACTION),
            sink: args.sink.or(persisted.sink).unwrap_or(DEFAULT_SINK),
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
            include_grass_ids: replace_list(&args.include_grass_ids, persisted.include_grass_ids),
            exclude_grass_ids: replace_list(&args.exclude_grass_ids, persisted.exclude_grass_ids),
            include_occluder_ids: replace_list(
                &args.include_occluder_ids,
                persisted.include_occluder_ids,
            ),
            exclude_occluder_ids: extend_list(
                &args.exclude_occluder_ids,
                persisted.exclude_occluder_ids,
                default_tree_occluder_exclude_ids,
                args.no_default_occluder_excludes,
            ),
            road_texture_paths: extend_list(
                &args.road_texture_paths,
                persisted.road_texture_paths,
                default_road_texture_path_patterns,
                args.no_default_road_textures,
            ),
        })
    }

    pub(crate) fn policy(&self) -> Result<UnclipPolicy, String> {
        Ok(UnclipPolicy {
            actions: Actions::from_args(&self.actions),
            float_tolerance: self.float_tolerance,
            max_sink: self.max_sink,
            max_sink_fraction: self.max_sink_fraction,
            sink: self.sink,
            orientation_epsilon_degrees: self.orientation_epsilon,
            relocation: RelocationPolicy {
                step: self.relocation_step,
                steps: self.relocation_steps,
            },
            target_filter: IdFilter::new(&self.include_grass_ids, &self.exclude_grass_ids)
                .map_err(|error| format!("invalid grass id filter: {error}"))?,
            occluder_filter: IdFilter::new(&self.include_occluder_ids, &self.exclude_occluder_ids)
                .map_err(|error| format!("invalid occluder id filter: {error}"))?,
            road_texture_filter: RoadTextureFilter::new(&self.road_texture_paths)
                .map_err(|error| format!("invalid road texture path filter: {error}"))?,
        })
    }

    fn validate(&self) -> io::Result<()> {
        validate_non_negative_f32("float_tolerance", self.float_tolerance)?;
        validate_non_negative_f32("max_sink", self.max_sink)?;
        validate_non_negative_f32("max_sink_fraction", self.max_sink_fraction)?;
        if self.max_sink_fraction > 1.0 {
            return Err(invalid_config("max_sink_fraction must be between 0 and 1"));
        }
        validate_non_negative_f32("sink", self.sink)?;
        validate_non_negative_f32("orientation_epsilon", self.orientation_epsilon)?;
        validate_positive_f32("relocation_step", self.relocation_step)?;
        if self.sink > self.max_sink {
            return Err(invalid_config(format!(
                "sink ({}) must not exceed max_sink ({})",
                self.sink, self.max_sink
            )));
        }
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
            actions: Some(default_actions()),
            float_tolerance: Some(DEFAULT_FLOAT_TOLERANCE),
            max_sink: Some(DEFAULT_MAX_SINK),
            max_sink_fraction: Some(DEFAULT_MAX_SINK_FRACTION),
            sink: Some(DEFAULT_SINK),
            relocation_step: Some(DEFAULT_RELOCATION_STEP),
            relocation_steps: Some(DEFAULT_RELOCATION_STEPS),
            orientation_epsilon: Some(DEFAULT_ORIENTATION_EPSILON_DEGREES),
            include_grass_ids: Some(Vec::new()),
            exclude_grass_ids: Some(Vec::new()),
            include_occluder_ids: Some(Vec::new()),
            exclude_occluder_ids: Some(default_tree_occluder_exclude_ids()),
            road_texture_paths: Some(default_road_texture_path_patterns()),
            ..Self::default()
        }
    }

    pub(crate) fn from_toml(contents: &str) -> io::Result<Self> {
        let root = toml::from_str::<UnclipConfigRoot>(contents).map_err(invalid_config)?;
        Ok(root.unclip)
    }
}

fn replace_list(cli: &[String], persisted: Option<Vec<String>>) -> Vec<String> {
    if cli.is_empty() {
        persisted.unwrap_or_default()
    } else {
        cli.to_vec()
    }
}

fn extend_list(
    cli: &[String],
    persisted: Option<Vec<String>>,
    builtin: fn() -> Vec<String>,
    no_default: bool,
) -> Vec<String> {
    let mut list = if no_default {
        Vec::new()
    } else {
        persisted.unwrap_or_else(builtin)
    };
    for pattern in cli {
        if !list.contains(pattern) {
            list.push(pattern.clone());
        }
    }
    list
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

fn validate_output_plugin(path: &Path) -> io::Result<()> {
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
#[allow(clippy::float_cmp)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use crate::{Cli, Command};

    use super::*;

    fn unclip_args(args: &[&str]) -> UnclipArgs {
        let cli = Cli::parse_from(args);
        let Some(Command::Unclip(args)) = cli.command else {
            panic!("expected unclip command");
        };
        *args
    }

    fn merged(args: &[&str], persisted: PersistedUnclipConfig) -> UnclipConfig {
        UnclipConfig::merge(&unclip_args(args), persisted, None).unwrap()
    }

    #[test]
    fn persisted_plugin_satisfies_missing_cli_plugin() {
        let config = merged(
            &["greenmote", "unclip"],
            PersistedUnclipConfig {
                plugin: Some(PathBuf::from("configured.omwaddon")),
                ..PersistedUnclipConfig::default()
            },
        );
        assert_eq!(config.plugin, PathBuf::from("configured.omwaddon"));

        let config = merged(
            &["greenmote", "unclip", "--plugin", "cli.omwaddon"],
            PersistedUnclipConfig {
                plugin: Some(PathBuf::from("configured.omwaddon")),
                ..PersistedUnclipConfig::default()
            },
        );
        assert_eq!(config.plugin, PathBuf::from("cli.omwaddon"));
    }

    #[test]
    fn missing_plugin_everywhere_is_an_error() {
        let error = UnclipConfig::merge(
            &unclip_args(&["greenmote", "unclip"]),
            PersistedUnclipConfig::default(),
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_defaults_to_dry_run() {
        let config = merged(
            &["greenmote", "unclip", "-p", "x.esp"],
            PersistedUnclipConfig::default(),
        );
        assert!(!config.write);

        let config = merged(
            &["greenmote", "unclip", "-p", "x.esp", "--write"],
            PersistedUnclipConfig::default(),
        );
        assert!(config.write);
    }

    #[test]
    fn runtime_keys_are_never_persisted_or_read() {
        let toml = toml::to_string(&PersistedUnclipConfig::generated_default()).unwrap();
        for key in ["write", "output_plugin", "verbose", "structured", "dry_run"] {
            assert!(
                !toml.contains(key),
                "{key} leaked into generated toml:\n{toml}"
            );
        }

        let persisted = PersistedUnclipConfig::from_toml(
            "[unclip]\nplugin = 'x.esp'\nwrite = true\ndry_run = false\n",
        )
        .unwrap();
        assert_eq!(
            persisted.unknown.keys().cloned().collect::<Vec<_>>(),
            vec!["dry_run".to_owned(), "write".to_owned()]
        );
        let config = merged(&["greenmote", "unclip"], persisted);
        assert!(!config.write);
    }

    #[test]
    fn legacy_write_actions_key_is_accepted() {
        let persisted =
            PersistedUnclipConfig::from_toml("[unclip]\nwrite_actions = ['orient']\n").unwrap();
        assert_eq!(persisted.actions, Some(vec![ActionArg::Orient]));
        assert!(persisted.unknown.is_empty());
    }

    #[test]
    fn cli_actions_replace_persisted_actions() {
        let persisted = PersistedUnclipConfig {
            plugin: Some(PathBuf::from("x.esp")),
            actions: Some(vec![ActionArg::Orient]),
            ..PersistedUnclipConfig::default()
        };
        let config = merged(&["greenmote", "unclip"], persisted.clone());
        assert_eq!(config.actions, vec![ActionArg::Orient]);

        let config = merged(
            &["greenmote", "unclip", "--actions", "terrain-z"],
            persisted,
        );
        assert_eq!(config.actions, vec![ActionArg::TerrainZ]);

        let config = merged(
            &["greenmote", "unclip", "-p", "x.esp"],
            PersistedUnclipConfig::default(),
        );
        assert_eq!(config.actions, default_actions());
    }

    #[test]
    fn numeric_values_merge_cli_over_persisted_over_defaults() {
        let persisted = PersistedUnclipConfig {
            plugin: Some(PathBuf::from("x.esp")),
            float_tolerance: Some(3.0),
            max_sink: Some(20.0),
            relocation_steps: Some(4),
            ..PersistedUnclipConfig::default()
        };
        let config = merged(&["greenmote", "unclip"], persisted.clone());
        assert_eq!(config.float_tolerance, 3.0);
        assert_eq!(config.max_sink, 20.0);
        assert_eq!(config.sink, DEFAULT_SINK);
        assert_eq!(config.relocation_steps, 4);

        let config = merged(
            &[
                "greenmote",
                "unclip",
                "--float-tolerance",
                "0.25",
                "--relocation-steps",
                "12",
            ],
            persisted,
        );
        assert_eq!(config.float_tolerance, 0.25);
        assert_eq!(config.relocation_steps, 12);
    }

    #[test]
    fn sink_larger_than_max_sink_fails_validation() {
        let config = merged(
            &[
                "greenmote",
                "unclip",
                "-p",
                "x.esp",
                "--sink",
                "9",
                "--max-sink",
                "8",
            ],
            PersistedUnclipConfig::default(),
        );
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("max_sink")
        );
    }

    #[test]
    fn output_plugin_validation_rejects_bad_paths() {
        let output_dir = std::env::temp_dir().join(format!(
            "greenmote-output-plugin-dir-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&output_dir).unwrap();
        let mut args = unclip_args(&["greenmote", "unclip", "--plugin", "input.omwaddon"]);
        args.output_plugin = Some(output_dir.clone());
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();
        let error = config.validate().unwrap_err();
        std::fs::remove_dir(&output_dir).unwrap();
        assert!(
            error
                .to_string()
                .contains("must not be an existing directory")
        );

        args.output_plugin = Some(PathBuf::new());
        let config = UnclipConfig::merge(&args, PersistedUnclipConfig::default(), None).unwrap();
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("must include a filename")
        );
    }

    #[test]
    fn grass_filters_replace_but_default_lists_extend() {
        let persisted = PersistedUnclipConfig {
            plugin: Some(PathBuf::from("x.esp")),
            include_grass_ids: Some(vec!["a".to_owned()]),
            ..PersistedUnclipConfig::default()
        };
        let config = merged(
            &["greenmote", "unclip", "--include-grass-id", "b"],
            persisted,
        );
        assert_eq!(config.include_grass_ids, vec!["b".to_owned()]);

        let config = merged(
            &[
                "greenmote",
                "unclip",
                "-p",
                "x.esp",
                "--exclude-occluder-id",
                "custom_.*",
            ],
            PersistedUnclipConfig::default(),
        );
        let mut expected = default_tree_occluder_exclude_ids();
        expected.push("custom_.*".to_owned());
        assert_eq!(config.exclude_occluder_ids, expected);

        let config = merged(
            &[
                "greenmote",
                "unclip",
                "-p",
                "x.esp",
                "--no-default-road-textures",
                "--road-texture-path",
                ".*mine.*",
            ],
            PersistedUnclipConfig::default(),
        );
        assert_eq!(config.road_texture_paths, vec![".*mine.*".to_owned()]);
    }

    #[test]
    fn persisted_lists_replace_builtin_defaults() {
        let persisted = PersistedUnclipConfig {
            plugin: Some(PathBuf::from("x.esp")),
            exclude_occluder_ids: Some(Vec::new()),
            road_texture_paths: Some(vec![".*only.*".to_owned()]),
            ..PersistedUnclipConfig::default()
        };
        let config = merged(&["greenmote", "unclip"], persisted);
        assert!(config.exclude_occluder_ids.is_empty());
        assert_eq!(config.road_texture_paths, vec![".*only.*".to_owned()]);
    }

    #[test]
    fn invalid_persisted_regex_fails_validation() {
        let persisted = PersistedUnclipConfig {
            plugin: Some(PathBuf::from("x.esp")),
            exclude_grass_ids: Some(vec!["(".to_owned()]),
            ..PersistedUnclipConfig::default()
        };
        let config = merged(&["greenmote", "unclip"], persisted);
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("grass id filter")
        );
    }

    #[test]
    fn generated_default_round_trips_without_unknown_keys() {
        let toml = toml::to_string(&PersistedUnclipConfig::generated_default()).unwrap();
        let parsed = PersistedUnclipConfig::from_toml(&format!("[unclip]\n{toml}")).unwrap();
        assert!(parsed.unknown.is_empty());
        assert_eq!(parsed, PersistedUnclipConfig::generated_default());
    }
}
