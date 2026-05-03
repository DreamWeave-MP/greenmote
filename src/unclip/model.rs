use serde::Serialize;

use super::{cells::CellCoord, write_plan::WriteReport};

pub(crate) const ORIGIN_TERRAIN_EPSILON: f32 = 0.5;
pub(crate) const CONTACT_TERRAIN_EPSILON: f32 = 0.5;

pub(crate) struct UnclipReportContext {
    pub(crate) target_plugin: String,
    target_exterior_cells: usize,
    active_cells: usize,
    loaded_terrain_cells_total: usize,
    missing_active_terrain_cells: Vec<CellCoord>,
    pub(crate) write: Option<WriteReport>,
}

impl UnclipReportContext {
    pub(crate) fn new(
        target_plugin_path: &std::path::Path,
        target_exterior_cells: usize,
        active_cells: usize,
        loaded_terrain_cells_total: usize,
        missing_active_terrain_cells: Vec<CellCoord>,
    ) -> Self {
        Self {
            target_plugin: target_plugin_path.display().to_string(),
            target_exterior_cells,
            active_cells,
            loaded_terrain_cells_total,
            missing_active_terrain_cells,
            write: None,
        }
    }

    pub(crate) fn missing_active_terrain_cells(&self) -> Vec<[i32; 2]> {
        self.missing_active_terrain_cells
            .iter()
            .map(|&(x, y)| [x, y])
            .collect()
    }

    pub(crate) fn summary(&self, inspection: &TerrainInspectionReport) -> UnclipSummary {
        let active_terrain_cells_missing = self.missing_active_terrain_cells.len();
        UnclipSummary {
            target_exterior_cells: self.target_exterior_cells,
            active_cells: self.active_cells,
            loaded_terrain_cells_total: self.loaded_terrain_cells_total,
            active_terrain_cells_loaded: self.active_cells - active_terrain_cells_missing,
            active_terrain_cells_missing,
            refs: inspection.refs,
            refs_actionable: inspection.refs - inspection.refs_deleted,
            refs_deleted: inspection.refs_deleted,
            refs_with_mesh_contact: inspection.refs_with_mesh_contact,
            refs_without_resolved_static: inspection.refs_without_resolved_static,
            refs_missing_mesh_contact: inspection.refs_missing_mesh_contact,
            refs_with_terrain: inspection.refs_with_terrain,
            refs_missing_terrain: inspection.refs_missing_terrain,
            refs_origin_above_terrain: inspection.refs_above_terrain,
            refs_origin_below_terrain: inspection.refs_below_terrain,
            refs_mesh_contact_above_terrain: inspection.refs_contact_above_terrain,
            refs_mesh_contact_below_terrain: inspection.refs_contact_below_terrain,
            refs_mesh_contact_missing_terrain: inspection.refs_contact_missing_terrain,
            refs_static_bounds_occluded: inspection.refs_static_bounds_occluded,
            refs_static_bounds_fully_occluded: inspection.refs_static_bounds_fully_occluded,
            refs_static_bounds_relocatable: inspection.refs_static_bounds_relocatable,
            refs_static_bounds_blocked: inspection.refs_static_bounds_blocked,
            origin_terrain_epsilon: ORIGIN_TERRAIN_EPSILON,
            mesh_contact_terrain_epsilon: CONTACT_TERRAIN_EPSILON,
        }
    }
}

#[cfg(test)]
impl UnclipReportContext {
    pub(crate) fn new_for_test(target_plugin: &str) -> Self {
        Self {
            target_plugin: target_plugin.to_owned(),
            target_exterior_cells: 0,
            active_cells: 0,
            loaded_terrain_cells_total: 0,
            missing_active_terrain_cells: Vec::new(),
            write: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct UnclipSummary {
    pub(crate) target_exterior_cells: usize,
    pub(crate) active_cells: usize,
    pub(crate) loaded_terrain_cells_total: usize,
    pub(crate) active_terrain_cells_loaded: usize,
    pub(crate) active_terrain_cells_missing: usize,
    pub(crate) refs: usize,
    pub(crate) refs_actionable: usize,
    pub(crate) refs_deleted: usize,
    pub(crate) refs_with_mesh_contact: usize,
    pub(crate) refs_without_resolved_static: usize,
    pub(crate) refs_missing_mesh_contact: usize,
    pub(crate) refs_with_terrain: usize,
    pub(crate) refs_missing_terrain: usize,
    pub(crate) refs_origin_above_terrain: usize,
    pub(crate) refs_origin_below_terrain: usize,
    pub(crate) refs_mesh_contact_above_terrain: usize,
    pub(crate) refs_mesh_contact_below_terrain: usize,
    pub(crate) refs_mesh_contact_missing_terrain: usize,
    pub(crate) refs_static_bounds_occluded: usize,
    pub(crate) refs_static_bounds_fully_occluded: usize,
    pub(crate) refs_static_bounds_relocatable: usize,
    pub(crate) refs_static_bounds_blocked: usize,
    pub(crate) origin_terrain_epsilon: f32,
    pub(crate) mesh_contact_terrain_epsilon: f32,
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
    pub(crate) delta: Option<f32>,
    pub(crate) classification: &'static str,
}

#[derive(Serialize)]
pub(crate) struct StaticBoundsOcclusionInspection {
    pub(crate) status: &'static str,
    pub(crate) ratio: f32,
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
