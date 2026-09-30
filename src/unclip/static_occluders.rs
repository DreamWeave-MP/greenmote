// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    collections::{BTreeMap, BTreeSet},
    io,
};

use rayon::prelude::*;
use tes3::esp::{Cell, Plugin};

use crate::groundcover::CancellationToken;

use super::{
    args::IdFilter,
    cells::CellCoord,
    mesh::{MeshCache, MeshColliderSource, StaticMeshIndex, WorldAabb},
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
    pub(crate) invalid_master_index: usize,
    pub(crate) missing_bounds: usize,
    pub(crate) missing_meshes: Vec<MissingOccluderMesh>,
    pub(crate) no_collision: usize,
    pub(crate) resolved_bounds: usize,
    pub(crate) collision_source: usize,
    pub(crate) visible_fallback_source: usize,
    pub(crate) collider_part_fallbacks: usize,
    pub(crate) huge_footprint: usize,
    pub(crate) huge_footprint_side_threshold: f32,
}

/// An occluder static whose mesh could not be loaded from the VFS.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
pub(crate) struct MissingOccluderMesh {
    pub(crate) static_id: String,
    pub(crate) mesh_path: String,
    pub(crate) error: String,
}

/// A loaded plugin together with its lowercased file name, used to resolve
/// plugin-relative master indices into global reference owners.
pub(crate) struct NamedPlugin<'a> {
    /// Lowercased plugin file name, e.g. "morrowind.esm".
    pub(crate) name: String,
    pub(crate) plugin: &'a Plugin,
}

pub(crate) fn build_static_occluders<'a>(
    active_plugins: &[NamedPlugin<'a>],
    active_cells: &BTreeSet<CellCoord>,
    static_index: &'a StaticMeshIndex,
    mesh_bounds: &mut MeshCache<'_>,
    target_static_ids: &BTreeSet<String>,
    occluder_filter: &IdFilter,
    cancellation: &CancellationToken,
) -> io::Result<(StaticOccluderIndex, StaticOccluderBuildReport)> {
    let scan = effective_static_occluder_refs(
        active_plugins,
        active_cells,
        static_index,
        target_static_ids,
        occluder_filter,
        cancellation,
    )?;
    let mut build_report = initial_build_report(&scan);
    let effective_refs = scan.refs;
    let mut missing_meshes: BTreeMap<String, MissingOccluderMesh> = BTreeMap::new();
    let mut pending = Vec::new();
    mesh_bounds.preload(
        effective_refs
            .values()
            .filter_map(|entry| match &entry.state {
                EffectiveRefState::Candidate { static_mesh, .. } => Some(*static_mesh),
                EffectiveRefState::Excluded(_) => None,
            }),
    );
    super::check_cancellation(cancellation)?;

    for (_identity, entry) in effective_refs {
        super::check_cancellation(cancellation)?;
        let EffectiveRefState::Candidate {
            reference,
            static_mesh,
        } = entry.state
        else {
            continue;
        };
        if let Err(error) = mesh_bounds.bounds(static_mesh) {
            record_missing_mesh(
                &mut build_report,
                &mut missing_meshes,
                &reference.id,
                static_mesh,
                &error,
            );
            continue;
        }
        let collider_parts = match mesh_bounds.collider_parts(static_mesh) {
            Ok(collider_parts) => collider_parts.clone(),
            Err(error) => {
                record_missing_mesh(
                    &mut build_report,
                    &mut missing_meshes,
                    &reference.id,
                    static_mesh,
                    &error,
                );
                continue;
            }
        };
        build_report.resolved_bounds += 1;
        if collider_parts.source() == MeshColliderSource::NoCollision || collider_parts.is_empty() {
            build_report.no_collision += 1;
            continue;
        }
        if collider_parts.fallback().is_some() {
            build_report.collider_part_fallbacks += 1;
        }
        match collider_parts.source() {
            MeshColliderSource::Collision => build_report.collision_source += 1,
            MeshColliderSource::VisibleFallback => build_report.visible_fallback_source += 1,
            MeshColliderSource::NoCollision => unreachable!("no-collision meshes are skipped"),
        }
        pending.push((entry.cell, entry.reference_key, reference, collider_parts));
    }

    // Placing every collision mesh in the world is independent per reference.
    let mut occluders: Vec<StaticOccluder> = pending
        .par_iter()
        .map(|(cell, reference_key, reference, collider_parts)| {
            let collider = RapierCollider::from_mesh_collider_parts(
                collider_parts,
                reference.translation,
                reference.rotation,
                reference.scale,
            );
            StaticOccluder {
                id: reference.id.clone(),
                cell: [cell.0, cell.1],
                reference_key: [reference_key.0, reference_key.1],
                bounds: collider.bounds(),
                collider,
            }
        })
        .collect();
    build_report.huge_footprint = occluders
        .iter()
        .filter(|occluder| huge_footprint(occluder.bounds))
        .count();
    occluders.shrink_to_fit();

    build_report.missing_meshes = missing_meshes.into_values().collect();

    Ok((StaticOccluderIndex::new(occluders), build_report))
}

fn initial_build_report(scan: &EffectiveRefScan<'_>) -> StaticOccluderBuildReport {
    let count_excluded = |reason: ExclusionReason| {
        scan.refs
            .values()
            .filter(|entry| matches!(entry.state, EffectiveRefState::Excluded(r) if r == reason))
            .count()
    };
    StaticOccluderBuildReport {
        active_refs_scanned: scan.refs.len(),
        target_refs_excluded: count_excluded(ExclusionReason::Target),
        regex_excluded: count_excluded(ExclusionReason::Regex),
        unresolved_static: count_excluded(ExclusionReason::UnresolvedStatic),
        invalid_master_index: scan.invalid_master_index,
        huge_footprint_side_threshold: HUGE_OCCLUDER_FOOTPRINT_SIDE,
        ..StaticOccluderBuildReport::default()
    }
}

fn record_missing_mesh(
    build_report: &mut StaticOccluderBuildReport,
    missing_meshes: &mut BTreeMap<String, MissingOccluderMesh>,
    static_id: &str,
    static_mesh: &super::mesh::StaticMesh,
    error: &io::Error,
) {
    build_report.missing_bounds += 1;
    missing_meshes
        .entry(static_mesh.mesh_path.clone())
        .or_insert_with(|| MissingOccluderMesh {
            static_id: static_id.to_owned(),
            mesh_path: static_mesh.mesh_path.clone(),
            error: error.to_string(),
        });
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

/// Load-order-independent identity of a reference: the lowercased name of the
/// plugin that originally created it plus its reference index in that plugin.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RefIdentity {
    owner: String,
    refr_index: u32,
}

struct EffectiveRef<'a> {
    /// Effective cell after applying any `moved_cell`.
    cell: CellCoord,
    /// Plugin-local `(mast_index, refr_index)` of the winning record.
    reference_key: (u32, u32),
    state: EffectiveRefState<'a>,
}

enum EffectiveRefState<'a> {
    Candidate {
        reference: &'a tes3::esp::Reference,
        static_mesh: &'a super::mesh::StaticMesh,
    },
    Excluded(ExclusionReason),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ExclusionReason {
    Target,
    Regex,
    UnresolvedStatic,
}

struct EffectiveRefScan<'a> {
    refs: BTreeMap<RefIdentity, EffectiveRef<'a>>,
    invalid_master_index: usize,
}

fn effective_static_occluder_refs<'a>(
    active_plugins: &[NamedPlugin<'a>],
    active_cells: &BTreeSet<CellCoord>,
    static_index: &'a StaticMeshIndex,
    target_static_ids: &BTreeSet<String>,
    occluder_filter: &IdFilter,
    cancellation: &CancellationToken,
) -> io::Result<EffectiveRefScan<'a>> {
    // Millions of references flow through here on a full load order, so the hot loop avoids
    // allocating: owners are interned to indices and each distinct object ID is classified
    // (regex filters, static lookup) once instead of per reference.
    let mut owners: Vec<String> = Vec::new();
    let mut owner_index: rustc_hash::FxHashMap<String, u32> = rustc_hash::FxHashMap::default();
    let mut intern = |name: &str| -> u32 {
        let lower = name.to_lowercase();
        if let Some(&index) = owner_index.get(&lower) {
            return index;
        }
        let index = u32::try_from(owners.len()).expect("owner count fits u32");
        owners.push(lower.clone());
        owner_index.insert(lower, index);
        index
    };
    let mut id_classes: rustc_hash::FxHashMap<&'a str, IdClass<'a>> =
        rustc_hash::FxHashMap::default();
    let mut refs: rustc_hash::FxHashMap<(u32, u32), EffectiveRef<'a>> =
        rustc_hash::FxHashMap::default();
    let mut invalid_master_index = 0;

    for named in active_plugins {
        super::check_cancellation(cancellation)?;
        let plugin_owner = intern(&named.name);
        let masters: Vec<u32> = named
            .plugin
            .header()
            .map(|header| {
                header
                    .masters
                    .iter()
                    .map(|(master, _)| intern(master))
                    .collect()
            })
            .unwrap_or_default();

        for cell in named.plugin.objects_of_type::<Cell>() {
            super::check_cancellation(cancellation)?;
            if !cell.is_exterior() {
                continue;
            }

            for (&(mast_index, refr_index), reference) in &cell.references {
                let owner = if mast_index == 0 {
                    plugin_owner
                } else {
                    let Some(&master) = usize::try_from(mast_index)
                        .ok()
                        .and_then(|index| masters.get(index - 1))
                    else {
                        invalid_master_index += 1;
                        continue;
                    };
                    master
                };
                let identity = (owner, refr_index);
                if reference.deleted == Some(true) {
                    refs.remove(&identity);
                    continue;
                }

                let class = *id_classes.entry(reference.id.as_str()).or_insert_with(|| {
                    classify_id(
                        &reference.id,
                        static_index,
                        target_static_ids,
                        occluder_filter,
                    )
                });
                let state = match class {
                    IdClass::Static(static_mesh) => EffectiveRefState::Candidate {
                        reference,
                        static_mesh,
                    },
                    IdClass::Excluded(reason) => EffectiveRefState::Excluded(reason),
                };
                refs.insert(
                    identity,
                    EffectiveRef {
                        cell: reference.moved_cell.unwrap_or(cell.data.grid),
                        reference_key: (mast_index, refr_index),
                        state,
                    },
                );
            }
        }
    }

    let refs = refs
        .into_iter()
        .filter(|(_, entry)| active_cells.contains(&entry.cell))
        .map(|((owner, refr_index), entry)| {
            (
                RefIdentity {
                    owner: owners[owner as usize].clone(),
                    refr_index,
                },
                entry,
            )
        })
        .collect();

    Ok(EffectiveRefScan {
        refs,
        invalid_master_index,
    })
}

#[derive(Clone, Copy)]
enum IdClass<'a> {
    Static(&'a super::mesh::StaticMesh),
    Excluded(ExclusionReason),
}

fn classify_id<'a>(
    id: &str,
    static_index: &'a StaticMeshIndex,
    target_static_ids: &BTreeSet<String>,
    occluder_filter: &IdFilter,
) -> IdClass<'a> {
    let key = id.to_lowercase();
    if target_static_ids.contains(&key) {
        IdClass::Excluded(ExclusionReason::Target)
    } else if !occluder_filter.includes(id) {
        IdClass::Excluded(ExclusionReason::Regex)
    } else if let Some(static_mesh) = static_index.get_normalized_key(&key) {
        IdClass::Static(static_mesh)
    } else {
        IdClass::Excluded(ExclusionReason::UnresolvedStatic)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use tes3::esp::{Cell, CellData, Header, Plugin, Reference, Static, TES3Object};
    use tes3::nif::{
        NiAVObject, NiGeometry, NiGeometryData, NiLink, NiObjectNET, NiStream, NiTriBasedGeom,
        NiTriBasedGeomData, NiTriShape, NiTriShapeData, NiType, RootCollisionNode,
    };
    use vfstool_lib::VFS;

    use crate::unclip::{
        args::IdFilter,
        cells::CellCoord,
        mesh::WorldAabb,
        occlusion::{StaticOccluder, StaticOccluderIndex},
        setup::{ContextPlugin, build_static_index},
    };

    type NifVec3 = tes3::nif::glam::Vec3;

    use super::{
        EffectiveRef, EffectiveRefState, ExclusionReason, NamedPlugin, RefIdentity,
        build_static_occluders, effective_static_occluder_refs, should_include_occluder,
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn effective_static_occluder_refs_apply_later_deletions() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_at_z(0.0),
            )]))],
        };
        let deleted = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([((1, 2), deleted_ref())]))],
        );

        let plugins = [first, deleted];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &BTreeSet::new(),
        );

        assert!(refs.is_empty());
    }

    #[test]
    fn effective_static_occluder_refs_resolve_master_relative_identity_for_moved_refs() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((0, 2), reference_at_z(0.0))],
            ))],
        };
        let mut moved = reference_at_z(5.0);
        moved.moved_cell = Some((0, 0));
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((1, 2), moved)],
            ))],
        );

        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &static_index,
            &BTreeSet::new(),
        );

        assert_eq!(refs.len(), 1);
        let entry = refs
            .get(&RefIdentity {
                owner: "plugin0.esp".to_owned(),
                refr_index: 2,
            })
            .expect("moved ref keeps its master identity");
        assert_eq!(entry.cell, (0, 0));
        assert_eq!(entry.reference_key, (1, 2));
    }

    #[test]
    fn later_plugin_moving_ref_out_of_active_set_removes_earlier_entry() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_with_id("rock"),
            )]))],
        };
        let mut moved = reference_with_id("rock");
        moved.moved_cell = Some((9, 9));
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([((1, 2), moved)]))],
        );

        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &BTreeSet::new(),
        );

        assert!(refs.is_empty());
    }

    #[test]
    fn ref_moved_into_active_cell_from_inactive_cell_record_is_kept() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell_at(
                (5, 5),
                [((0, 2), reference_with_id("rock"))],
            ))],
        };
        let mut moved = reference_with_id("rock");
        moved.moved_cell = Some((0, 0));
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell_at(
                (5, 5),
                [((1, 2), moved)],
            ))],
        );

        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &BTreeSet::new(),
        );

        assert_eq!(refs.len(), 1);
        assert_eq!(refs.values().next().unwrap().cell, (0, 0));
    }

    #[test]
    fn out_of_range_master_index_is_skipped_and_counted() {
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((0, 1), reference_with_id("rock")),
                ((3, 2), reference_with_id("rock")),
            ]))],
        };
        let plugins = [plugin];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 1);
        assert_eq!(report.invalid_master_index, 1);
    }

    #[test]
    fn moved_master_ref_yields_single_occluder_at_new_position() {
        let temp_dir = TempDir::new("moved-master-ref");
        write_visible_nif(&temp_dir.path().join("Meshes/Rock.nif"));
        let static_index = rock_static_index();
        let master = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((0, 7), rock_at([8192.0 + 100.0, 100.0, 0.0]))],
            ))],
        };
        let mut moved_rock = rock_at([100.0, 100.0, 50.0]);
        moved_rock.moved_cell = Some((0, 0));
        let mover = plugin_with_masters(
            &["Master.esm"],
            vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((1, 7), moved_rock)],
            ))],
        );
        let named = [
            NamedPlugin {
                name: "master.esm".to_owned(),
                plugin: &master,
            },
            NamedPlugin {
                name: "mover.esp".to_owned(),
                plugin: &mover,
            },
        ];
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (index, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 1);
        let occluders = all_occluders(&index);
        assert_eq!(occluders.len(), 1);
        assert_eq!(occluders[0].cell, [0, 0]);
        assert_eq!(occluders[0].reference_key, [1, 7]);
        assert!((occluders[0].bounds.min[2] - 50.0).abs() < 1e-3);
        assert!(occluders[0].bounds.max[0] < 8192.0);
    }

    #[test]
    fn later_plugin_deleting_master_ref_yields_no_occluder() {
        let temp_dir = TempDir::new("deleted-master-ref");
        write_visible_nif(&temp_dir.path().join("Meshes/Rock.nif"));
        let static_index = rock_static_index();
        let master = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 7),
                rock_at([100.0, 100.0, 0.0]),
            )]))],
        };
        let deleter = plugin_with_masters(
            &["master.esm"],
            vec![TES3Object::Cell(exterior_cell([((1, 7), deleted_ref())]))],
        );
        let named = [
            NamedPlugin {
                name: "master.esm".to_owned(),
                plugin: &master,
            },
            NamedPlugin {
                name: "deleter.esp".to_owned(),
                plugin: &deleter,
            },
        ];
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (index, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 0);
        assert!(all_occluders(&index).is_empty());
    }

    #[test]
    fn own_refs_from_different_plugins_do_not_collide() {
        let temp_dir = TempDir::new("distinct-owners");
        write_visible_nif(&temp_dir.path().join("Meshes/Rock.nif"));
        let static_index = rock_static_index();
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 1),
                rock_at([100.0, 100.0, 0.0]),
            )]))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 1),
                rock_at([500.0, 500.0, 0.0]),
            )]))],
        };
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (index, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 2);
        assert_eq!(all_occluders(&index).len(), 2);
    }

    #[test]
    fn missing_occluder_mesh_is_reported_with_path() {
        let static_index = static_index();
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((0, 1), reference_with_id("rock")),
                ((0, 2), reference_with_id("rock")),
                ((0, 3), reference_with_id("grass")),
            ]))],
        };
        let plugins = [plugin];
        let named = named_plugins(&plugins);
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.missing_bounds, 3);
        let paths: Vec<&str> = report
            .missing_meshes
            .iter()
            .map(|missing| missing.mesh_path.as_str())
            .collect();
        assert_eq!(paths, ["grass.nif", "rock.nif"]);
        let rock = &report.missing_meshes[1];
        assert_eq!(rock.static_id, "rock");
        assert!(rock.error.contains("rock.nif"), "{}", rock.error);
    }

    #[test]
    fn borrowed_active_target_context_slot_preserves_later_override_order() {
        let before = Plugin {
            objects: vec![TES3Object::from(static_record(
                "grass",
                "meshes/before-grass.nif",
            ))],
        };
        let target = Plugin {
            objects: vec![
                TES3Object::from(static_record("rock", "meshes/target-rock.nif")),
                TES3Object::from(static_record("grass", "meshes/target-grass.nif")),
                TES3Object::Cell(exterior_cell([
                    ((0, 2), reference_with_id("rock")),
                    ((0, 4), reference_with_id("grass")),
                ])),
            ],
        };
        let mut moved_grass = reference_at_z(12.0);
        moved_grass.moved_cell = Some((0, 0));
        let later = plugin_with_masters(
            &["target.esp"],
            vec![
                TES3Object::from(static_record("grass", "meshes/later-grass.nif")),
                TES3Object::Cell(exterior_cell([((1, 2), deleted_ref())])),
                TES3Object::Cell(exterior_cell_at((1, 0), [((1, 4), moved_grass)])),
            ],
        );
        let context_plugins = [
            ContextPlugin::Owned(before),
            ContextPlugin::Borrowed(&target),
            ContextPlugin::Owned(later),
        ];
        assert!(matches!(context_plugins[1], ContextPlugin::Borrowed(_)));
        assert!(std::ptr::eq(
            std::ptr::from_ref(context_plugins[1].as_plugin()),
            std::ptr::from_ref(&target),
        ));

        let static_index =
            build_static_index(context_plugins.iter().map(ContextPlugin::as_plugin), None);
        let named: Vec<NamedPlugin<'_>> = ["before.esp", "target.esp", "later.esp"]
            .into_iter()
            .zip(context_plugins.iter().map(ContextPlugin::as_plugin))
            .map(|(name, plugin)| NamedPlugin {
                name: name.to_owned(),
                plugin,
            })
            .collect();
        let scan = effective_static_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &static_index,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();
        let refs = scan.refs;

        assert!(!refs.contains_key(&RefIdentity {
            owner: "target.esp".to_owned(),
            refr_index: 2,
        }));
        assert_eq!(refs.len(), 1);
        let moved_entry = refs
            .get(&RefIdentity {
                owner: "target.esp".to_owned(),
                refr_index: 4,
            })
            .expect("later moved ref should replace target ref under its master identity");
        assert_eq!(moved_entry.cell, (0, 0));
        let EffectiveRefState::Candidate {
            reference,
            static_mesh,
        } = moved_entry.state
        else {
            panic!("later moved target ref should remain an occluder candidate");
        };
        assert!((reference.translation[2] - 12.0).abs() < f32::EPSILON);
        assert_eq!(static_mesh.mesh_path, "meshes/later-grass.nif");
    }

    #[test]
    fn later_live_non_candidate_suppresses_earlier_candidate() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("grass"),
            )]))],
        );
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &BTreeSet::from(["grass".to_owned()]),
        );

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            refs.values().next().map(|entry| &entry.state),
            Some(EffectiveRefState::Excluded(ExclusionReason::Target))
        ));
    }

    #[test]
    fn moved_cell_non_candidate_suppresses_earlier_candidate() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_with_id("rock"),
            )]))],
        };
        let mut moved_non_candidate = reference_with_id("grass");
        moved_non_candidate.moved_cell = Some((0, 0));
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((1, 2), moved_non_candidate)],
            ))],
        );
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let refs = effective_occluder_refs(
            &named,
            &BTreeSet::from([(0, 0), (1, 0)]),
            &static_index,
            &BTreeSet::from(["grass".to_owned()]),
        );

        assert_eq!(refs.len(), 1);
        assert!(matches!(
            refs.values().next().map(|entry| &entry.state),
            Some(EffectiveRefState::Excluded(ExclusionReason::Target))
        ));
    }

    #[test]
    fn active_refs_scanned_stays_final_effective_live_ref_count() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((0, 1), reference_with_id("rock")),
                ((0, 2), reference_with_id("rock")),
            ]))],
        };
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([
                ((1, 2), deleted_ref()),
                ((0, 3), reference_with_id("unknown")),
            ]))],
        );
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);
        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 2);
        assert_eq!(report.missing_bounds, 1);
        assert_eq!(report.missing_meshes.len(), 1);
        assert_eq!(report.unresolved_static, 1);
    }

    #[test]
    fn overwritten_candidate_does_not_load_missing_mesh_bounds() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("grass"),
            )]))],
        );
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::from(["grass".to_owned()]),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 1);
        assert_eq!(report.target_refs_excluded, 1);
        assert_eq!(report.missing_bounds, 0);
        assert!(report.missing_meshes.is_empty());
        assert_eq!(report.resolved_bounds, 0);
    }

    #[test]
    fn overwritten_candidate_by_regex_excluded_ref_does_not_load_missing_mesh_bounds() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("tree_huge"),
            )]))],
        );
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &["^tree_huge$".to_owned()]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.active_refs_scanned, 1);
        assert_eq!(report.regex_excluded, 1);
        assert_eq!(report.missing_bounds, 0);
        assert_eq!(report.resolved_bounds, 0);
    }

    #[test]
    fn static_occluder_report_counts_collider_sources() {
        let temp_dir = TempDir::new("collider-sources");
        write_visible_nif(&temp_dir.path().join("Meshes/Rock.nif"));
        write_collision_nif(&temp_dir.path().join("Meshes/Tree.nif"));
        let rock = Static {
            id: "rock".to_owned(),
            mesh: "Rock.nif".to_owned(),
            ..Static::default()
        };
        let tree = Static {
            id: "tree".to_owned(),
            mesh: "Tree.nif".to_owned(),
            ..Static::default()
        };
        let static_index = crate::unclip::mesh::StaticMeshIndex::from_statics([&rock, &tree]);
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((0, 1), reference_with_id("rock")),
                ((0, 2), reference_with_id("tree")),
            ]))],
        };
        let plugins = [plugin];
        let named = named_plugins(&plugins);
        let vfs = VFS::from_directories(vec![temp_dir.path().to_path_buf()], None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        assert_eq!(report.resolved_bounds, 2);
        assert_eq!(report.collision_source, 1);
        assert_eq!(report.visible_fallback_source, 1);
        assert_eq!(report.no_collision, 0);
    }

    #[test]
    fn overwritten_candidate_by_unresolved_static_does_not_load_missing_mesh_bounds() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (0, 2),
                reference_with_id("rock"),
            )]))],
        };
        let second = plugin_with_masters(
            &["plugin0.esp"],
            vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_with_id("unknown"),
            )]))],
        );
        let plugins = [first, second];
        let named = named_plugins(&plugins);
        let static_index = static_index();
        let vfs = VFS::from_directories(Vec::<PathBuf>::new(), None);
        let mut mesh_bounds = crate::unclip::mesh::MeshCache::new(&vfs);

        let (_, report) = build_static_occluders(
            &named,
            &BTreeSet::from([(0, 0)]),
            &static_index,
            &mut mesh_bounds,
            &BTreeSet::new(),
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

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

    fn named_plugins(plugins: &[Plugin]) -> Vec<NamedPlugin<'_>> {
        plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| NamedPlugin {
                name: format!("plugin{index}.esp"),
                plugin,
            })
            .collect()
    }

    fn plugin_with_masters(masters: &[&str], mut objects: Vec<TES3Object>) -> Plugin {
        let header = Header {
            masters: masters
                .iter()
                .map(|master| ((*master).to_owned(), 0))
                .collect(),
            ..Header::default()
        };
        objects.insert(0, TES3Object::Header(header));
        Plugin { objects }
    }

    fn all_occluders(index: &StaticOccluderIndex) -> Vec<&StaticOccluder> {
        index.candidates_for(WorldAabb {
            min: [-1.0e9; 3],
            max: [1.0e9; 3],
        })
    }

    fn rock_static_index() -> crate::unclip::mesh::StaticMeshIndex {
        let rock = Static {
            id: "rock".to_owned(),
            mesh: "Rock.nif".to_owned(),
            ..Static::default()
        };
        crate::unclip::mesh::StaticMeshIndex::from_statics([&rock])
    }

    fn rock_at(translation: [f32; 3]) -> Reference {
        Reference {
            id: "rock".to_owned(),
            translation,
            ..Reference::default()
        }
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

    fn static_record(id: &str, mesh: &str) -> Static {
        Static {
            id: id.to_owned(),
            mesh: mesh.to_owned(),
            ..Static::default()
        }
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-static-occluders-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn write_visible_nif(path: &Path) {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, visible_vertices());
        stream.roots.push(shape.cast());
        write_stream(path, stream);
    }

    fn write_collision_nif(path: &Path) {
        let mut stream = NiStream::new();
        let shape = insert_shape(&mut stream, collision_vertices());
        let collision = RootCollisionNode {
            base: tes3::nif::NiNode {
                base: NiAVObject::default(),
                children: vec![shape],
                ..tes3::nif::NiNode::default()
            },
        };
        let collision_key = stream.objects.insert(NiType::from(collision));
        // OpenMW only honours a RootCollisionNode that is a child of the root node.
        let root = tes3::nif::NiNode {
            base: NiAVObject::default(),
            children: vec![NiLink::<NiAVObject>::new(collision_key).cast()],
            ..tes3::nif::NiNode::default()
        };
        let root_key = stream.objects.insert(NiType::from(root));
        stream
            .roots
            .push(NiLink::<NiAVObject>::new(root_key).cast());
        write_stream(path, stream);
    }

    fn write_stream(path: &Path, mut stream: NiStream) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, stream.save_bytes().unwrap()).unwrap();
    }

    fn insert_shape(stream: &mut NiStream, vertices: &[[f32; 3]]) -> NiLink<NiAVObject> {
        let geometry_data = NiTriShapeData {
            base: NiTriBasedGeomData {
                base: NiGeometryData {
                    vertices: vertices
                        .iter()
                        .copied()
                        .map(|vertex| NifVec3::new(vertex[0], vertex[1], vertex[2]))
                        .collect(),
                    ..NiGeometryData::default()
                },
            },
            triangles: vec![[0, 1, 2]],
            shared_normals: Vec::new(),
        };
        let data_key = stream.objects.insert(NiType::from(geometry_data));
        let shape = NiTriShape {
            base: NiTriBasedGeom {
                base: NiGeometry {
                    base: NiAVObject {
                        base: NiObjectNET::default(),
                        ..NiAVObject::default()
                    },
                    geometry_data: NiLink::new(data_key),
                    ..NiGeometry::default()
                },
            },
        };
        let shape_key = stream.objects.insert(NiType::from(shape));
        NiLink::new(shape_key)
    }

    fn visible_vertices() -> &'static [[f32; 3]] {
        &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 1.0]]
    }

    fn collision_vertices() -> &'static [[f32; 3]] {
        &[[0.0, 0.0, -1.0], [2.0, 0.0, -1.0], [0.0, 2.0, 2.0]]
    }

    fn effective_occluder_refs<'a>(
        plugins: &[NamedPlugin<'a>],
        active_cells: &BTreeSet<CellCoord>,
        static_index: &'a crate::unclip::mesh::StaticMeshIndex,
        target_static_ids: &BTreeSet<String>,
    ) -> BTreeMap<RefIdentity, EffectiveRef<'a>> {
        effective_static_occluder_refs(
            plugins,
            active_cells,
            static_index,
            target_static_ids,
            &IdFilter::new(&[], &[]).unwrap(),
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap()
        .refs
    }
}
