use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use super::model::{CONTACT_TERRAIN_EPSILON, ORIGIN_TERRAIN_EPSILON};

const DEFAULT_RELOCATION_STEP: f32 = 32.0;
const DEFAULT_RELOCATION_STEPS: u16 = 8;

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

    /// Back up and replace the target plugin with adjusted reference Z positions.
    #[arg(long = "write")]
    pub write: bool,

    /// Comma-separated write actions to plan when --write is set.
    #[arg(
        long = "write-actions",
        value_enum,
        value_delimiter = ',',
        default_values_t = [WriteActionArg::TerrainZ, WriteActionArg::StaticDelete, WriteActionArg::StaticMove],
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
}

#[derive(Clone, Debug)]
pub(crate) struct UnclipPolicy {
    pub(crate) write_actions: WriteActions,
    pub(crate) contact_epsilon: f32,
    pub(crate) origin_epsilon: f32,
    pub(crate) relocation: RelocationPolicy,
    pub(crate) target_filter: TargetFilter,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WriteActions {
    pub(crate) terrain_z: bool,
    pub(crate) static_delete: bool,
    pub(crate) static_move: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RelocationPolicy {
    pub(crate) step: f32,
    pub(crate) steps: u16,
}

#[derive(Clone, Debug)]
pub(crate) struct TargetFilter {
    include_ids: Vec<String>,
    exclude_ids: Vec<String>,
}

impl UnclipArgs {
    pub(crate) fn policy(&self) -> UnclipPolicy {
        UnclipPolicy {
            write_actions: WriteActions::from_args(&self.write_actions),
            contact_epsilon: self.contact_epsilon,
            origin_epsilon: self.origin_epsilon,
            relocation: RelocationPolicy {
                step: self.relocation_step,
                steps: self.relocation_steps,
            },
            target_filter: TargetFilter::new(&self.include_ids, &self.exclude_ids),
        }
    }
}

impl WriteActions {
    fn from_args(actions: &[WriteActionArg]) -> Self {
        let mut write_actions = Self {
            terrain_z: false,
            static_delete: false,
            static_move: false,
        };
        for action in actions {
            match action {
                WriteActionArg::All => {
                    write_actions.terrain_z = true;
                    write_actions.static_delete = true;
                    write_actions.static_move = true;
                }
                WriteActionArg::None => {
                    write_actions.terrain_z = false;
                    write_actions.static_delete = false;
                    write_actions.static_move = false;
                }
                WriteActionArg::TerrainZ => write_actions.terrain_z = true,
                WriteActionArg::StaticDelete => write_actions.static_delete = true,
                WriteActionArg::StaticMove => write_actions.static_move = true,
            }
        }
        write_actions
    }
}

impl TargetFilter {
    pub(crate) fn new(include_ids: &[String], exclude_ids: &[String]) -> Self {
        Self {
            include_ids: include_ids
                .iter()
                .map(|pattern| pattern.to_lowercase())
                .collect(),
            exclude_ids: exclude_ids
                .iter()
                .map(|pattern| pattern.to_lowercase())
                .collect(),
        }
    }

    pub(crate) fn includes(&self, id: &str) -> bool {
        let id = id.to_lowercase();
        if self
            .exclude_ids
            .iter()
            .any(|pattern| wildcard_matches(pattern, &id))
        {
            return false;
        }
        self.include_ids.is_empty()
            || self
                .include_ids
                .iter()
                .any(|pattern| wildcard_matches(pattern, &id))
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
