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

pub(crate) fn apply_unclip_adjustments(
    plugin: &mut Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
) -> WritePlan {
    let mut plan = WritePlan::default();

    for object in &mut plugin.objects {
        let TES3Object::Cell(cell) = object else {
            continue;
        };
        if !cell.is_exterior() {
            continue;
        }

        for (key, reference) in &mut cell.references {
            let change = adjust_reference_for_terrain_and_static_bounds(
                cell.data.grid,
                *key,
                reference,
                terrain,
                static_index,
                mesh_contacts,
                static_occluders,
            );
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
    }

    plan
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

fn adjust_reference_for_terrain_and_static_bounds(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
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
    let mut corrected_reference = reference.clone();
    corrected_reference.translation = corrected_translation;

    match decide_static_bounds_action(
        geometry
            .bounds
            .world_aabb(corrected_translation, reference.rotation, reference.scale),
        static_occluders,
    ) {
        StaticBoundsAction::None => {}
        StaticBoundsAction::Delete { ratio, occluder } => {
            reference.deleted = Some(true);
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
                cell,
                key,
                &mut corrected_reference,
                geometry,
                terrain,
                static_occluders,
                StaticBoundsMoveCause { ratio, occluder },
            ) else {
                let Some(terrain_z) = terrain_z else {
                    return WriteReferenceChange::None;
                };
                return apply_contact_adjustment(cell, key, reference, contact_position, terrain_z)
                    .map_or(WriteReferenceChange::None, WriteReferenceChange::AdjustZ);
            };
            reference.translation = corrected_reference.translation;
            return WriteReferenceChange::Move(move_);
        }
    }

    let Some(terrain_z) = terrain_z else {
        return WriteReferenceChange::None;
    };
    apply_contact_adjustment(cell, key, reference, contact_position, terrain_z)
        .map_or(WriteReferenceChange::None, WriteReferenceChange::AdjustZ)
}

fn apply_static_bounds_move(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
    geometry: &MeshGeometry,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
    cause: StaticBoundsMoveCause<'_>,
) -> Option<WriteStaticBoundsMove> {
    let old_position = reference.translation;
    let new_position = find_valid_relocation_position(
        cell,
        reference,
        &geometry.contact,
        geometry.bounds,
        terrain,
        static_occluders,
    )?;
    reference.translation = new_position;

    Some(WriteStaticBoundsMove {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        occlusion_ratio: cause.ratio,
        old_position,
        new_position,
        occluder_id: cause.occluder.id.clone(),
        occluder_cell: cause.occluder.cell,
        occluder_reference_key: cause.occluder.reference_key,
    })
}

pub(crate) fn find_valid_relocation_position(
    cell: CellCoord,
    reference: &tes3::esp::Reference,
    contact: &MeshContact,
    bounds: MeshAabb,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
) -> Option<[f32; 3]> {
    let original = [reference.translation[0], reference.translation[1]];
    let grass_bounds =
        bounds.world_aabb(reference.translation, reference.rotation, reference.scale);

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

            let mut candidate_translation = reference.translation;
            candidate_translation[0] = candidate_xy[0];
            candidate_translation[1] = candidate_xy[1];
            let contact_position =
                contact.world_position(candidate_translation, reference.rotation, reference.scale);
            let terrain_z = terrain.height_at(contact_position[0], contact_position[1])?;
            candidate_translation[2] -= contact_position[2] - terrain_z;
            let final_bounds =
                bounds.world_aabb(candidate_translation, reference.rotation, reference.scale);
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

fn apply_contact_adjustment(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
    contact_position: [f32; 3],
    terrain_z: f32,
) -> Option<WriteAdjustment> {
    let contact_delta = contact_position[2] - terrain_z;
    if contact_delta.abs() <= CONTACT_TERRAIN_EPSILON {
        return None;
    }
    let old_z = reference.translation[2];
    reference.translation[2] -= contact_delta;
    Some(WriteAdjustment {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        old_z,
        new_z: reference.translation[2],
        applied_delta: -contact_delta,
        contact_position,
        terrain_z,
    })
}

#[cfg(test)]
mod tests {
    use tes3::esp::Reference;

    use super::apply_contact_adjustment;

    #[test]
    fn contact_adjustment_moves_buried_contact_up() {
        let mut reference = reference_at_z(10.0);

        let adjustment =
            apply_contact_adjustment((1, 2), (3, 4), &mut reference, [0.0, 0.0, 7.0], 9.0).unwrap();

        assert_close(reference.translation[2], 12.0);
        assert_close(adjustment.old_z, 10.0);
        assert_close(adjustment.new_z, 12.0);
        assert_close(adjustment.applied_delta, 2.0);
    }

    #[test]
    fn contact_adjustment_moves_floating_contact_down() {
        let mut reference = reference_at_z(10.0);

        let adjustment =
            apply_contact_adjustment((1, 2), (3, 4), &mut reference, [0.0, 0.0, 12.0], 9.0)
                .unwrap();

        assert_close(reference.translation[2], 7.0);
        assert_close(adjustment.applied_delta, -3.0);
    }

    #[test]
    fn contact_adjustment_skips_within_epsilon() {
        let mut reference = reference_at_z(10.0);

        let adjustment =
            apply_contact_adjustment((1, 2), (3, 4), &mut reference, [0.0, 0.0, 9.25], 9.0);

        assert!(adjustment.is_none());
        assert_close(reference.translation[2], 10.0);
    }

    fn reference_at_z(z: f32) -> Reference {
        Reference {
            id: "grass".to_owned(),
            translation: [0.0, 0.0, z],
            ..Reference::default()
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
