use serde::Serialize;

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
    pub(crate) refs_static_bounds_lightly_occluded: usize,
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
    pub(crate) refs_static_bounds_lightly_occluded: usize,
    pub(crate) refs_static_bounds_blocked: usize,
}
