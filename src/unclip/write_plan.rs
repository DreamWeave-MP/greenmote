use std::{collections::BTreeSet, path::Path};

use serde::Serialize;

use super::cells::CellCoord;

#[derive(Serialize)]
pub(crate) struct WriteReport {
    pub(crate) written: bool,
    pub(crate) destination_plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backup_plugin: Option<String>,
    pub(crate) adjusted_refs: usize,
    pub(crate) deleted_refs: usize,
    pub(crate) moved_refs: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) adjustments: Vec<WriteAdjustment>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) deletions: Vec<WriteStaticBoundsDeletion>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) moves: Vec<WriteStaticBoundsMove>,
    #[serde(skip)]
    adjusted_ref_keys: BTreeSet<AdjustedRefKey>,
    #[serde(skip)]
    deleted_ref_keys: BTreeSet<AdjustedRefKey>,
    #[serde(skip)]
    moved_ref_keys: BTreeSet<AdjustedRefKey>,
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
            backup_path.map(|path| path.display().to_string()),
            plan,
        )
    }

    pub(crate) fn not_written(destination_path: &Path, plan: WritePlan) -> Self {
        Self::from_plan(false, destination_path, None, plan)
    }

    fn from_plan(
        written: bool,
        destination_path: &Path,
        backup_plugin: Option<String>,
        plan: WritePlan,
    ) -> Self {
        let adjusted_ref_keys = adjusted_ref_keys(&plan.adjustments);
        let deleted_ref_keys = adjusted_ref_keys_from_deletions(&plan.deletions);
        let moved_ref_keys = adjusted_ref_keys_from_moves(&plan.moves);
        Self {
            written,
            destination_plugin: destination_path.display().to_string(),
            backup_plugin,
            adjusted_refs: plan.adjusted_refs,
            deleted_refs: plan.deleted_refs,
            moved_refs: plan.moved_refs,
            adjustments: plan.adjustments,
            deletions: plan.deletions,
            moves: plan.moves,
            adjusted_ref_keys,
            deleted_ref_keys,
            moved_ref_keys,
        }
    }

    pub(crate) fn summary(&self) -> WriteSummary {
        WriteSummary {
            written: self.written,
            destination_plugin: self.destination_plugin.clone(),
            backup_plugin: self.backup_plugin.clone(),
            adjusted_refs: self.adjusted_refs,
            deleted_refs: self.deleted_refs,
            moved_refs: self.moved_refs,
        }
    }

    pub(crate) fn is_adjusted(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.adjusted_ref_keys
            .contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn is_deleted(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.deleted_ref_keys
            .contains(&AdjustedRefKey::new(cell, key))
    }

    pub(crate) fn is_moved(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.moved_ref_keys
            .contains(&AdjustedRefKey::new(cell, key))
    }
}

#[derive(Serialize)]
pub(crate) struct WriteSummary {
    pub(crate) written: bool,
    pub(crate) destination_plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backup_plugin: Option<String>,
    pub(crate) adjusted_refs: usize,
    pub(crate) deleted_refs: usize,
    pub(crate) moved_refs: usize,
}

#[derive(Default)]
pub(crate) struct WritePlan {
    pub(crate) adjusted_refs: usize,
    pub(crate) deleted_refs: usize,
    pub(crate) moved_refs: usize,
    pub(crate) adjustments: Vec<WriteAdjustment>,
    pub(crate) deletions: Vec<WriteStaticBoundsDeletion>,
    pub(crate) moves: Vec<WriteStaticBoundsMove>,
}

impl WritePlan {
    pub(crate) const fn changed_refs(&self) -> usize {
        self.adjusted_refs + self.deleted_refs + self.moved_refs
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
pub(crate) struct WriteStaticBoundsMove {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) occlusion_ratio: f32,
    pub(crate) old_position: [f32; 3],
    pub(crate) new_position: [f32; 3],
    pub(crate) occluder_id: String,
    pub(crate) occluder_cell: [i32; 2],
    pub(crate) occluder_reference_key: [u32; 2],
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

fn adjusted_ref_keys_from_moves(moves: &[WriteStaticBoundsMove]) -> BTreeSet<AdjustedRefKey> {
    moves
        .iter()
        .map(|move_| AdjustedRefKey {
            cell: move_.cell,
            reference_key: move_.reference_key,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{WriteAdjustment, WritePlan, WriteReport};

    #[test]
    fn write_report_uses_adjusted_key_set() {
        let report = WriteReport::not_written(
            Path::new("plugin.omwaddon"),
            WritePlan {
                adjusted_refs: 1,
                adjustments: vec![write_adjustment()],
                ..WritePlan::default()
            },
        );

        assert!(report.is_adjusted((1, 2), (3, 4)));
        assert!(!report.is_adjusted((1, 2), (3, 5)));
    }

    fn write_adjustment() -> WriteAdjustment {
        WriteAdjustment {
            cell: [1, 2],
            reference_key: [3, 4],
            id: "grass".to_owned(),
            old_z: 10.0,
            new_z: 12.0,
            applied_delta: 2.0,
            contact_position: [0.0, 0.0, 7.0],
            terrain_z: 9.0,
        }
    }
}
