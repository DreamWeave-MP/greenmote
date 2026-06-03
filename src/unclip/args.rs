// SPDX-License-Identifier: GPL-3.0-only

//! Command-line arguments and policy values for `greenmote unclip`.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

#[cfg(test)]
use super::model::ORIGIN_TERRAIN_EPSILON;

pub(crate) const DEFAULT_RELOCATION_STEP: f32 = 32.0;
pub(crate) const DEFAULT_RELOCATION_STEPS: u16 = 8;
pub(crate) const DEFAULT_ORIENTATION_EPSILON_DEGREES: f32 = 1.0;

/// Parsed arguments for the `unclip` subcommand.
#[derive(Parser, Clone, Debug)]
#[command(
    name = "unclip",
    about = "Inspect groundcover refs against OpenMW terrain before unclipping."
)]
pub struct UnclipArgs {
    /// Groundcover plugin to inspect. May be a filesystem path or a VFS plugin name.
    #[arg(short = 'p', long = "plugin", value_name = "PLUGIN")]
    pub plugin: Option<PathBuf>,

    /// Plugin path to write. Defaults to replacing the resolved source plugin.
    #[arg(long = "output-plugin", value_name = "PATH")]
    pub output_plugin: Option<PathBuf>,

    /// Deprecated alias for --verbose.
    #[arg(long = "instances", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub instances: Option<bool>,

    /// Write full per-reference diagnostics to greenmote.log.
    #[arg(long = "verbose", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub verbose: Option<bool>,

    /// mw-groundcover-generator INI used as an optional placement inference hint.
    #[arg(long = "meshgenerator-ini", value_name = "INI")]
    pub meshgenerator_ini: Option<PathBuf>,

    #[arg(skip)]
    pub(crate) ignore_meshgenerator_ini: bool,

    /// Emit the compact summary as machine-readable JSON.
    #[arg(long = "structured", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub structured: Option<bool>,

    /// Inspect planned unclipping changes without writing them.
    #[arg(long = "dry-run", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub dry_run: Option<bool>,

    /// Comma-separated write actions to plan when writing.
    #[arg(long = "write-actions", value_enum, value_delimiter = ',')]
    pub write_actions: Vec<WriteActionArg>,

    /// Maximum reference origin/terrain Z delta treated as already on terrain.
    #[arg(long = "origin-epsilon", value_parser = non_negative_f32)]
    pub origin_epsilon: Option<f32>,

    /// Horizontal distance between static-bounds relocation probes.
    #[arg(long = "relocation-step", value_parser = positive_f32)]
    pub relocation_step: Option<f32>,

    /// Number of relocation probe rings to try for static-bounds moves.
    #[arg(long = "relocation-steps", value_parser = relocation_steps)]
    pub relocation_steps: Option<u16>,

    /// Maximum tilt angle in degrees treated as already aligned to terrain.
    #[arg(long = "orientation-epsilon", value_parser = non_negative_f32)]
    pub orientation_epsilon: Option<f32>,

    /// Include only target grass refs whose full IDs match this case-insensitive regex. May be repeated.
    #[arg(long = "include-grass-id", value_name = "REGEX")]
    pub include_grass_ids: Vec<String>,

    /// Exclude target grass refs whose full IDs match this case-insensitive regex. May be repeated.
    #[arg(long = "exclude-grass-id", value_name = "REGEX")]
    pub exclude_grass_ids: Vec<String>,

    /// Include only static occluders whose full IDs match this case-insensitive regex. May be repeated.
    #[arg(long = "include-occluder-id", value_name = "REGEX")]
    pub include_occluder_ids: Vec<String>,

    /// Exclude static occluders whose full IDs match this case-insensitive regex. May be repeated.
    #[arg(long = "exclude-occluder-id", value_name = "REGEX")]
    pub exclude_occluder_ids: Vec<String>,

    /// Road texture path regexes used by road-delete. May be repeated.
    #[arg(long = "road-texture-path", value_name = "REGEX")]
    pub road_texture_paths: Vec<String>,
}

/// Write actions accepted by `unclip --write-actions` and `[unclip].write_actions`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum WriteActionArg {
    /// Enable every write action.
    All,
    /// Disable every write action.
    None,
    /// Move clipped references vertically to terrain height.
    TerrainZ,
    /// Delete refs that terrain adjustment would move across exterior water.
    WaterDelete,
    /// Delete refs whose sampled LAND texture path matches road filters.
    RoadDelete,
    /// Delete references that remain inside static occluders.
    StaticDelete,
    /// Move references horizontally away from static occluders when a nearby location is found.
    StaticMove,
    /// Orient references toward terrain slope.
    Orient,
}

#[derive(Clone, Debug)]
pub(crate) struct UnclipPolicy {
    pub(crate) write_actions: WriteActions,
    pub(crate) origin_epsilon: f32,
    pub(crate) orientation_epsilon_degrees: f32,
    pub(crate) relocation: RelocationPolicy,
    pub(crate) target_filter: IdFilter,
    pub(crate) occluder_filter: IdFilter,
    pub(crate) road_texture_filter: RoadTextureFilter,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WriteActions {
    flags: u8,
}

const WRITE_TERRAIN_Z: u8 = 1 << 0;
const WRITE_WATER_DELETE: u8 = 1 << 1;
const WRITE_ROAD_DELETE: u8 = 1 << 2;
const WRITE_STATIC_DELETE: u8 = 1 << 3;
const WRITE_STATIC_MOVE: u8 = 1 << 4;
const WRITE_ORIENT: u8 = 1 << 5;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RelocationPolicy {
    pub(crate) step: f32,
    pub(crate) steps: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct IdFilter {
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    includes: Vec<Regex>,
    excludes: Vec<Regex>,
}

#[derive(Clone, Debug)]
pub(crate) struct RoadTextureFilter {
    patterns: Vec<String>,
    regexes: Vec<Regex>,
}

impl UnclipArgs {
    #[cfg(test)]
    pub(crate) fn policy(&self) -> Result<UnclipPolicy, String> {
        let plugin = self.plugin.as_ref().ok_or_else(|| {
            "unclip requires --plugin or [unclip].plugin in greenmote.toml".to_owned()
        })?;
        let resolved = crate::unclip::config::UnclipConfig {
            openmw_cfg: None,
            plugin: plugin.clone(),
            output_plugin: self.output_plugin.clone(),
            meshgenerator_ini: if self.ignore_meshgenerator_ini {
                None
            } else {
                self.meshgenerator_ini.clone()
            },
            verbose: self.verbose.or(self.instances).unwrap_or(false),
            structured: self.structured.unwrap_or(false),
            dry_run: self.dry_run.unwrap_or(false),
            write_actions: if self.write_actions.is_empty() {
                default_write_actions()
            } else {
                self.write_actions.clone()
            },
            origin_epsilon: self.origin_epsilon.unwrap_or(ORIGIN_TERRAIN_EPSILON),
            relocation_step: self.relocation_step.unwrap_or(DEFAULT_RELOCATION_STEP),
            relocation_steps: self.relocation_steps.unwrap_or(DEFAULT_RELOCATION_STEPS),
            orientation_epsilon: self
                .orientation_epsilon
                .unwrap_or(DEFAULT_ORIENTATION_EPSILON_DEGREES),
            include_grass_ids: self.include_grass_ids.clone(),
            exclude_grass_ids: self.exclude_grass_ids.clone(),
            include_occluder_ids: self.include_occluder_ids.clone(),
            exclude_occluder_ids: self.exclude_occluder_ids.clone(),
            road_texture_paths: self.road_texture_paths.clone(),
        };
        resolved.policy()
    }
}

pub(crate) fn default_write_actions() -> Vec<WriteActionArg> {
    vec![
        WriteActionArg::TerrainZ,
        WriteActionArg::WaterDelete,
        WriteActionArg::RoadDelete,
        WriteActionArg::StaticDelete,
        WriteActionArg::StaticMove,
        WriteActionArg::Orient,
    ]
}

impl crate::unclip::config::UnclipConfig {
    pub(crate) fn policy(&self) -> Result<UnclipPolicy, String> {
        let write_actions = WriteActions::from_args(&self.write_actions)?;
        Ok(UnclipPolicy {
            write_actions,
            origin_epsilon: self.origin_epsilon,
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
}

impl WriteActions {
    fn from_args(actions: &[WriteActionArg]) -> Result<Self, String> {
        if actions.len() > 1
            && actions
                .iter()
                .any(|action| matches!(action, WriteActionArg::All | WriteActionArg::None))
        {
            return Err(
                "write action 'all' or 'none' cannot be combined with other actions".to_owned(),
            );
        }
        let mut write_actions = Self::empty();
        for action in actions {
            match action {
                WriteActionArg::All => {
                    write_actions = Self::all();
                }
                WriteActionArg::None => {
                    write_actions = Self::empty();
                }
                WriteActionArg::TerrainZ => write_actions.enable(WRITE_TERRAIN_Z),
                WriteActionArg::WaterDelete => write_actions.enable(WRITE_WATER_DELETE),
                WriteActionArg::RoadDelete => write_actions.enable(WRITE_ROAD_DELETE),
                WriteActionArg::StaticDelete => write_actions.enable(WRITE_STATIC_DELETE),
                WriteActionArg::StaticMove => write_actions.enable(WRITE_STATIC_MOVE),
                WriteActionArg::Orient => write_actions.enable(WRITE_ORIENT),
            }
        }
        Ok(write_actions)
    }

    pub(crate) const fn empty() -> Self {
        Self { flags: 0 }
    }

    pub(crate) const fn all() -> Self {
        Self {
            flags: WRITE_TERRAIN_Z
                | WRITE_WATER_DELETE
                | WRITE_ROAD_DELETE
                | WRITE_STATIC_DELETE
                | WRITE_STATIC_MOVE
                | WRITE_ORIENT,
        }
    }

    fn enable(&mut self, flag: u8) {
        self.flags |= flag;
    }

    #[cfg(test)]
    pub(crate) fn disable_terrain_z(&mut self) {
        self.flags &= !WRITE_TERRAIN_Z;
    }

    #[cfg(test)]
    pub(crate) fn disable_water_delete(&mut self) {
        self.flags &= !WRITE_WATER_DELETE;
    }

    #[cfg(test)]
    pub(crate) fn disable_road_delete(&mut self) {
        self.flags &= !WRITE_ROAD_DELETE;
    }

    #[cfg(test)]
    pub(crate) fn disable_static_delete(&mut self) {
        self.flags &= !WRITE_STATIC_DELETE;
    }

    #[cfg(test)]
    pub(crate) fn disable_static_move(&mut self) {
        self.flags &= !WRITE_STATIC_MOVE;
    }

    #[cfg(test)]
    pub(crate) fn disable_orient(&mut self) {
        self.flags &= !WRITE_ORIENT;
    }

    pub(crate) const fn terrain_z(self) -> bool {
        self.flags & WRITE_TERRAIN_Z != 0
    }

    pub(crate) const fn water_delete(self) -> bool {
        self.flags & WRITE_WATER_DELETE != 0
    }

    pub(crate) const fn road_delete(self) -> bool {
        self.flags & WRITE_ROAD_DELETE != 0
    }

    pub(crate) const fn static_delete(self) -> bool {
        self.flags & WRITE_STATIC_DELETE != 0
    }

    pub(crate) const fn static_move(self) -> bool {
        self.flags & WRITE_STATIC_MOVE != 0
    }

    pub(crate) const fn orient(self) -> bool {
        self.flags & WRITE_ORIENT != 0
    }

    pub(crate) const fn any_enabled(self) -> bool {
        self.flags != 0
    }

    pub(crate) fn enabled_names(self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.terrain_z() {
            names.push("terrain-z");
        }
        if self.water_delete() {
            names.push("water-delete");
        }
        if self.road_delete() {
            names.push("road-delete");
        }
        if self.static_delete() {
            names.push("static-delete");
        }
        if self.static_move() {
            names.push("static-move");
        }
        if self.orient() {
            names.push("orient");
        }
        names
    }
}

impl RoadTextureFilter {
    pub(crate) fn new(paths: &[String]) -> Result<Self, String> {
        Ok(Self {
            patterns: paths.to_vec(),
            regexes: compile_regexes(paths)?,
        })
    }

    pub(crate) fn includes(&self, path: &str) -> bool {
        self.regexes.iter().any(|pattern| pattern.is_match(path))
    }

    pub(crate) fn paths(&self) -> &[String] {
        &self.patterns
    }
}

pub(crate) fn default_road_texture_path_patterns() -> Vec<String> {
    vec![
        r".*(road|mainroad|dirtroad|gravelroad|beatenpath).*".to_owned(),
        r".*(cobble|cobblestone).*".to_owned(),
        r".*(street|whiteroad).*".to_owned(),
        r".*t_.*_terrroad.*".to_owned(),
        r".*t_imp_highway_txroad.*".to_owned(),
        r".*t_hr_.*road.*".to_owned(),
        r".*t_ham_.*road.*".to_owned(),
        r".*tx_sky.*road.*".to_owned(),
        r".*tr_alm_street.*".to_owned(),
        r".*nec_whiteroad.*".to_owned(),
    ]
}

impl IdFilter {
    pub(crate) fn new(include_ids: &[String], exclude_ids: &[String]) -> Result<Self, String> {
        Ok(Self {
            include_patterns: include_ids.to_vec(),
            exclude_patterns: exclude_ids.to_vec(),
            includes: compile_regexes(include_ids)?,
            excludes: compile_regexes(exclude_ids)?,
        })
    }

    pub(crate) fn includes(&self, id: &str) -> bool {
        if self.excludes.iter().any(|pattern| pattern.is_match(id)) {
            return false;
        }
        self.includes.is_empty() || self.includes.iter().any(|pattern| pattern.is_match(id))
    }

    pub(crate) fn include_ids(&self) -> &[String] {
        &self.include_patterns
    }

    pub(crate) fn exclude_ids(&self) -> &[String] {
        &self.exclude_patterns
    }
}

fn compile_regexes(patterns: &[String]) -> Result<Vec<Regex>, String> {
    patterns
        .iter()
        .map(|pattern| {
            RegexBuilder::new(&format!(r"\A(?:{pattern})\z"))
                .case_insensitive(true)
                .build()
                .map_err(|error| format!("{pattern:?}: {error}"))
        })
        .collect()
}

fn non_negative_f32(value: &str) -> Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|error| format!("expected a finite non-negative number: {error}"))?;
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err("expected a finite non-negative number".to_owned())
    }
}

fn positive_f32(value: &str) -> Result<f32, String> {
    let value = value
        .parse::<f32>()
        .map_err(|error| format!("expected a finite positive number: {error}"))?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err("expected a finite positive number".to_owned())
    }
}

fn relocation_steps(value: &str) -> Result<u16, String> {
    let value = value
        .parse::<u16>()
        .map_err(|error| format!("expected an integer from 1 to 256: {error}"))?;
    if (1..=256).contains(&value) {
        Ok(value)
    } else {
        Err("expected an integer from 1 to 256".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{Cli, Command};

    use super::IdFilter;

    fn parse_unclip_args(args: &[&str]) -> super::UnclipArgs {
        let cli = Cli::parse_from(args);
        let Some(Command::Unclip(args)) = cli.command else {
            panic!("expected unclip command");
        };
        *args
    }

    #[test]
    fn verbose_flag_accepts_optional_bool() {
        let args = parse_unclip_args(&["greenmote", "unclip", "--verbose"]);
        assert_eq!(args.verbose, Some(true));

        let args = parse_unclip_args(&["greenmote", "unclip", "--verbose=false"]);
        assert_eq!(args.verbose, Some(false));
    }

    #[test]
    fn output_plugin_flag_parses_path() {
        let args = parse_unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "source.omwaddon",
            "--output-plugin",
            "patched/source.omwaddon",
        ]);

        assert_eq!(
            args.output_plugin,
            Some(std::path::PathBuf::from("patched/source.omwaddon"))
        );
    }

    #[test]
    fn instances_flag_remains_deprecated_verbose_alias() {
        let args = parse_unclip_args(&["greenmote", "unclip", "--instances"]);
        assert_eq!(args.instances, Some(true));
        assert_eq!(args.verbose, None);

        let args = parse_unclip_args(&["greenmote", "unclip", "--instances=false"]);
        assert_eq!(args.instances, Some(false));
        assert_eq!(args.verbose, None);
    }

    #[test]
    fn target_filter_includes_all_without_include_patterns() {
        let filter = IdFilter::new(&[], &[]).unwrap();

        assert!(filter.includes("flora_grass_01"));
    }

    #[test]
    fn target_filter_matches_case_insensitive_regexes() {
        let filter = IdFilter::new(&["flora_grass_.*".to_owned()], &[]).unwrap();

        assert!(filter.includes("Flora_Grass_01"));
        assert!(!filter.includes("flora_bush_01"));
    }

    #[test]
    fn target_filter_regexes_match_full_ids() {
        let filter = IdFilter::new(&["grass".to_owned()], &[]).unwrap();

        assert!(filter.includes("grass"));
        assert!(!filter.includes("flora_grass_01"));
    }

    #[test]
    fn target_filter_exclude_wins_over_include() {
        let filter =
            IdFilter::new(&["flora_.*".to_owned()], &["flora_grass_bad_.+".to_owned()]).unwrap();

        assert!(filter.includes("flora_grass_good_1"));
        assert!(!filter.includes("flora_grass_bad_1"));
    }

    #[test]
    fn target_filter_rejects_invalid_regex() {
        assert!(IdFilter::new(&["(".to_owned()], &[]).is_err());
    }

    #[test]
    fn road_texture_filter_uses_configured_patterns() {
        let filter = super::RoadTextureFilter::new(&[
            ".*dirtroad.*".to_owned(),
            ".*custom_path_tile.*".to_owned(),
        ])
        .unwrap();

        assert!(filter.includes("textures/landscape/tx_bm_dirtroad_01.dds"));
        assert!(filter.includes("textures/custom/custom_path_tile_01.dds"));
        assert!(!filter.includes("textures/landscape/tx_grass_01.dds"));
    }

    #[test]
    fn road_texture_filter_does_not_extend_empty_config() {
        let filter = super::RoadTextureFilter::new(&[]).unwrap();

        assert!(!filter.includes("textures/landscape/tx_bm_dirtroad_01.dds"));
    }

    #[test]
    fn road_texture_filter_rejects_invalid_regex() {
        assert!(super::RoadTextureFilter::new(&["(".to_owned()]).is_err());
    }
}
