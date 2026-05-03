use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use super::model::{CONTACT_TERRAIN_EPSILON, ORIGIN_TERRAIN_EPSILON};

const DEFAULT_RELOCATION_STEP: f32 = 32.0;
const DEFAULT_RELOCATION_STEPS: u16 = 8;
const DEFAULT_ORIENTATION_EPSILON_DEGREES: f32 = 1.0;

#[derive(Parser, Clone, Debug)]
#[command(
    name = "unclip",
    about = "Inspect groundcover refs against OpenMW terrain before unclipping."
)]
pub struct UnclipArgs {
    /// Path to openmw.cfg, or a directory containing openmw.cfg.
    #[arg(short = 'c', long = "openmw-cfg")]
    pub openmw_cfg: Option<PathBuf>,

    /// Groundcover plugin to inspect. May be a filesystem path or a VFS plugin name.
    #[arg(short = 'p', long = "plugin", value_name = "PLUGIN")]
    pub plugin: PathBuf,

    /// Include per-reference instance diagnostics.
    #[arg(long = "instances")]
    pub instances: bool,

    /// Emit machine-readable compact JSON. With --instances, emits newline-delimited JSON records.
    #[arg(long = "structured")]
    pub structured: bool,

    /// Back up and replace the target plugin with planned unclipping changes.
    #[arg(long = "write")]
    pub write: bool,

    /// Comma-separated write actions to plan when --write is set.
    #[arg(
        long = "write-actions",
        value_enum,
        value_delimiter = ',',
        default_values_t = [WriteActionArg::TerrainZ, WriteActionArg::StaticDelete, WriteActionArg::StaticMove, WriteActionArg::Orient],
    )]
    pub write_actions: Vec<WriteActionArg>,

    /// Maximum mesh contact/terrain Z delta treated as already on terrain.
    #[arg(long = "contact-epsilon", default_value_t = CONTACT_TERRAIN_EPSILON, value_parser = non_negative_f32)]
    pub contact_epsilon: f32,

    /// Maximum reference origin/terrain Z delta treated as already on terrain.
    #[arg(long = "origin-epsilon", default_value_t = ORIGIN_TERRAIN_EPSILON, value_parser = non_negative_f32)]
    pub origin_epsilon: f32,

    /// Horizontal distance between static-bounds relocation probes.
    #[arg(long = "relocation-step", default_value_t = DEFAULT_RELOCATION_STEP, value_parser = positive_f32)]
    pub relocation_step: f32,

    /// Number of relocation probe rings to try for static-bounds moves.
    #[arg(long = "relocation-steps", default_value_t = DEFAULT_RELOCATION_STEPS, value_parser = relocation_steps)]
    pub relocation_steps: u16,

    /// Maximum tilt angle in degrees treated as already aligned to terrain.
    #[arg(long = "orientation-epsilon", default_value_t = DEFAULT_ORIENTATION_EPSILON_DEGREES, value_parser = non_negative_f32)]
    pub orientation_epsilon: f32,

    /// Include only target refs whose IDs match this case-insensitive wildcard. May be repeated.
    #[arg(long = "include-id", value_name = "PATTERN")]
    pub include_ids: Vec<String>,

    /// Exclude target refs whose IDs match this case-insensitive wildcard. May be repeated.
    #[arg(long = "exclude-id", value_name = "PATTERN")]
    pub exclude_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum WriteActionArg {
    All,
    None,
    TerrainZ,
    StaticDelete,
    StaticMove,
    Orient,
}

#[derive(Clone, Debug)]
pub(crate) struct UnclipPolicy {
    pub(crate) write_actions: WriteActions,
    pub(crate) contact_epsilon: f32,
    pub(crate) origin_epsilon: f32,
    pub(crate) orientation_epsilon_degrees: f32,
    pub(crate) relocation: RelocationPolicy,
    pub(crate) target_filter: TargetFilter,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WriteActions {
    flags: u8,
}

const WRITE_TERRAIN_Z: u8 = 1 << 0;
const WRITE_STATIC_DELETE: u8 = 1 << 1;
const WRITE_STATIC_MOVE: u8 = 1 << 2;
const WRITE_ORIENT: u8 = 1 << 3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct RelocationPolicy {
    pub(crate) step: f32,
    pub(crate) steps: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct TargetFilter {
    include_patterns: Vec<String>,
    exclude_patterns: Vec<String>,
    normalized_includes: Vec<String>,
    normalized_excludes: Vec<String>,
}

impl UnclipArgs {
    pub(crate) fn policy(&self) -> Result<UnclipPolicy, String> {
        let write_actions = WriteActions::from_args(&self.write_actions)?;
        Ok(UnclipPolicy {
            write_actions,
            contact_epsilon: self.contact_epsilon,
            origin_epsilon: self.origin_epsilon,
            orientation_epsilon_degrees: self.orientation_epsilon,
            relocation: RelocationPolicy {
                step: self.relocation_step,
                steps: self.relocation_steps,
            },
            target_filter: TargetFilter::new(&self.include_ids, &self.exclude_ids),
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
            flags: WRITE_TERRAIN_Z | WRITE_STATIC_DELETE | WRITE_STATIC_MOVE | WRITE_ORIENT,
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

impl TargetFilter {
    pub(crate) fn new(include_ids: &[String], exclude_ids: &[String]) -> Self {
        Self {
            include_patterns: include_ids.to_vec(),
            exclude_patterns: exclude_ids.to_vec(),
            normalized_includes: include_ids
                .iter()
                .map(|pattern| pattern.to_lowercase())
                .collect(),
            normalized_excludes: exclude_ids
                .iter()
                .map(|pattern| pattern.to_lowercase())
                .collect(),
        }
    }

    pub(crate) fn includes(&self, id: &str) -> bool {
        let id = id.to_lowercase();
        if self
            .normalized_excludes
            .iter()
            .any(|pattern| wildcard_matches(pattern, &id))
        {
            return false;
        }
        self.normalized_includes.is_empty()
            || self
                .normalized_includes
                .iter()
                .any(|pattern| wildcard_matches(pattern, &id))
    }

    pub(crate) fn include_ids(&self) -> &[String] {
        &self.include_patterns
    }

    pub(crate) fn exclude_ids(&self) -> &[String] {
        &self.exclude_patterns
    }
}

fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let (mut pattern_index, mut value_index) = (0, 0);
    let mut star = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
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
    use super::TargetFilter;

    #[test]
    fn target_filter_includes_all_without_include_patterns() {
        let filter = TargetFilter::new(&[], &[]);

        assert!(filter.includes("flora_grass_01"));
    }

    #[test]
    fn target_filter_matches_case_insensitive_wildcards() {
        let filter = TargetFilter::new(&["flora_grass_*".to_owned()], &[]);

        assert!(filter.includes("Flora_Grass_01"));
        assert!(!filter.includes("flora_bush_01"));
    }

    #[test]
    fn target_filter_exclude_wins_over_include() {
        let filter = TargetFilter::new(&["flora_*".to_owned()], &["flora_grass_bad_?".to_owned()]);

        assert!(filter.includes("flora_grass_good_1"));
        assert!(!filter.includes("flora_grass_bad_1"));
    }
}
