use std::collections::{BTreeMap, BTreeSet};

use tes3::esp::{Cell, Plugin};

use super::{
    args::IdFilter,
    cells::CellCoord,
    mesh::{MeshCache, MeshColliderParts, StaticMeshIndex, WorldAabb},
    occlusion::{StaticOccluder, StaticOccluderIndex},
    physics::RapierCollider,
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
    let effective_refs = effective_static_occluder_refs(
        active_plugins,
        active_cells,
        static_index,
        target_static_ids,
        occluder_filter,
    );
    let mut build_report = StaticOccluderBuildReport {
        active_refs_scanned: effective_refs.len(),
        target_refs_excluded: effective_refs
            .values()
            .filter(|state| matches!(state, EffectiveRefState::Excluded(ExclusionReason::Target)))
            .count(),
        regex_excluded: effective_refs
            .values()
            .filter(|state| matches!(state, EffectiveRefState::Excluded(ExclusionReason::Regex)))
            .count(),
        unresolved_static: effective_refs
            .values()
            .filter(|state| {
                matches!(
                    state,
                    EffectiveRefState::Excluded(ExclusionReason::UnresolvedStatic)
                )
            })
            .count(),
        huge_footprint_side_threshold: HUGE_OCCLUDER_FOOTPRINT_SIDE,
        ..StaticOccluderBuildReport::default()
    };
    let mut occluders = Vec::new();

    for (key, state) in effective_refs {
        let EffectiveRefState::Candidate {
            reference,
            static_mesh,
        } = state
        else {
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
        let collider_parts = MeshColliderParts::from_mesh_aabb(bounds);

        occluders.push(StaticOccluder {
            id: reference.id.clone(),
            cell: [key.cell.0, key.cell.1],
            reference_key: [key.reference.0, key.reference.1],
            bounds: world_bounds,
            collider: RapierCollider::from_mesh_collider_parts(
                &collider_parts,
                reference.translation,
                reference.rotation,
                reference.scale,
            ),
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

enum EffectiveRefState<'a> {
    Candidate {
        reference: &'a tes3::esp::Reference,
        static_mesh: &'a super::mesh::StaticMesh,
    },
    Excluded(ExclusionReason),
}

#[derive(Clone, Copy)]
enum ExclusionReason {
    Target,
    Regex,
    UnresolvedStatic,
}

fn effective_static_occluder_refs<'a>(
    active_plugins: &'a [Plugin],
    active_cells: &BTreeSet<CellCoord>,
    static_index: &'a StaticMeshIndex,
    target_static_ids: &BTreeSet<String>,
    occluder_filter: &IdFilter,
) -> BTreeMap<EffectiveRefKey, EffectiveRefState<'a>> {
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
                    let reference_id_key = reference.id.to_lowercase();
                    let state = if target_static_ids.contains(&reference_id_key) {
                        EffectiveRefState::Excluded(ExclusionReason::Target)
                    } else if !occluder_filter.includes(&reference.id) {
                        EffectiveRefState::Excluded(ExclusionReason::Regex)
                    } else if let Some(static_mesh) =
                        static_index.get_normalized_key(&reference_id_key)
                    {
                        EffectiveRefState::Candidate {
                            reference,
                            static_mesh,
                        }
                    } else {
                        EffectiveRefState::Excluded(ExclusionReason::UnresolvedStatic)
                    };
                    refs.insert(key, state);
                }
            }
        }
    }

    refs
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::PathBuf,
    };

    use tes3::esp::{Cell, CellData, Plugin, Reference, Static, TES3Object};
    use vfstool_lib::VFS;

    use crate::unclip::{args::IdFilter, cells::CellCoord};

    use super::{
        EffectiveRefState, ExclusionReason, build_static_occluders, effective_static_occluder_refs,
        should_include_occluder,
    };

    #[test]
    fn effective_static_occluder_refs_apply_later_deletions() {
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
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &plugins,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &BTreeSet::new(),
        );

        assert!(refs.is_empty());
    }

    #[test]
    fn effective_static_occluder_refs_key_moved_refs_by_original_cell() {
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
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &plugins,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &static_index,
            &BTreeSet::new(),
        );

        assert!(refs.is_empty());
    }

    #[test]
    fn later_live_non_candidate_suppresses_earlier_candidate() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("grass"),
            )]))],
        };
        let plugins = [first, second];
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &plugins,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &BTreeSet::from(["grass".to_owned()]),
        );

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            refs.values().next(),
            Some(EffectiveRefState::Excluded(ExclusionReason::Target))
        ));
    }

    #[test]
    fn moved_cell_non_candidate_suppresses_earlier_candidate() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("rock"),
            )]))],
        };
        let mut moved_non_candidate = reference_with_id("grass");
        moved_non_candidate.moved_cell = Some((0, 0));
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((1, 2), moved_non_candidate)],
            ))],
        };
        let plugins = [first, second];
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &plugins,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &static_index,
            &BTreeSet::from(["grass".to_owned()]),
        );

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            refs.values().next(),
            Some(EffectiveRefState::Excluded(ExclusionReason::Target))
        ));
    }

    #[test]
    fn active_refs_scanned_stays_final_effective_live_ref_count() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((1, 1), reference_with_id("rock")),
                ((1, 2), reference_with_id("rock")),
            ]))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((1, 2), deleted_ref()),
                ((1, 3), reference_with_id("unknown")),
            ]))],
        };
        let plugins = [first, second];
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);
        let (_, report) = build_static_occluders(
            &plugins,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
        );

        assert_eq!(report.active_refs_scanned, 2);
        assert_eq!(report.missing_bounds, 1);
        assert_eq!(report.unresolved_static, 1);
    }

    #[test]
    fn overwritten_candidate_does_not_load_missing_mesh_bounds() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("grass"),
            )]))],
        };
        let plugins = [first, second];
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &plugins,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::from(["grass".to_owned()]),
            &IdFilter::new(&[], &[]).unwrap(),
        );

        assert_eq!(report.active_refs_scanned, 1);
        assert_eq!(report.target_refs_excluded, 1);
        assert_eq!(report.missing_bounds, 0);
        assert_eq!(report.resolved_bounds, 0);
    }

    #[test]
    fn overwritten_candidate_by_regex_excluded_ref_does_not_load_missing_mesh_bounds() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("tree_huge"),
            )]))],
        };
        let plugins = [first, second];
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &plugins,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &["^tree_huge$".to_owned()]).unwrap(),
        );

        assert_eq!(report.active_refs_scanned, 1);
        assert_eq!(report.regex_excluded, 1);
        assert_eq!(report.missing_bounds, 0);
        assert_eq!(report.resolved_bounds, 0);
    }

    #[test]
    fn overwritten_candidate_by_unresolved_static_does_not_load_missing_mesh_bounds() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("unknown"),
            )]))],
        };
        let plugins = [first, second];
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &plugins,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
        );

        assert_eq!(report.active_refs_scanned, 1);
        assert_eq!(report.unresolved_static, 1);
        assert_eq!(report.missing_bounds, 0);
        assert_eq!(report.resolved_bounds, 0);
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

    fn reference_with_id(id: &str) -> Reference {
        Reference {
            id: id.to_owned(),
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

    fn static_index() -> crate::unclip::mesh::StaticMeshIndex {
        let rock = Static {
            id: "rock".to_owned(),
            mesh: "rock.nif".to_owned(),
            ..Static::default()
        };
        let grass = Static {
            id: "grass".to_owned(),
            mesh: "grass.nif".to_owned(),
            ..Static::default()
        };
        crate::unclip::mesh::StaticMeshIndex::from_statics([&rock, &grass])
    }

    fn effective_occluder_refs<'a>(
        plugins: &'a [Plugin],
        active_cells: &BTreeSet<CellCoord>,
        static_index: &'a crate::unclip::mesh::StaticMeshIndex,
        target_static_ids: &BTreeSet<String>,
    ) -> BTreeMap<super::EffectiveRefKey, EffectiveRefState<'a>> {
        effective_static_occluder_refs(
            plugins,
            active_cells,
            static_index,
            target_static_ids,
            &IdFilter::new(&[], &[]).unwrap(),
        )
    }
}
