use std::collections::{BTreeMap, BTreeSet};

use tes3::esp::{Cell, Plugin};

use super::{
    args::IdFilter,
    cells::CellCoord,
    mesh::{MeshCache, StaticMeshIndex, WorldAabb},
    occlusion::{StaticOccluder, StaticOccluderIndex},
};

const HUGE_OCCLUDER_FOOTPRINT_SIDE: f32 = 4096.0;

#[derive(Clone, Debug, Default, serde::Serialize)]
pub(crate) struct StaticOccluderBuildReport {
    pub(crate) active_refs_scanned: usize,
    pub(crate) target_refs_excluded: usize,
    pub(crate) regex_excluded: usize,
    pub(crate) unresolved_static: usize,
    pub(crate) missing_bounds: usize,
    pub(crate) resolved_bounds: usize,
    pub(crate) huge_footprint: usize,
    pub(crate) huge_footprint_side_threshold: f32,
}

pub(crate) fn build_static_occluders(
    active_plugins: &[Plugin],
    active_cells: &BTreeSet<CellCoord>,
    static_index: &StaticMeshIndex,
    mesh_bounds: &mut MeshCache<'_>,
    target_static_ids: &BTreeSet<String>,
    occluder_filter: &IdFilter,
) -> (StaticOccluderIndex, StaticOccluderBuildReport) {
    let effective_refs = effective_active_refs(active_plugins, active_cells);
    let mut build_report = StaticOccluderBuildReport {
        active_refs_scanned: effective_refs.len(),
        huge_footprint_side_threshold: HUGE_OCCLUDER_FOOTPRINT_SIDE,
        ..StaticOccluderBuildReport::default()
    };
    let mut occluders = Vec::new();

    for (key, reference) in effective_refs {
        let reference_id_key = reference.id.to_lowercase();
        if target_static_ids.contains(&reference_id_key) {
            build_report.target_refs_excluded += 1;
            continue;
        }
        if !occluder_filter.includes(&reference.id) {
            build_report.regex_excluded += 1;
            continue;
        }

        let Some(static_mesh) = static_index.get_normalized_key(&reference_id_key) else {
            build_report.unresolved_static += 1;
            continue;
        };
        let Ok(bounds) = mesh_bounds.bounds(static_mesh) else {
            build_report.missing_bounds += 1;
            continue;
        };
        let world_bounds =
            bounds.world_aabb(reference.translation, reference.rotation, reference.scale);
        build_report.resolved_bounds += 1;
        if huge_footprint(world_bounds) {
            build_report.huge_footprint += 1;
        }

        occluders.push(StaticOccluder {
            id: reference.id.clone(),
            cell: [key.cell.0, key.cell.1],
            reference_key: [key.reference.0, key.reference.1],
            bounds: world_bounds,
        });
    }

    (StaticOccluderIndex::new(occluders), build_report)
}

#[cfg(test)]
fn should_include_occluder(
    reference_id: &str,
    reference_id_key: &str,
    target_static_ids: &BTreeSet<String>,
    occluder_filter: &IdFilter,
) -> bool {
    !target_static_ids.contains(reference_id_key) && occluder_filter.includes(reference_id)
}

fn huge_footprint(bounds: WorldAabb) -> bool {
    let width = bounds.max[0] - bounds.min[0];
    let depth = bounds.max[1] - bounds.min[1];
    width > HUGE_OCCLUDER_FOOTPRINT_SIDE || depth > HUGE_OCCLUDER_FOOTPRINT_SIDE
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct EffectiveRefKey {
    cell: CellCoord,
    reference: (u32, u32),
}

fn effective_active_refs<'a>(
    active_plugins: &'a [Plugin],
    active_cells: &BTreeSet<CellCoord>,
) -> BTreeMap<EffectiveRefKey, &'a tes3::esp::Reference> {
    let mut refs = BTreeMap::new();

    for plugin in active_plugins {
        for cell in plugin.objects_of_type::<Cell>() {
            if !cell.is_exterior() || !active_cells.contains(&cell.data.grid) {
                continue;
            }

            for (reference_key, reference) in &cell.references {
                let key = EffectiveRefKey {
                    cell: reference.moved_cell.unwrap_or(cell.data.grid),
                    reference: *reference_key,
                };
                if reference.deleted == Some(true) {
                    refs.remove(&key);
                } else {
                    refs.insert(key, reference);
                }
            }
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use crate::unclip::args::IdFilter;

    use super::{effective_active_refs, should_include_occluder};

    #[test]
    fn effective_active_refs_apply_later_deletions() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_at_z(0.0),
            )]))],
        };
        let deleted = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([((1, 2), deleted_ref())]))],
        };

        let plugins = [first, deleted];
        let refs = effective_active_refs(&plugins, &BTreeSet::from([(0, 0)]));

        assert!(refs.is_empty());
    }

    #[test]
    fn effective_active_refs_key_moved_refs_by_original_cell() {
        let mut moved = reference_at_z(0.0);
        moved.moved_cell = Some((0, 0));
        let mut deleted = deleted_ref();
        deleted.moved_cell = Some((0, 0));
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((1, 2), moved)],
            ))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([((1, 2), deleted)]))],
        };

        let plugins = [first, second];
        let refs = effective_active_refs(&plugins, &BTreeSet::from([(0, 0), (1, 0)]));

        assert!(refs.is_empty());
    }

    #[test]
    fn occluder_filter_excludes_matching_static_refs() {
        let target_static_ids = BTreeSet::new();
        let filter = IdFilter::new(&[], &["^tree_huge$".to_owned()]).unwrap();

        assert!(!should_include_occluder(
            "tree_huge",
            "tree_huge",
            &target_static_ids,
            &filter
        ));
        assert!(should_include_occluder(
            "terrain_rock",
            "terrain_rock",
            &target_static_ids,
            &filter
        ));
    }

    fn reference_at_z(z: f32) -> Reference {
        Reference {
            id: "grass".to_owned(),
            translation: [0.0, 0.0, z],
            ..Reference::default()
        }
    }

    fn deleted_ref() -> Reference {
        Reference {
            deleted: Some(true),
            id: "rock".to_owned(),
            ..Reference::default()
        }
    }

    fn exterior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        exterior_cell_at((0, 0), refs)
    }

    fn exterior_cell_at(
        grid: (i32, i32),
        refs: impl IntoIterator<Item = ((u32, u32), Reference)>,
    ) -> Cell {
        let mut cell = Cell {
            data: CellData {
                grid,
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }
}
