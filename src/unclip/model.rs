// SPDX-License-Identifier: GPL-3.0-only

use serde::Serialize;

use super::{
    args::UnclipPolicy, cells::CellCoord, static_occluders::StaticOccluderBuildReport,
    write_plan::WriteReport,
};

pub(crate) const ORIGIN_TERRAIN_EPSILON: f32 = 0.5;
pub(crate) const CONTACT_TERRAIN_EPSILON: f32 = 0.5;

pub(crate) struct UnclipReportContext {
    pub(crate) target_plugin: String,
    target_exterior_cells: usize,
    target_refs_total: usize,
    active_cells: usize,
    loaded_terrain_cells_total: usize,
    missing_active_terrain_cells: Vec<CellCoord>,
    pub(crate) static_occluder_report: StaticOccluderBuildReport,
    policy: UnclipPolicySummary,
    write_requested: bool,
    pub(crate) write: Option<WriteReport>,
}

impl UnclipReportContext {
    pub(crate) fn new(input: UnclipReportContextInput<'_>, policy: &UnclipPolicy) -> Self {
        Self {
            target_plugin: input.target_plugin_path.display().to_string(),
            target_exterior_cells: input.target_exterior_cells,
            target_refs_total: input.target_refs_total,
            active_cells: input.active_cells,
            loaded_terrain_cells_total: input.loaded_terrain_cells_total,
            missing_active_terrain_cells: input.missing_active_terrain_cells,
            static_occluder_report: input.static_occluder_report,
            policy: UnclipPolicySummary::from_policy(policy),
            write_requested: input.write_requested,
            write: None,
        }
    }

    pub(crate) const fn policy(&self) -> &UnclipPolicySummary {
        &self.policy
    }

    pub(crate) const fn write_requested(&self) -> bool {
        self.write_requested
    }

    pub(crate) fn missing_active_terrain_cells(&self) -> Vec<[i32; 2]> {
        self.missing_active_terrain_cells
            .iter()
            .map(|&(x, y)| [x, y])
            .collect()
    }

    pub(crate) fn summary(&self, inspection: &TerrainInspectionReport) -> UnclipSummary {
        let active_terrain_cells_missing = self.missing_active_terrain_cells.len();
        let target_refs_matching_filter = inspection.refs;
        UnclipSummary {
            target_exterior_cells: self.target_exterior_cells,
            target_refs_total: self.target_refs_total,
            target_refs_matching_filter,
            target_refs_filtered_out: self.target_refs_total - target_refs_matching_filter,
            active_cells: self.active_cells,
            loaded_terrain_cells_total: self.loaded_terrain_cells_total,
            active_terrain_cells_loaded: self.active_cells - active_terrain_cells_missing,
            active_terrain_cells_missing,
            filtered_refs_actionable: inspection.refs - inspection.refs_deleted,
            filtered_refs_deleted: inspection.refs_deleted,
            filtered_refs_with_mesh_contact: inspection.refs_with_mesh_contact,
            filtered_refs_without_resolved_static: inspection.refs_without_resolved_static,
            filtered_refs_missing_mesh_contact: inspection.refs_missing_mesh_contact,
            filtered_refs_with_origin_terrain: inspection.refs_with_terrain,
            filtered_refs_missing_origin_terrain: inspection.refs_missing_terrain,
            filtered_refs_origin_above_terrain: inspection.refs_above_terrain,
            filtered_refs_origin_below_terrain: inspection.refs_below_terrain,
            filtered_refs_mesh_contact_above_terrain: inspection.refs_contact_above_terrain,
            filtered_refs_mesh_contact_below_terrain: inspection.refs_contact_below_terrain,
            filtered_refs_mesh_contact_missing_terrain: inspection.refs_contact_missing_terrain,
            filtered_refs_static_bounds_occluded: inspection.refs_static_bounds_occluded,
            filtered_refs_static_bounds_fully_occluded: inspection
                .refs_static_bounds_fully_occluded,
            filtered_refs_static_bounds_relocatable: inspection.refs_static_bounds_relocatable,
            filtered_refs_static_bounds_blocked: inspection.refs_static_bounds_blocked,
        }
    }
}

pub(crate) struct UnclipReportContextInput<'a> {
    pub(crate) target_plugin_path: &'a std::path::Path,
    pub(crate) target_exterior_cells: usize,
    pub(crate) target_refs_total: usize,
    pub(crate) active_cells: usize,
    pub(crate) loaded_terrain_cells_total: usize,
    pub(crate) missing_active_terrain_cells: Vec<CellCoord>,
    pub(crate) static_occluder_report: StaticOccluderBuildReport,
    pub(crate) write_requested: bool,
}

#[derive(Clone, Serialize)]
pub(crate) struct UnclipPolicySummary {
    pub(crate) write_actions: Vec<&'static str>,
    pub(crate) origin_terrain_epsilon: f32,
    pub(crate) orientation_epsilon_degrees: f32,
    pub(crate) relocation_step: f32,
    pub(crate) relocation_steps: u16,
    pub(crate) include_grass_ids: Vec<String>,
    pub(crate) exclude_grass_ids: Vec<String>,
    pub(crate) include_occluder_ids: Vec<String>,
    pub(crate) exclude_occluder_ids: Vec<String>,
}

impl UnclipPolicySummary {
    fn from_policy(policy: &UnclipPolicy) -> Self {
        Self {
            write_actions: policy.write_actions.enabled_names(),
            origin_terrain_epsilon: policy.origin_epsilon,
            orientation_epsilon_degrees: policy.orientation_epsilon_degrees,
            relocation_step: policy.relocation.step,
            relocation_steps: policy.relocation.steps,
            include_grass_ids: policy.target_filter.include_ids().to_vec(),
            exclude_grass_ids: policy.target_filter.exclude_ids().to_vec(),
            include_occluder_ids: policy.occluder_filter.include_ids().to_vec(),
            exclude_occluder_ids: policy.occluder_filter.exclude_ids().to_vec(),
        }
    }

    pub(crate) const fn has_target_filter(&self) -> bool {
        !self.include_grass_ids.is_empty() || !self.exclude_grass_ids.is_empty()
    }

    pub(crate) const fn has_occluder_filter(&self) -> bool {
        !self.include_occluder_ids.is_empty() || !self.exclude_occluder_ids.is_empty()
    }
}

#[cfg(test)]
impl UnclipReportContext {
    pub(crate) fn new_for_test(target_plugin: &str) -> Self {
        Self {
            target_plugin: target_plugin.to_owned(),
            target_exterior_cells: 0,
            target_refs_total: 0,
            active_cells: 0,
            loaded_terrain_cells_total: 0,
            missing_active_terrain_cells: Vec::new(),
            static_occluder_report: StaticOccluderBuildReport::default(),
            policy: UnclipPolicySummary {
                write_actions: vec!["terrain-z", "static-delete", "static-move", "orient"],
                origin_terrain_epsilon: ORIGIN_TERRAIN_EPSILON,
                orientation_epsilon_degrees: 1.0,
                relocation_step: 32.0,
                relocation_steps: 8,
                include_grass_ids: Vec::new(),
                exclude_grass_ids: Vec::new(),
                include_occluder_ids: Vec::new(),
                exclude_occluder_ids: Vec::new(),
            },
            write_requested: false,
            write: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct UnclipSummary {
    pub(crate) target_exterior_cells: usize,
    pub(crate) target_refs_total: usize,
    pub(crate) target_refs_matching_filter: usize,
    pub(crate) target_refs_filtered_out: usize,
    pub(crate) active_cells: usize,
    pub(crate) loaded_terrain_cells_total: usize,
    pub(crate) active_terrain_cells_loaded: usize,
    pub(crate) active_terrain_cells_missing: usize,
    pub(crate) filtered_refs_actionable: usize,
    pub(crate) filtered_refs_deleted: usize,
    pub(crate) filtered_refs_with_mesh_contact: usize,
    pub(crate) filtered_refs_without_resolved_static: usize,
    pub(crate) filtered_refs_missing_mesh_contact: usize,
    pub(crate) filtered_refs_with_origin_terrain: usize,
    pub(crate) filtered_refs_missing_origin_terrain: usize,
    pub(crate) filtered_refs_origin_above_terrain: usize,
    pub(crate) filtered_refs_origin_below_terrain: usize,
    pub(crate) filtered_refs_mesh_contact_above_terrain: usize,
    pub(crate) filtered_refs_mesh_contact_below_terrain: usize,
    pub(crate) filtered_refs_mesh_contact_missing_terrain: usize,
    pub(crate) filtered_refs_static_bounds_occluded: usize,
    pub(crate) filtered_refs_static_bounds_fully_occluded: usize,
    pub(crate) filtered_refs_static_bounds_relocatable: usize,
    pub(crate) filtered_refs_static_bounds_blocked: usize,
}

#[derive(Serialize)]
pub(crate) struct ReferenceInspection {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) static_resolution: &'static str,
    pub(crate) mesh_contact_status: &'static str,
    pub(crate) deleted: bool,
    pub(crate) write_status: &'static str,
    pub(crate) origin: OriginInspection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) static_mesh: Option<StaticMeshInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mesh_contact_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mesh_contact: Option<MeshContactInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) static_bounds_occlusion: Option<StaticBoundsOcclusionInspection>,
}

#[derive(Serialize)]
pub(crate) struct OriginInspection {
    pub(crate) position: [f32; 3],
    pub(crate) terrain_z: Option<f32>,
    pub(crate) delta: Option<f32>,
    pub(crate) classification: &'static str,
}

#[derive(Serialize)]
pub(crate) struct StaticMeshInspection {
    pub(crate) id: String,
    pub(crate) mesh: String,
}

#[derive(Serialize)]
pub(crate) struct MeshContactInspection {
    pub(crate) position: [f32; 3],
    pub(crate) terrain_z: Option<f32>,
    pub(crate) terrain_normal: Option<[f32; 3]>,
    pub(crate) delta: Option<f32>,
    pub(crate) orientation_angle_degrees: Option<f32>,
    pub(crate) classification: &'static str,
}

#[derive(Serialize)]
pub(crate) struct StaticBoundsOcclusionInspection {
    pub(crate) status: &'static str,
    pub(crate) ratio: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) occluder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) occluder_cell: Option<[i32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) occluder_reference_key: Option<[u32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_bounds: Option<BoundsInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) occluder_bounds: Option<BoundsInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) intersection_volume: Option<f32>,
}

#[derive(Serialize)]
pub(crate) struct BoundsInspection {
    pub(crate) min: [f32; 3],
    pub(crate) max: [f32; 3],
}

#[derive(Default)]
pub(crate) struct TerrainInspectionReport {
    pub(crate) refs: usize,
    pub(crate) refs_deleted: usize,
    pub(crate) refs_with_mesh_contact: usize,
    pub(crate) refs_without_resolved_static: usize,
    pub(crate) refs_missing_mesh_contact: usize,
    pub(crate) refs_with_terrain: usize,
    pub(crate) refs_missing_terrain: usize,
    pub(crate) refs_above_terrain: usize,
    pub(crate) refs_below_terrain: usize,
    pub(crate) refs_contact_above_terrain: usize,
    pub(crate) refs_contact_below_terrain: usize,
    pub(crate) refs_contact_missing_terrain: usize,
    pub(crate) refs_static_bounds_occluded: usize,
    pub(crate) refs_static_bounds_fully_occluded: usize,
    pub(crate) refs_static_bounds_relocatable: usize,
    pub(crate) refs_static_bounds_blocked: usize,
}
