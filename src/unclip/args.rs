// SPDX-License-Identifier: GPL-3.0-only

//! Command-line arguments and policy values for `greenmote unclip`.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

/// Largest gap between the highest base vertex and the terrain that still counts as resting on it.
pub(crate) const DEFAULT_FLOAT_TOLERANCE: f32 = 1.0;
/// Deepest burial of the highest base vertex that still counts as acceptable, in units.
pub(crate) const DEFAULT_MAX_SINK: f32 = 24.0;
/// Deepest burial as a fraction of the mesh height. The larger of the two limits applies.
pub(crate) const DEFAULT_MAX_SINK_FRACTION: f32 = 0.75;
/// Target burial depth of the highest base vertex after a terrain fix.
pub(crate) const DEFAULT_SINK: f32 = 4.0;
pub(crate) const DEFAULT_RELOCATION_STEP: f32 = 32.0;
pub(crate) const DEFAULT_RELOCATION_STEPS: u16 = 8;
pub(crate) const DEFAULT_ORIENTATION_EPSILON_DEGREES: f32 = 1.0;

/// Parsed arguments for the `unclip` subcommand.
#[derive(Parser, Clone, Debug, Default)]
#[command(
    name = "unclip",
    about = "Find groundcover refs that float, sink, stand on roads or water, or clip into statics, and fix them."
)]
pub struct UnclipArgs {
    /// Groundcover plugin to inspect. May be a filesystem path or a VFS plugin name.
    #[arg(short = 'p', long = "plugin", value_name = "PLUGIN")]
    pub plugin: Option<PathBuf>,

    /// Write the planned changes. Without this flag unclip only reports what it would do.
    #[arg(long = "write", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub write: Option<bool>,

    /// Write the rewritten plugin to PATH instead of replacing the source.
    #[arg(long = "output-plugin", value_name = "PATH")]
    pub output_plugin: Option<PathBuf>,

    /// Write a per-reference table of every verdict to greenmote-unclip.log.
    #[arg(long = "verbose", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub verbose: Option<bool>,

    /// Emit the report as machine-readable JSON.
    #[arg(long = "structured", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL")]
    pub structured: Option<bool>,

    /// Comma-separated actions to plan. Defaults to every action.
    #[arg(
        long = "actions",
        visible_alias = "write-actions",
        value_enum,
        value_delimiter = ','
    )]
    pub actions: Vec<ActionArg>,

    /// Largest gap (units) between the mesh base and the terrain that still counts as grounded.
    #[arg(long = "float-tolerance", value_parser = non_negative_f32)]
    pub float_tolerance: Option<f32>,

    /// Deepest burial (units) of the mesh base that still counts as acceptable.
    #[arg(long = "max-sink", value_parser = non_negative_f32)]
    pub max_sink: Option<f32>,

    /// Deepest burial as a fraction of the mesh height (0 to 1). The larger of --max-sink and
    /// this fraction of the mesh height is the limit; generators bury tall plants deeper.
    #[arg(long = "max-sink-fraction", value_parser = unit_fraction)]
    pub max_sink_fraction: Option<f32>,

    /// Burial depth (units) the mesh base is placed at when terrain-z fixes a ref.
    #[arg(long = "sink", value_parser = non_negative_f32)]
    pub sink: Option<f32>,

    /// Horizontal distance between static relocation probes.
    #[arg(long = "relocation-step", value_parser = positive_f32)]
    pub relocation_step: Option<f32>,

    /// Number of relocation probe rings to try for static moves.
    #[arg(long = "relocation-steps", value_parser = relocation_steps)]
    pub relocation_steps: Option<u16>,

    /// Maximum tilt difference in degrees treated as already aligned to terrain.
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

    /// Exclude static occluders whose full IDs match this case-insensitive regex. Adds to the defaults.
    #[arg(long = "exclude-occluder-id", value_name = "REGEX")]
    pub exclude_occluder_ids: Vec<String>,

    /// Drop the built-in tree-like occluder exclusions.
    #[arg(long = "no-default-occluder-excludes")]
    pub no_default_occluder_excludes: bool,

    /// Road texture path regex used by road-delete. Adds to the defaults. May be repeated.
    #[arg(long = "road-texture-path", value_name = "REGEX")]
    pub road_texture_paths: Vec<String>,

    /// Drop the built-in road texture patterns.
    #[arg(long = "no-default-road-textures")]
    pub no_default_road_textures: bool,

    /// Deprecated and ignored: unclip always continues past static occluder meshes it cannot
    /// load and lists them as a warning in the report.
    #[arg(long = "ignore-missing-meshes", num_args = 0..=1, default_missing_value = "true", value_name = "BOOL", hide = true)]
    pub ignore_missing_meshes: Option<bool>,
}

/// Actions accepted by `unclip --actions` and `[unclip].actions`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ActionArg {
    /// Move floating or buried references vertically so their base rests on the terrain.
    TerrainZ,
    /// Delete references standing on terrain that lies below the exterior water plane.
    WaterDelete,
    /// Delete references whose LAND texture matches the road filters.
    RoadDelete,
    /// Delete references that are inside a static occluder and cannot be moved clear.
    StaticDelete,
    /// Move references sideways when a nearby clear spot exists.
    StaticMove,
    /// Tilt references to the terrain slope.
    Orient,
}

impl ActionArg {
    pub(crate) const ALL: [Self; 6] = [
        Self::TerrainZ,
        Self::WaterDelete,
        Self::RoadDelete,
        Self::StaticDelete,
        Self::StaticMove,
        Self::Orient,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::TerrainZ => "terrain-z",
            Self::WaterDelete => "water-delete",
            Self::RoadDelete => "road-delete",
            Self::StaticDelete => "static-delete",
            Self::StaticMove => "static-move",
            Self::Orient => "orient",
        }
    }
}

/// Fully resolved unclip policy used by measurement, decision, and reporting.
#[derive(Clone, Debug)]
pub(crate) struct UnclipPolicy {
    pub(crate) actions: Actions,
    pub(crate) float_tolerance: f32,
    pub(crate) max_sink: f32,
    pub(crate) max_sink_fraction: f32,
    pub(crate) sink: f32,
    pub(crate) orientation_epsilon_degrees: f32,
    pub(crate) relocation: RelocationPolicy,
    pub(crate) target_filter: IdFilter,
    pub(crate) occluder_filter: IdFilter,
    pub(crate) road_texture_filter: RoadTextureFilter,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Actions {
    flags: u8,
}

const ACTION_TERRAIN_Z: u8 = 1 << 0;
const ACTION_WATER_DELETE: u8 = 1 << 1;
const ACTION_ROAD_DELETE: u8 = 1 << 2;
const ACTION_STATIC_DELETE: u8 = 1 << 3;
const ACTION_STATIC_MOVE: u8 = 1 << 4;
const ACTION_ORIENT: u8 = 1 << 5;

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

pub(crate) fn default_actions() -> Vec<ActionArg> {
    ActionArg::ALL.to_vec()
}

impl Actions {
    pub(crate) fn from_args(actions: &[ActionArg]) -> Self {
        let mut result = Self::empty();
        for action in actions {
            result.enable(*action);
        }
        result
    }

    pub(crate) const fn empty() -> Self {
        Self { flags: 0 }
    }

    #[cfg(test)]
    pub(crate) const fn all() -> Self {
        Self {
            flags: ACTION_TERRAIN_Z
                | ACTION_WATER_DELETE
                | ACTION_ROAD_DELETE
                | ACTION_STATIC_DELETE
                | ACTION_STATIC_MOVE
                | ACTION_ORIENT,
        }
    }

    const fn flag(action: ActionArg) -> u8 {
        match action {
            ActionArg::TerrainZ => ACTION_TERRAIN_Z,
            ActionArg::WaterDelete => ACTION_WATER_DELETE,
            ActionArg::RoadDelete => ACTION_ROAD_DELETE,
            ActionArg::StaticDelete => ACTION_STATIC_DELETE,
            ActionArg::StaticMove => ACTION_STATIC_MOVE,
            ActionArg::Orient => ACTION_ORIENT,
        }
    }

    pub(crate) const fn enable(&mut self, action: ActionArg) {
        self.flags |= Self::flag(action);
    }

    #[cfg(test)]
    pub(crate) const fn disable(&mut self, action: ActionArg) {
        self.flags &= !Self::flag(action);
    }

    pub(crate) const fn contains(self, action: ActionArg) -> bool {
        self.flags & Self::flag(action) != 0
    }

    pub(crate) const fn terrain_z(self) -> bool {
        self.contains(ActionArg::TerrainZ)
    }

    pub(crate) const fn water_delete(self) -> bool {
        self.contains(ActionArg::WaterDelete)
    }

    pub(crate) const fn road_delete(self) -> bool {
        self.contains(ActionArg::RoadDelete)
    }

    pub(crate) const fn static_delete(self) -> bool {
        self.contains(ActionArg::StaticDelete)
    }

    pub(crate) const fn static_move(self) -> bool {
        self.contains(ActionArg::StaticMove)
    }

    pub(crate) const fn orient(self) -> bool {
        self.contains(ActionArg::Orient)
    }

    pub(crate) fn enabled_names(self) -> Vec<&'static str> {
        ActionArg::ALL
            .into_iter()
            .filter(|action| self.contains(*action))
            .map(ActionArg::name)
            .collect()
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

fn unit_fraction(value: &str) -> Result<f32, String> {
    let value = non_negative_f32(value)?;
    if value <= 1.0 {
        Ok(value)
    } else {
        Err("expected a number from 0 to 1".to_owned())
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

impl UnclipPolicy {
    /// How deep a mesh of `mesh_height` may sit below the terrain before it counts as buried:
    /// `max(max_sink, max_sink_fraction * mesh_height)`.
    #[must_use]
    pub(crate) fn burial_limit(&self, mesh_height: f32) -> f32 {
        self.max_sink.max(self.max_sink_fraction * mesh_height)
    }
}

#[cfg(test)]
impl UnclipPolicy {
    pub(crate) fn for_test() -> Self {
        Self {
            actions: Actions::all(),
            float_tolerance: DEFAULT_FLOAT_TOLERANCE,
            max_sink: DEFAULT_MAX_SINK,
            max_sink_fraction: DEFAULT_MAX_SINK_FRACTION,
            sink: DEFAULT_SINK,
            orientation_epsilon_degrees: DEFAULT_ORIENTATION_EPSILON_DEGREES,
            relocation: RelocationPolicy {
                step: DEFAULT_RELOCATION_STEP,
                steps: DEFAULT_RELOCATION_STEPS,
            },
            target_filter: IdFilter::new(&[], &[]).unwrap(),
            occluder_filter: IdFilter::new(&[], &[]).unwrap(),
            road_texture_filter: RoadTextureFilter::new(&default_road_texture_path_patterns())
                .unwrap(),
        }
    }

    pub(crate) fn with_target_filter(mut self, include: &[&str], exclude: &[&str]) -> Self {
        let include = include.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let exclude = exclude.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        self.target_filter = IdFilter::new(&include, &exclude).unwrap();
        self
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{Cli, Command};

    use super::{ActionArg, Actions, IdFilter};

    fn parse_unclip_args(args: &[&str]) -> super::UnclipArgs {
        let cli = Cli::parse_from(args);
        let Some(Command::Unclip(args)) = cli.command else {
            panic!("expected unclip command");
        };
        *args
    }

    #[test]
    fn write_flag_accepts_optional_bool() {
        let args = parse_unclip_args(&["greenmote", "unclip", "--write"]);
        assert_eq!(args.write, Some(true));

        let args = parse_unclip_args(&["greenmote", "unclip", "--write=false"]);
        assert_eq!(args.write, Some(false));

        let args = parse_unclip_args(&["greenmote", "unclip"]);
        assert_eq!(args.write, None);
    }

    #[test]
    fn removed_flags_are_rejected() {
        for flag in [
            "--dry-run",
            "--in-place",
            "--instances",
            "--meshgenerator-ini=x.ini",
            "--origin-epsilon=1",
        ] {
            assert!(
                Cli::try_parse_from(["greenmote", "unclip", flag]).is_err(),
                "{flag} should be unknown"
            );
        }
    }

    #[test]
    fn actions_accept_comma_lists_and_legacy_alias() {
        let args = parse_unclip_args(&["greenmote", "unclip", "--actions", "terrain-z,orient"]);
        assert_eq!(args.actions, vec![ActionArg::TerrainZ, ActionArg::Orient]);

        let args = parse_unclip_args(&["greenmote", "unclip", "--write-actions", "road-delete"]);
        assert_eq!(args.actions, vec![ActionArg::RoadDelete]);

        assert!(Cli::try_parse_from(["greenmote", "unclip", "--actions", "all"]).is_err());
    }

    #[test]
    fn actions_flags_round_trip() {
        let actions = Actions::from_args(&[ActionArg::TerrainZ, ActionArg::StaticMove]);
        assert!(actions.terrain_z());
        assert!(actions.static_move());
        assert!(!actions.orient());
        assert_eq!(actions.enabled_names(), vec!["terrain-z", "static-move"]);
        assert_eq!(Actions::from_args(&ActionArg::ALL), Actions::all());
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
    fn target_filter_includes_all_without_include_patterns() {
        let filter = IdFilter::new(&[], &[]).unwrap();

        assert!(filter.includes("flora_grass_01"));
    }

    #[test]
    fn target_filter_matches_case_insensitive_full_ids() {
        let filter = IdFilter::new(&["flora_grass_.*".to_owned()], &[]).unwrap();

        assert!(filter.includes("Flora_Grass_01"));
        assert!(!filter.includes("flora_bush_01"));
        assert!(
            !IdFilter::new(&["grass".to_owned()], &[])
                .unwrap()
                .includes("flora_grass_01")
        );
    }

    #[test]
    fn target_filter_exclude_wins_over_include() {
        let filter =
            IdFilter::new(&["flora_.*".to_owned()], &["flora_grass_bad_.+".to_owned()]).unwrap();

        assert!(filter.includes("flora_grass_good_1"));
        assert!(!filter.includes("flora_grass_bad_1"));
    }

    #[test]
    fn filters_reject_invalid_regex() {
        assert!(IdFilter::new(&["(".to_owned()], &[]).is_err());
        assert!(super::RoadTextureFilter::new(&["(".to_owned()]).is_err());
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
        assert!(
            !super::RoadTextureFilter::new(&[])
                .unwrap()
                .includes("textures/landscape/tx_bm_dirtroad_01.dds")
        );
    }
}
