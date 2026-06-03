// SPDX-License-Identifier: GPL-3.0-only

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::Serialize;

use super::{cells::CellCoord, mesh::WorldAabb};

#[derive(Serialize)]
pub(crate) struct WriteReport {
    pub(crate) written: bool,
    pub(crate) destination_plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) no_write_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backup_plugin: Option<String>,
    pub(crate) adjusted_refs: usize,
    pub(crate) deleted_refs: usize,
    pub(crate) water_deleted_refs: usize,
    pub(crate) moved_refs: usize,
    pub(crate) oriented_refs: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) adjustments: Vec<WriteAdjustment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) deletions: Vec<WriteStaticBoundsDeletion>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) water_deletions: Vec<WriteWaterDeletion>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) moves: Vec<WriteStaticBoundsMove>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) orientations: Vec<WriteOrientation>,
}

impl WriteReport {
    pub(crate) fn written(
        destination_path: &Path,
        backup_path: Option<&Path>,
        plan: WritePlan,
    ) -> Self {
        Self::from_plan(
            true,
            destination_path,
            None,
            backup_path.map(|path| path.display().to_string()),
            plan,
        )
    }

    pub(crate) fn not_written(
        destination_path: &Path,
        plan: WritePlan,
        reason: &'static str,
    ) -> Self {
        Self::from_plan(false, destination_path, Some(reason), None, plan)
    }

    fn from_plan(
        written: bool,
        destination_path: &Path,
        no_write_reason: Option<&'static str>,
        backup_plugin: Option<String>,
        mut plan: WritePlan,
    ) -> Self {
        plan.sort_records();
        Self {
            written,
            destination_plugin: destination_path.display().to_string(),
            no_write_reason,
            backup_plugin,
            adjusted_refs: plan.adjusted_refs,
            deleted_refs: plan.deleted_refs,
            water_deleted_refs: plan.water_deleted_refs,
            moved_refs: plan.moved_refs,
            oriented_refs: plan.oriented_refs,
            adjustments: plan.adjustments,
            deletions: plan.deletions,
            water_deletions: plan.water_deletions,
            moves: plan.moves,
            orientations: plan.orientations,
        }
    }

    pub(crate) fn summary(&self) -> WriteSummary {
        WriteSummary {
            written: self.written,
            destination_plugin: self.destination_plugin.clone(),
            no_write_reason: self.no_write_reason,
            backup_plugin: self.backup_plugin.clone(),
            adjusted_refs: self.adjusted_refs,
            deleted_refs: self.deleted_refs,
            water_deleted_refs: self.water_deleted_refs,
            moved_refs: self.moved_refs,
            oriented_refs: self.oriented_refs,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct WriteSummary {
    pub(crate) written: bool,
    pub(crate) destination_plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) no_write_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backup_plugin: Option<String>,
    pub(crate) adjusted_refs: usize,
    pub(crate) deleted_refs: usize,
    pub(crate) water_deleted_refs: usize,
    pub(crate) moved_refs: usize,
    pub(crate) oriented_refs: usize,
}

#[derive(Default)]
pub(crate) struct WritePlan {
    pub(crate) adjusted_refs: usize,
    pub(crate) deleted_refs: usize,
    pub(crate) water_deleted_refs: usize,
    pub(crate) moved_refs: usize,
    pub(crate) oriented_refs: usize,
    pub(crate) adjustments: Vec<WriteAdjustment>,
    pub(crate) deletions: Vec<WriteStaticBoundsDeletion>,
    pub(crate) water_deletions: Vec<WriteWaterDeletion>,
    pub(crate) moves: Vec<WriteStaticBoundsMove>,
    pub(crate) orientations: Vec<WriteOrientation>,
    pub(crate) static_bounds_analysis: Vec<WriteStaticBoundsAnalysis>,
}

impl WritePlan {
    pub(crate) const fn changed_refs(&self) -> usize {
        self.adjusted_refs + self.deleted_refs + self.moved_refs + self.oriented_refs
    }

    fn sort_records(&mut self) {
        self.adjustments
            .sort_by_key(|adjustment| (adjustment.cell, adjustment.reference_key));
        self.deletions
            .sort_by_key(|deletion| (deletion.cell, deletion.reference_key));
        self.water_deletions
            .sort_by_key(|deletion| (deletion.cell, deletion.reference_key));
        self.moves
            .sort_by_key(|move_| (move_.cell, move_.reference_key));
        self.orientations
            .sort_by_key(|orientation| (orientation.cell, orientation.reference_key));
    }
}

#[derive(Serialize)]
pub(crate) struct WriteAdjustment {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) old_z: f32,
    pub(crate) new_z: f32,
    pub(crate) applied_delta: f32,
    pub(crate) sample_kind: &'static str,
    pub(crate) contact_position: [f32; 3],
    pub(crate) terrain_z: f32,
}

#[derive(Serialize)]
pub(crate) struct WriteStaticBoundsDeletion {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) occlusion_ratio: f32,
    pub(crate) occluder_id: String,
    pub(crate) occluder_cell: [i32; 2],
    pub(crate) occluder_reference_key: [u32; 2],
}

#[derive(Serialize)]
pub(crate) struct WriteWaterDeletion {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) old_z: f32,
    pub(crate) new_z: f32,
    pub(crate) water_level: f32,
}

#[derive(Serialize)]
pub(crate) struct WriteStaticBoundsMove {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) occlusion_ratio: f32,
    pub(crate) block_reason: &'static str,
    pub(crate) old_position: [f32; 3],
    pub(crate) new_position: [f32; 3],
    pub(crate) occluder_id: String,
    pub(crate) occluder_cell: [i32; 2],
    pub(crate) occluder_reference_key: [u32; 2],
}

#[derive(Serialize)]
pub(crate) struct WriteOrientation {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) old_rotation: [f32; 3],
    pub(crate) new_rotation: [f32; 3],
    pub(crate) terrain_normal: [f32; 3],
    pub(crate) angle_degrees: f32,
    pub(crate) sample_kind: &'static str,
    pub(crate) sample_position: [f32; 3],
}

#[derive(Clone)]
pub(crate) struct WriteStaticBoundsAnalysis {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) status: &'static str,
    pub(crate) ratio: f32,
    pub(crate) occluder_id: Option<String>,
    pub(crate) occluder_cell: Option<[i32; 2]>,
    pub(crate) occluder_reference_key: Option<[u32; 2]>,
    pub(crate) target_bounds: Option<WorldAabb>,
    pub(crate) occluder_bounds: Option<WorldAabb>,
    pub(crate) intersection_volume: Option<f32>,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct AdjustedRefKey {
    cell: [i32; 2],
    reference_key: [u32; 2],
}

impl AdjustedRefKey {
    const fn new(cell: CellCoord, key: (u32, u32)) -> Self {
        Self {
            cell: [cell.0, cell.1],
            reference_key: [key.0, key.1],
        }
    }

    const fn from_adjustment(adjustment: &WriteAdjustment) -> Self {
        Self {
            cell: adjustment.cell,
            reference_key: adjustment.reference_key,
        }
    }
}

fn adjusted_ref_keys(adjustments: &[WriteAdjustment]) -> BTreeSet<AdjustedRefKey> {
    adjustments
        .iter()
        .map(AdjustedRefKey::from_adjustment)
        .collect()
}

fn adjusted_ref_keys_from_deletions(
    deletions: &[WriteStaticBoundsDeletion],
) -> BTreeSet<AdjustedRefKey> {
    deletions
        .iter()
        .map(|deletion| AdjustedRefKey {
            cell: deletion.cell,
            reference_key: deletion.reference_key,
        })
        .collect()
}

fn adjusted_ref_keys_from_water_deletions(
    deletions: &[WriteWaterDeletion],
) -> BTreeSet<AdjustedRefKey> {
    deletions
        .iter()
        .map(|deletion| AdjustedRefKey {
            cell: deletion.cell,
            reference_key: deletion.reference_key,
        })
        .collect()
}

fn adjusted_ref_keys_from_moves(moves: &[WriteStaticBoundsMove]) -> BTreeSet<AdjustedRefKey> {
    moves
        .iter()
        .map(|move_| AdjustedRefKey {
            cell: move_.cell,
            reference_key: move_.reference_key,
        })
        .collect()
}

fn adjusted_ref_keys_from_orientations(
    orientations: &[WriteOrientation],
) -> BTreeSet<AdjustedRefKey> {
    orientations
        .iter()
        .map(|orientation| AdjustedRefKey {
            cell: orientation.cell,
            reference_key: orientation.reference_key,
        })
        .collect()
}

pub(crate) struct WriteStatusIndex {
    adjusted: BTreeSet<AdjustedRefKey>,
    deleted: BTreeSet<AdjustedRefKey>,
    water_deleted: BTreeSet<AdjustedRefKey>,
    moved: BTreeSet<AdjustedRefKey>,
    oriented: BTreeSet<AdjustedRefKey>,
    static_bounds: BTreeMap<AdjustedRefKey, WriteStaticBoundsAnalysis>,
}

impl WriteStatusIndex {
    pub(crate) fn from_plan(plan: &WritePlan) -> Self {
        Self {
            adjusted: adjusted_ref_keys(&plan.adjustments),
            deleted: adjusted_ref_keys_from_deletions(&plan.deletions),
            water_deleted: adjusted_ref_keys_from_water_deletions(&plan.water_deletions),
            moved: adjusted_ref_keys_from_moves(&plan.moves),
            oriented: adjusted_ref_keys_from_orientations(&plan.orientations),
            static_bounds: plan
                .static_bounds_analysis
                .iter()
                .cloned()
                .map(|analysis| {
                    (
                        AdjustedRefKey {
                            cell: analysis.cell,
                            reference_key: analysis.reference_key,
                        },
                        analysis,
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn is_adjusted(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.adjusted.contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn is_deleted(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.deleted.contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn is_water_deleted(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.water_deleted.contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn is_moved(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.moved.contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn is_oriented(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.oriented.contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn static_bounds_analysis(
        &self,
        cell: CellCoord,
        key: (u32, u32),
    ) -> Option<&WriteStaticBoundsAnalysis> {
        self.static_bounds.get(&AdjustedRefKey::new(cell, key))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        WriteAdjustment, WriteOrientation, WritePlan, WriteReport, WriteStaticBoundsDeletion,
        WriteStatusIndex, WriteWaterDeletion,
    };

    #[test]
    fn write_status_index_uses_adjusted_key_set() {
        let plan = WritePlan {
            adjusted_refs: 1,
            adjustments: vec![write_adjustment()],
            ..WritePlan::default()
        };
        let index = WriteStatusIndex::from_plan(&plan);

        assert!(index.is_adjusted((1, 2), (3, 4)));
        assert!(!index.is_adjusted((1, 2), (3, 5)));
    }

    #[test]
    fn write_status_index_tracks_oriented_refs() {
        let plan = WritePlan {
            oriented_refs: 1,
            orientations: vec![write_orientation_at([1, 2], [3, 4])],
            ..WritePlan::default()
        };
        let index = WriteStatusIndex::from_plan(&plan);

        assert!(index.is_oriented((1, 2), (3, 4)));
        assert!(!index.is_oriented((1, 2), (3, 5)));
    }

    #[test]
    fn write_status_index_tracks_water_deleted_refs() {
        let plan = WritePlan {
            water_deleted_refs: 1,
            water_deletions: vec![write_water_deletion_at([1, 2], [3, 4])],
            ..WritePlan::default()
        };
        let index = WriteStatusIndex::from_plan(&plan);

        assert!(index.is_water_deleted((1, 2), (3, 4)));
        assert!(!index.is_water_deleted((1, 2), (3, 5)));
    }

    #[test]
    fn write_report_sorts_change_records() {
        let report = WriteReport::not_written(
            Path::new("plugin.omwaddon"),
            WritePlan {
                adjusted_refs: 2,
                deleted_refs: 2,
                water_deleted_refs: 1,
                adjustments: vec![
                    write_adjustment_at([1, 0], [4, 0]),
                    write_adjustment_at([0, 0], [9, 0]),
                ],
                deletions: vec![
                    write_deletion_at([1, 0], [4, 0]),
                    write_deletion_at([0, 0], [9, 0]),
                ],
                water_deletions: vec![write_water_deletion_at([2, 0], [8, 0])],
                ..WritePlan::default()
            },
            "no_refs_changed",
        );

        assert_eq!(report.adjustments[0].cell, [0, 0]);
        assert_eq!(report.adjustments[0].reference_key, [9, 0]);
        assert_eq!(report.adjustments[1].cell, [1, 0]);
        assert_eq!(report.deletions[0].cell, [0, 0]);
        assert_eq!(report.deletions[1].cell, [1, 0]);
        assert_eq!(report.water_deletions[0].cell, [2, 0]);
    }

    fn write_adjustment() -> WriteAdjustment {
        write_adjustment_at([1, 2], [3, 4])
    }

    fn write_adjustment_at(cell: [i32; 2], reference_key: [u32; 2]) -> WriteAdjustment {
        WriteAdjustment {
            cell,
            reference_key,
            id: "grass".to_owned(),
            old_z: 10.0,
            new_z: 12.0,
            applied_delta: 2.0,
            sample_kind: "contact",
            contact_position: [0.0, 0.0, 7.0],
            terrain_z: 9.0,
        }
    }

    fn write_deletion_at(cell: [i32; 2], reference_key: [u32; 2]) -> WriteStaticBoundsDeletion {
        WriteStaticBoundsDeletion {
            cell,
            reference_key,
            id: "grass".to_owned(),
            occlusion_ratio: 1.0,
            occluder_id: "rock".to_owned(),
            occluder_cell: [0, 0],
            occluder_reference_key: [1, 0],
        }
    }

    fn write_water_deletion_at(cell: [i32; 2], reference_key: [u32; 2]) -> WriteWaterDeletion {
        WriteWaterDeletion {
            cell,
            reference_key,
            id: "grass".to_owned(),
            old_z: 10.0,
            new_z: -2.0,
            water_level: -1.0,
        }
    }

    fn write_orientation_at(cell: [i32; 2], reference_key: [u32; 2]) -> WriteOrientation {
        WriteOrientation {
            cell,
            reference_key,
            id: "grass".to_owned(),
            old_rotation: [0.0, 0.0, 0.0],
            new_rotation: [0.1, 0.2, 0.0],
            terrain_normal: [0.0, 0.2, 0.98],
            angle_degrees: 10.0,
            sample_kind: "contact",
            sample_position: [0.0, 0.0, 10.0],
        }
    }
}
