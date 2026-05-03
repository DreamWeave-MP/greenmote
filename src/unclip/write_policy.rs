use std::collections::BTreeMap;

use tes3::esp::{Plugin, TES3Object};

use super::{
    cells::CellCoord,
    mesh::{MeshAabb, MeshContact, MeshContactCache, MeshGeometry, StaticMeshIndex},
    model::CONTACT_TERRAIN_EPSILON,
    occlusion::{
        StaticBoundsAction, StaticOccluder, StaticOccluderIndex, decide_static_bounds_action,
        static_bounds_occlusion_ratio, translate_bounds_xy,
    },
    terrain::TerrainIndex,
    write_plan::{WriteAdjustment, WritePlan, WriteStaticBoundsDeletion, WriteStaticBoundsMove},
};

const RELOCATION_STEP: f32 = 32.0;
const RELOCATION_STEPS: u16 = 8;
const CELL_SIZE: f32 = 8192.0;
const RELOCATION_DIRECTIONS: &[[f32; 2]] = &[
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
    [0.707_106_77, 0.707_106_77],
    [0.707_106_77, -0.707_106_77],
    [-0.707_106_77, 0.707_106_77],
    [-0.707_106_77, -0.707_106_77],
];

pub(crate) fn plan_unclip_adjustments(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
) -> WritePlan {
    let mut plan = WritePlan::default();
    let mut exterior_cells = plugin
        .objects
        .iter()
        .enumerate()
        .filter_map(|(index, object)| {
            let TES3Object::Cell(cell) = object else {
                return None;
            };
            cell.is_exterior().then_some((cell.data.grid, index))
        })
        .collect::<Vec<_>>();
    exterior_cells.sort_unstable();

    for (cell_grid, object_index) in exterior_cells {
        let TES3Object::Cell(cell) = &plugin.objects[object_index] else {
            unreachable!("sorted exterior cell index should still point to a CELL")
        };
        let mut reference_keys = cell.references.keys().copied().collect::<Vec<_>>();
        reference_keys.sort_unstable();

        for key in reference_keys {
            let Some(reference) = cell.references.get(&key) else {
                continue;
            };
            let change = adjust_reference_for_terrain_and_static_bounds(
                cell_grid,
                key,
                reference,
                terrain,
                static_index,
                mesh_contacts,
                static_occluders,
            );
            record_reference_change(change, &mut plan);
        }
    }

    plan
}

pub(crate) fn apply_unclip_write_plan(plugin: &mut Plugin, plan: &WritePlan) {
    let changes_by_cell = WriteChangesByCell::from_plan(plan);
    for object in &mut plugin.objects {
        let TES3Object::Cell(cell) = object else {
            continue;
        };
        if !cell.is_exterior() {
            continue;
        }
        let cell_grid = [cell.data.grid.0, cell.data.grid.1];
        let Some(changes) = changes_by_cell.get(cell_grid) else {
            continue;
        };

        for deletion in &changes.deletions {
            cell.references
                .remove(&(deletion.reference_key[0], deletion.reference_key[1]));
        }

        for adjustment in &changes.adjustments {
            if let Some(reference) = cell
                .references
                .get_mut(&(adjustment.reference_key[0], adjustment.reference_key[1]))
            {
                reference.translation[2] = adjustment.new_z;
            }
        }

        for move_ in &changes.moves {
            if let Some(reference) = cell
                .references
                .get_mut(&(move_.reference_key[0], move_.reference_key[1]))
            {
                reference.translation = move_.new_position;
            }
        }
    }
}

#[derive(Default)]
struct CellWriteChanges<'a> {
    adjustments: Vec<&'a WriteAdjustment>,
    deletions: Vec<&'a WriteStaticBoundsDeletion>,
    moves: Vec<&'a WriteStaticBoundsMove>,
}

struct WriteChangesByCell<'a> {
    cells: BTreeMap<[i32; 2], CellWriteChanges<'a>>,
}

impl<'a> WriteChangesByCell<'a> {
    fn from_plan(plan: &'a WritePlan) -> Self {
        let mut cells = BTreeMap::<[i32; 2], CellWriteChanges<'a>>::new();
        for adjustment in &plan.adjustments {
            cells
                .entry(adjustment.cell)
                .or_default()
                .adjustments
                .push(adjustment);
        }
        for deletion in &plan.deletions {
            cells
                .entry(deletion.cell)
                .or_default()
                .deletions
                .push(deletion);
        }
        for move_ in &plan.moves {
            cells.entry(move_.cell).or_default().moves.push(move_);
        }
        Self { cells }
    }

    fn get(&self, cell: [i32; 2]) -> Option<&CellWriteChanges<'a>> {
        self.cells.get(&cell)
    }
}

fn record_reference_change(change: WriteReferenceChange, plan: &mut WritePlan) {
    match change {
        WriteReferenceChange::None => {}
        WriteReferenceChange::AdjustZ(adjustment) => {
            plan.adjusted_refs += 1;
            plan.adjustments.push(adjustment);
        }
        WriteReferenceChange::Delete(deletion) => {
            plan.deleted_refs += 1;
            plan.deletions.push(deletion);
        }
        WriteReferenceChange::Move(move_) => {
            plan.moved_refs += 1;
            plan.moves.push(move_);
        }
    }
}

enum WriteReferenceChange {
    None,
    AdjustZ(WriteAdjustment),
    Delete(WriteStaticBoundsDeletion),
    Move(WriteStaticBoundsMove),
}

#[derive(Clone, Copy)]
struct StaticBoundsMoveCause<'a> {
    ratio: f32,
    occluder: &'a StaticOccluder,
}

#[derive(Clone, Copy)]
pub(crate) struct RefTransform {
    pub(crate) translation: [f32; 3],
    pub(crate) rotation: [f32; 3],
    pub(crate) scale: Option<f32>,
}

#[derive(Clone, Copy)]
struct WriteTarget {
    cell: CellCoord,
    key: (u32, u32),
}

fn adjust_reference_for_terrain_and_static_bounds(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
) -> WriteReferenceChange {
    if reference.deleted == Some(true) {
        return WriteReferenceChange::None;
    }
    let Some(static_mesh) = static_index.get(&reference.id) else {
        return WriteReferenceChange::None;
    };
    let Ok(geometry) = mesh_contacts.geometry(&static_mesh.mesh_path) else {
        return WriteReferenceChange::None;
    };

    let contact_position =
        geometry
            .contact
            .world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(contact_position[0], contact_position[1]);
    let mut corrected_translation = reference.translation;
    if let Some(terrain_z) = terrain_z {
        corrected_translation[2] -= contact_position[2] - terrain_z;
    }
    match decide_static_bounds_action(
        geometry
            .bounds
            .world_aabb(corrected_translation, reference.rotation, reference.scale),
        static_occluders,
    ) {
        StaticBoundsAction::None => {}
        StaticBoundsAction::Delete { ratio, occluder } => {
            return WriteReferenceChange::Delete(WriteStaticBoundsDeletion {
                cell: [cell.0, cell.1],
                reference_key: [key.0, key.1],
                id: reference.id.clone(),
                occlusion_ratio: ratio,
                occluder_id: occluder.id.clone(),
                occluder_cell: occluder.cell,
                occluder_reference_key: occluder.reference_key,
            });
        }
        StaticBoundsAction::Move {
            ratio, occluder, ..
        } => {
            let Some(move_) = apply_static_bounds_move(
                WriteTarget { cell, key },
                reference,
                RefTransform {
                    translation: corrected_translation,
                    rotation: reference.rotation,
                    scale: reference.scale,
                },
                geometry,
                terrain,
                static_occluders,
                StaticBoundsMoveCause { ratio, occluder },
            ) else {
                let Some(terrain_z) = terrain_z else {
                    return WriteReferenceChange::None;
                };
                return plan_contact_adjustment(cell, key, reference, contact_position, terrain_z)
                    .map_or(WriteReferenceChange::None, WriteReferenceChange::AdjustZ);
            };
            return WriteReferenceChange::Move(move_);
        }
    }

    let Some(terrain_z) = terrain_z else {
        return WriteReferenceChange::None;
    };
    plan_contact_adjustment(cell, key, reference, contact_position, terrain_z)
        .map_or(WriteReferenceChange::None, WriteReferenceChange::AdjustZ)
}

fn apply_static_bounds_move(
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    transform: RefTransform,
    geometry: &MeshGeometry,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
    cause: StaticBoundsMoveCause<'_>,
) -> Option<WriteStaticBoundsMove> {
    let old_position = transform.translation;
    let new_position = find_valid_relocation_transform(
        target.cell,
        transform,
        &geometry.contact,
        geometry.bounds,
        terrain,
        static_occluders,
    )?;

    Some(WriteStaticBoundsMove {
        cell: [target.cell.0, target.cell.1],
        reference_key: [target.key.0, target.key.1],
        id: reference.id.clone(),
        occlusion_ratio: cause.ratio,
        old_position,
        new_position,
        occluder_id: cause.occluder.id.clone(),
        occluder_cell: cause.occluder.cell,
        occluder_reference_key: cause.occluder.reference_key,
    })
}

pub(crate) fn find_valid_relocation_transform(
    cell: CellCoord,
    transform: RefTransform,
    contact: &MeshContact,
    bounds: MeshAabb,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
) -> Option<[f32; 3]> {
    let RefTransform {
        translation,
        rotation,
        scale,
    } = transform;
    let original = [translation[0], translation[1]];
    let grass_bounds = bounds.world_aabb(translation, rotation, scale);

    for step in 1..=RELOCATION_STEPS {
        let radius = f32::from(step) * RELOCATION_STEP;
        for direction in RELOCATION_DIRECTIONS {
            let candidate_xy = [
                original[0] + direction[0] * radius,
                original[1] + direction[1] * radius,
            ];
            if !cell_contains_xy(cell, candidate_xy) {
                continue;
            }

            let moved_bounds = translate_bounds_xy(
                grass_bounds,
                candidate_xy[0] - original[0],
                candidate_xy[1] - original[1],
            );
            if static_bounds_occlusion_ratio(
                moved_bounds,
                &static_occluders.bounds_for(moved_bounds),
            ) > f32::EPSILON
            {
                continue;
            }

            let mut candidate_translation = translation;
            candidate_translation[0] = candidate_xy[0];
            candidate_translation[1] = candidate_xy[1];
            let contact_position = contact.world_position(candidate_translation, rotation, scale);
            let terrain_z = terrain.height_at(contact_position[0], contact_position[1])?;
            candidate_translation[2] -= contact_position[2] - terrain_z;
            let final_bounds = bounds.world_aabb(candidate_translation, rotation, scale);
            if static_bounds_occlusion_ratio(
                final_bounds,
                &static_occluders.bounds_for(final_bounds),
            ) <= f32::EPSILON
            {
                return Some(candidate_translation);
            }
        }
    }

    None
}

#[allow(clippy::cast_precision_loss)]
fn cell_contains_xy(cell: CellCoord, position: [f32; 2]) -> bool {
    let min_x = cell.0 as f32 * CELL_SIZE;
    let min_y = cell.1 as f32 * CELL_SIZE;
    position[0] >= min_x
        && position[0] < min_x + CELL_SIZE
        && position[1] >= min_y
        && position[1] < min_y + CELL_SIZE
}

fn plan_contact_adjustment(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    contact_position: [f32; 3],
    terrain_z: f32,
) -> Option<WriteAdjustment> {
    let contact_delta = contact_position[2] - terrain_z;
    if contact_delta.abs() <= CONTACT_TERRAIN_EPSILON {
        return None;
    }
    let old_z = reference.translation[2];
    let new_z = old_z - contact_delta;
    Some(WriteAdjustment {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        old_z,
        new_z,
        applied_delta: -contact_delta,
        contact_position,
        terrain_z,
    })
}

#[cfg(test)]
mod tests {
    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use super::{
        WriteReferenceChange, apply_unclip_write_plan, plan_contact_adjustment,
        record_reference_change,
    };
    use crate::unclip::write_plan::{WritePlan, WriteStaticBoundsDeletion};

    #[test]
    fn contact_adjustment_moves_buried_contact_up() {
        let reference = reference_at_z(10.0);

        let adjustment =
            plan_contact_adjustment((1, 2), (3, 4), &reference, [0.0, 0.0, 7.0], 9.0).unwrap();

        assert_close(reference.translation[2], 10.0);
        assert_close(adjustment.old_z, 10.0);
        assert_close(adjustment.new_z, 12.0);
        assert_close(adjustment.applied_delta, 2.0);
    }

    #[test]
    fn contact_adjustment_moves_floating_contact_down() {
        let reference = reference_at_z(10.0);

        let adjustment =
            plan_contact_adjustment((1, 2), (3, 4), &reference, [0.0, 0.0, 12.0], 9.0).unwrap();

        assert_close(reference.translation[2], 10.0);
        assert_close(adjustment.new_z, 7.0);
        assert_close(adjustment.applied_delta, -3.0);
    }

    #[test]
    fn contact_adjustment_skips_within_epsilon() {
        let reference = reference_at_z(10.0);

        let adjustment = plan_contact_adjustment((1, 2), (3, 4), &reference, [0.0, 0.0, 9.25], 9.0);

        assert!(adjustment.is_none());
        assert_close(reference.translation[2], 10.0);
    }

    #[test]
    fn deleted_write_changes_physically_remove_local_refs() {
        let mut plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (3, 4),
                reference_at_z(10.0),
            )]))],
        };
        let mut plan = WritePlan::default();

        record_reference_change(
            WriteReferenceChange::Delete(WriteStaticBoundsDeletion {
                cell: [1, 2],
                reference_key: [3, 4],
                id: "grass".to_owned(),
                occlusion_ratio: 1.0,
                occluder_id: "rock".to_owned(),
                occluder_cell: [1, 2],
                occluder_reference_key: [5, 6],
            }),
            &mut plan,
        );

        apply_unclip_write_plan(&mut plugin, &plan);
        let TES3Object::Cell(cell) = &plugin.objects[0] else {
            unreachable!("test plugin should contain a CELL")
        };
        assert_eq!(plan.deleted_refs, 1);
        assert!(!cell.references.contains_key(&(3, 4)));
    }

    fn reference_at_z(z: f32) -> Reference {
        Reference {
            id: "grass".to_owned(),
            translation: [0.0, 0.0, z],
            ..Reference::default()
        }
    }

    fn exterior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        let mut cell = Cell {
            data: CellData {
                grid: (1, 2),
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
