use std::collections::BTreeMap;

use tes3::esp::{Plugin, TES3Object};

use super::{
    args::{RelocationPolicy, UnclipPolicy},
    cells::CellCoord,
    mesh::{MeshAabb, MeshCache, MeshContact, MeshGeometry, StaticMeshIndex, WorldAabb},
    occlusion::{
        StaticBoundsAction, StaticOccluder, StaticOccluderIndex, decide_static_bounds_action,
        translate_bounds_xy,
    },
    orientation::{OrientationResult, orientation_to_terrain},
    target::TargetRefIndex,
    terrain::TerrainIndex,
    write_plan::{
        WriteAdjustment, WriteOrientation, WritePlan, WriteStaticBoundsAnalysis,
        WriteStaticBoundsDeletion, WriteStaticBoundsMove,
    },
};

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
    target_refs: &TargetRefIndex,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshCache<'_>,
    static_occluders: &StaticOccluderIndex,
    policy: &UnclipPolicy,
) -> WritePlan {
    let mut plan = WritePlan::default();
    let mut context = WritePlanningContext {
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        policy,
    };
    for (cell_grid, key, reference) in target_refs.iter_refs(plugin) {
        let change =
            adjust_reference_for_terrain_and_static_bounds(cell_grid, key, reference, &mut context);
        record_reference_change(change, &mut plan);
    }

    plan
}

struct WritePlanningContext<'a, 'b> {
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshCache<'b>,
    static_occluders: &'a StaticOccluderIndex,
    policy: &'a UnclipPolicy,
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

        for orientation in &changes.orientations {
            if let Some(reference) = cell
                .references
                .get_mut(&(orientation.reference_key[0], orientation.reference_key[1]))
            {
                reference.rotation = orientation.new_rotation;
            }
        }
    }
}

#[derive(Default)]
struct CellWriteChanges<'a> {
    adjustments: Vec<&'a WriteAdjustment>,
    deletions: Vec<&'a WriteStaticBoundsDeletion>,
    moves: Vec<&'a WriteStaticBoundsMove>,
    orientations: Vec<&'a WriteOrientation>,
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
        for orientation in &plan.orientations {
            cells
                .entry(orientation.cell)
                .or_default()
                .orientations
                .push(orientation);
        }
        Self { cells }
    }

    fn get(&self, cell: [i32; 2]) -> Option<&CellWriteChanges<'a>> {
        self.cells.get(&cell)
    }
}

fn record_reference_change(change: WriteReferenceChange, plan: &mut WritePlan) {
    match change {
        WriteReferenceChange::None(analysis) => {
            if let Some(analysis) = analysis {
                plan.static_bounds_analysis.push(analysis);
            }
        }
        WriteReferenceChange::Changes(changes) => {
            let changes = *changes;
            if let Some(analysis) = changes.static_bounds_analysis {
                plan.static_bounds_analysis.push(analysis);
            }
            if let Some(adjustment) = changes.adjustment {
                plan.adjusted_refs += 1;
                plan.adjustments.push(adjustment);
            }
            if let Some(deletion) = changes.deletion {
                plan.deleted_refs += 1;
                plan.deletions.push(deletion);
            }
            if let Some(move_) = changes.move_ {
                plan.moved_refs += 1;
                plan.moves.push(move_);
            }
            if let Some(orientation) = changes.orientation {
                plan.oriented_refs += 1;
                plan.orientations.push(orientation);
            }
        }
    }
}

enum WriteReferenceChange {
    None(Option<WriteStaticBoundsAnalysis>),
    Changes(Box<WriteReferenceChanges>),
}

#[derive(Default)]
struct WriteReferenceChanges {
    adjustment: Option<WriteAdjustment>,
    deletion: Option<WriteStaticBoundsDeletion>,
    move_: Option<WriteStaticBoundsMove>,
    orientation: Option<WriteOrientation>,
    static_bounds_analysis: Option<WriteStaticBoundsAnalysis>,
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
    context: &mut WritePlanningContext<'_, '_>,
) -> WriteReferenceChange {
    if reference.deleted == Some(true) {
        return WriteReferenceChange::None(None);
    }
    let terrain = context.terrain;
    let static_occluders = context.static_occluders;
    let policy = context.policy;
    let target = WriteTarget { cell, key };
    let orientation_context = OrientationPlanningContext {
        terrain,
        static_occluders,
        policy,
    };
    let static_bounds_context = StaticBoundsPlanningContext {
        terrain,
        static_occluders,
        policy,
    };
    let Some(static_mesh) = context.static_index.get(&reference.id) else {
        return WriteReferenceChange::None(None);
    };
    let Ok(geometry) = context.mesh_contacts.geometry(static_mesh) else {
        return WriteReferenceChange::None(None);
    };

    let contact_position =
        geometry
            .contact
            .world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(contact_position[0], contact_position[1]);
    let adjustment_context = AdjustmentOrientationContext {
        target,
        reference,
        contact_position,
        terrain_z,
        policy,
        geometry,
        orientation: orientation_context,
    };
    let mut corrected_translation = reference.translation;
    if let Some(terrain_z) = terrain_z {
        corrected_translation[2] -= contact_position[2] - terrain_z;
    }
    let mut changes = WriteReferenceChanges::default();
    let final_translation = match plan_static_bounds_change(
        &mut changes,
        target,
        reference,
        corrected_translation,
        geometry,
        static_bounds_context,
    ) {
        StaticBoundsPlanResult::Continue(final_translation) => final_translation,
        StaticBoundsPlanResult::FinishTerrainAndOrientation(final_translation) => {
            return finish_with_terrain_and_orientation(
                changes,
                final_translation,
                adjustment_context,
            );
        }
        StaticBoundsPlanResult::Stop => return changes_to_result(changes),
    };

    if changes.move_.is_none() {
        return finish_with_terrain_and_orientation(changes, final_translation, adjustment_context);
    }
    plan_orientation_to_changes(
        &mut changes,
        target,
        reference,
        final_translation,
        geometry,
        orientation_context,
    );
    changes_to_result(changes)
}

enum StaticBoundsPlanResult {
    Continue([f32; 3]),
    FinishTerrainAndOrientation([f32; 3]),
    Stop,
}

#[derive(Clone, Copy)]
struct StaticBoundsPlanningContext<'a> {
    terrain: &'a TerrainIndex,
    static_occluders: &'a StaticOccluderIndex,
    policy: &'a UnclipPolicy,
}

fn plan_static_bounds_change(
    changes: &mut WriteReferenceChanges,
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    corrected_translation: [f32; 3],
    geometry: &MeshGeometry,
    context: StaticBoundsPlanningContext<'_>,
) -> StaticBoundsPlanResult {
    let corrected_bounds =
        geometry
            .bounds
            .world_aabb(corrected_translation, reference.rotation, reference.scale);
    match decide_static_bounds_action(corrected_bounds, context.static_occluders) {
        StaticBoundsAction::None => {
            changes.static_bounds_analysis = Some(static_bounds_analysis(
                target,
                "static_bounds_clear",
                0.0,
                None,
                corrected_bounds,
            ));
        }
        StaticBoundsAction::Delete { ratio, occluder } => {
            changes.static_bounds_analysis = Some(static_bounds_analysis(
                target,
                "static_bounds_fully_occluded",
                ratio,
                Some(occluder),
                corrected_bounds,
            ));
            if context.policy.write_actions.static_delete() {
                changes.deletion = Some(static_bounds_deletion(
                    target.cell,
                    target.key,
                    reference,
                    ratio,
                    occluder,
                ));
                return StaticBoundsPlanResult::Stop;
            }
        }
        StaticBoundsAction::Move { ratio, occluder }
            if context.policy.write_actions.static_move() =>
        {
            let move_ = try_static_bounds_move(
                target,
                reference,
                corrected_translation,
                MoveSearchContext {
                    geometry,
                    terrain: context.terrain,
                    static_occluders: context.static_occluders,
                    relocation: context.policy.relocation,
                },
                StaticBoundsMoveCause { ratio, occluder },
            );
            changes.static_bounds_analysis = Some(static_bounds_analysis(
                target,
                if move_.is_some() {
                    "static_bounds_relocatable"
                } else {
                    "static_bounds_blocked"
                },
                ratio,
                Some(occluder),
                corrected_bounds,
            ));
            let Some(move_) = move_ else {
                return StaticBoundsPlanResult::FinishTerrainAndOrientation(reference.translation);
            };
            let new_position = move_.new_position;
            changes.move_ = Some(move_);
            return StaticBoundsPlanResult::Continue(new_position);
        }
        StaticBoundsAction::Move { .. } => {}
    }
    StaticBoundsPlanResult::Continue(reference.translation)
}

#[derive(Clone, Copy)]
struct AdjustmentOrientationContext<'a> {
    target: WriteTarget,
    reference: &'a tes3::esp::Reference,
    contact_position: [f32; 3],
    terrain_z: Option<f32>,
    policy: &'a UnclipPolicy,
    geometry: &'a MeshGeometry,
    orientation: OrientationPlanningContext<'a>,
}

fn finish_with_terrain_and_orientation(
    mut changes: WriteReferenceChanges,
    mut final_translation: [f32; 3],
    context: AdjustmentOrientationContext<'_>,
) -> WriteReferenceChange {
    apply_terrain_adjustment_to_changes(
        &mut changes,
        &mut final_translation,
        context.target,
        context.reference,
        context.contact_position,
        context.terrain_z,
        context.policy,
    );
    plan_orientation_to_changes(
        &mut changes,
        context.target,
        context.reference,
        final_translation,
        context.geometry,
        context.orientation,
    );
    changes_to_result(changes)
}

fn plan_orientation_to_changes(
    changes: &mut WriteReferenceChanges,
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    final_translation: [f32; 3],
    geometry: &MeshGeometry,
    context: OrientationPlanningContext<'_>,
) {
    changes.orientation =
        plan_reference_orientation(target, reference, final_translation, geometry, context);
}

fn static_bounds_deletion(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    ratio: f32,
    occluder: &StaticOccluder,
) -> WriteStaticBoundsDeletion {
    WriteStaticBoundsDeletion {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        occlusion_ratio: ratio,
        occluder_id: occluder.id.clone(),
        occluder_cell: occluder.cell,
        occluder_reference_key: occluder.reference_key,
    }
}

fn static_bounds_analysis(
    target: WriteTarget,
    status: &'static str,
    ratio: f32,
    occluder: Option<&StaticOccluder>,
    target_bounds: WorldAabb,
) -> WriteStaticBoundsAnalysis {
    WriteStaticBoundsAnalysis {
        cell: [target.cell.0, target.cell.1],
        reference_key: [target.key.0, target.key.1],
        status,
        ratio,
        occluder_id: occluder.map(|occluder| occluder.id.clone()),
        occluder_cell: occluder.map(|occluder| occluder.cell),
        occluder_reference_key: occluder.map(|occluder| occluder.reference_key),
        target_bounds: Some(target_bounds),
        occluder_bounds: occluder.map(|occluder| occluder.bounds),
        intersection_volume: occluder
            .and_then(|occluder| target_bounds.intersection(occluder.bounds))
            .map(WorldAabb::volume),
    }
}

fn try_static_bounds_move(
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    corrected_translation: [f32; 3],
    search: MoveSearchContext<'_>,
    cause: StaticBoundsMoveCause<'_>,
) -> Option<WriteStaticBoundsMove> {
    apply_static_bounds_move(
        target,
        reference,
        RefTransform {
            translation: corrected_translation,
            rotation: reference.rotation,
            scale: reference.scale,
        },
        search,
        cause,
    )
}

fn apply_terrain_adjustment_to_changes(
    changes: &mut WriteReferenceChanges,
    final_translation: &mut [f32; 3],
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    contact_position: [f32; 3],
    terrain_z: Option<f32>,
    policy: &UnclipPolicy,
) {
    if let Some(terrain_z) = terrain_z
        && let Some(adjustment) = plan_contact_adjustment(
            target.cell,
            target.key,
            reference,
            contact_position,
            terrain_z,
            policy,
        )
    {
        final_translation[2] = adjustment.new_z;
        changes.adjustment = Some(adjustment);
    }
}

fn changes_to_result(changes: WriteReferenceChanges) -> WriteReferenceChange {
    if changes.adjustment.is_none()
        && changes.deletion.is_none()
        && changes.move_.is_none()
        && changes.orientation.is_none()
    {
        WriteReferenceChange::None(changes.static_bounds_analysis)
    } else {
        WriteReferenceChange::Changes(Box::new(changes))
    }
}

fn apply_static_bounds_move(
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    transform: RefTransform,
    search: MoveSearchContext<'_>,
    cause: StaticBoundsMoveCause<'_>,
) -> Option<WriteStaticBoundsMove> {
    let old_position = transform.translation;
    let new_position = find_valid_relocation_transform(
        target.cell,
        transform,
        &search.geometry.contact,
        search.geometry.bounds,
        search.terrain,
        search.static_occluders,
        search.relocation,
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

#[derive(Clone, Copy)]
struct MoveSearchContext<'a> {
    geometry: &'a MeshGeometry,
    terrain: &'a TerrainIndex,
    static_occluders: &'a StaticOccluderIndex,
    relocation: RelocationPolicy,
}

pub(crate) fn find_valid_relocation_transform(
    cell: CellCoord,
    transform: RefTransform,
    contact: &MeshContact,
    bounds: MeshAabb,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
    relocation: RelocationPolicy,
) -> Option<[f32; 3]> {
    let RefTransform {
        translation,
        rotation,
        scale,
    } = transform;
    let original = [translation[0], translation[1]];
    let grass_bounds = bounds.world_aabb(translation, rotation, scale);
    let contact_offset = contact.local_contact_offset(rotation, scale);

    for step in 1..=relocation.steps {
        let radius = f32::from(step) * relocation.step;
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
            if static_occluders.intersects_volume(moved_bounds) {
                continue;
            }

            let mut candidate_translation = translation;
            candidate_translation[0] = candidate_xy[0];
            candidate_translation[1] = candidate_xy[1];
            let contact_position = [
                contact_offset[0] + candidate_translation[0],
                contact_offset[1] + candidate_translation[1],
                contact_offset[2] + candidate_translation[2],
            ];
            let terrain_z = terrain.height_at(contact_position[0], contact_position[1])?;
            candidate_translation[2] -= contact_position[2] - terrain_z;
            let final_bounds = bounds.world_aabb(candidate_translation, rotation, scale);
            if !static_occluders.intersects_volume(final_bounds) {
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
    policy: &UnclipPolicy,
) -> Option<WriteAdjustment> {
    if !policy.write_actions.terrain_z() {
        return None;
    }
    let contact_delta = contact_position[2] - terrain_z;
    if contact_delta.abs() <= policy.contact_epsilon {
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

fn plan_reference_orientation(
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    translation: [f32; 3],
    geometry: &MeshGeometry,
    context: OrientationPlanningContext<'_>,
) -> Option<WriteOrientation> {
    if !context.policy.write_actions.orient() {
        return None;
    }
    let contact_position =
        geometry
            .contact
            .world_position(translation, reference.rotation, reference.scale);
    let terrain_sample = context
        .terrain
        .sample_at(contact_position[0], contact_position[1])?;
    let orientation = orientation_to_terrain(
        reference.rotation,
        terrain_sample.normal,
        context.policy.orientation_epsilon_degrees,
    )?;
    if orientation_blocks_static_bounds(
        translation,
        reference.scale,
        geometry.bounds,
        orientation,
        context.static_occluders,
    ) {
        return None;
    }

    Some(WriteOrientation {
        cell: [target.cell.0, target.cell.1],
        reference_key: [target.key.0, target.key.1],
        id: reference.id.clone(),
        old_rotation: reference.rotation,
        new_rotation: orientation.new_rotation,
        terrain_normal: orientation.terrain_normal,
        angle_degrees: orientation.angle_degrees,
        contact_position,
    })
}

#[derive(Clone, Copy)]
struct OrientationPlanningContext<'a> {
    terrain: &'a TerrainIndex,
    static_occluders: &'a StaticOccluderIndex,
    policy: &'a UnclipPolicy,
}

fn orientation_blocks_static_bounds(
    translation: [f32; 3],
    scale: Option<f32>,
    bounds: MeshAabb,
    orientation: OrientationResult,
    static_occluders: &StaticOccluderIndex,
) -> bool {
    static_occluders.intersects_volume(bounds.world_aabb(
        translation,
        orientation.new_rotation,
        scale,
    ))
}

#[cfg(test)]
mod tests {
    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use super::{
        WriteReferenceChange, WriteReferenceChanges, apply_unclip_write_plan,
        plan_contact_adjustment, record_reference_change,
    };
    use crate::unclip::{
        args::{IdFilter, RelocationPolicy, UnclipPolicy, WriteActions},
        write_plan::{WriteOrientation, WritePlan, WriteStaticBoundsDeletion},
    };

    #[test]
    fn contact_adjustment_moves_buried_contact_up() {
        let reference = reference_at_z(10.0);

        let adjustment = plan_contact_adjustment(
            (1, 2),
            (3, 4),
            &reference,
            [0.0, 0.0, 7.0],
            9.0,
            &test_policy(),
        )
        .unwrap();

        assert_close(reference.translation[2], 10.0);
        assert_close(adjustment.old_z, 10.0);
        assert_close(adjustment.new_z, 12.0);
        assert_close(adjustment.applied_delta, 2.0);
    }

    #[test]
    fn contact_adjustment_moves_floating_contact_down() {
        let reference = reference_at_z(10.0);

        let adjustment = plan_contact_adjustment(
            (1, 2),
            (3, 4),
            &reference,
            [0.0, 0.0, 12.0],
            9.0,
            &test_policy(),
        )
        .unwrap();

        assert_close(reference.translation[2], 10.0);
        assert_close(adjustment.new_z, 7.0);
        assert_close(adjustment.applied_delta, -3.0);
    }

    #[test]
    fn contact_adjustment_skips_within_epsilon() {
        let reference = reference_at_z(10.0);

        let adjustment = plan_contact_adjustment(
            (1, 2),
            (3, 4),
            &reference,
            [0.0, 0.0, 9.25],
            9.0,
            &test_policy(),
        );

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
            WriteReferenceChange::Changes(Box::new(WriteReferenceChanges {
                deletion: Some(WriteStaticBoundsDeletion {
                    cell: [1, 2],
                    reference_key: [3, 4],
                    id: "grass".to_owned(),
                    occlusion_ratio: 1.0,
                    occluder_id: "rock".to_owned(),
                    occluder_cell: [1, 2],
                    occluder_reference_key: [5, 6],
                }),
                ..WriteReferenceChanges::default()
            })),
            &mut plan,
        );

        apply_unclip_write_plan(&mut plugin, &plan);
        let TES3Object::Cell(cell) = &plugin.objects[0] else {
            unreachable!("test plugin should contain a CELL")
        };
        assert_eq!(plan.deleted_refs, 1);
        assert!(!cell.references.contains_key(&(3, 4)));
    }

    #[test]
    fn orientation_write_changes_update_rotation() {
        let mut plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (3, 4),
                reference_at_z(10.0),
            )]))],
        };
        let mut plan = WritePlan::default();

        record_reference_change(
            WriteReferenceChange::Changes(Box::new(WriteReferenceChanges {
                orientation: Some(WriteOrientation {
                    cell: [1, 2],
                    reference_key: [3, 4],
                    id: "grass".to_owned(),
                    old_rotation: [0.0, 0.0, 0.0],
                    new_rotation: [0.1, 0.2, 0.0],
                    terrain_normal: [0.0, 0.2, 0.98],
                    angle_degrees: 10.0,
                    contact_position: [0.0, 0.0, 10.0],
                }),
                ..WriteReferenceChanges::default()
            })),
            &mut plan,
        );

        apply_unclip_write_plan(&mut plugin, &plan);
        let TES3Object::Cell(cell) = &plugin.objects[0] else {
            unreachable!("test plugin should contain a CELL")
        };
        let reference = cell.references.get(&(3, 4)).unwrap();
        assert_eq!(plan.oriented_refs, 1);
        assert_close(reference.rotation[0], 0.1);
        assert_close(reference.rotation[1], 0.2);
        assert_close(reference.rotation[2], 0.0);
    }

    fn reference_at_z(z: f32) -> Reference {
        Reference {
            id: "grass".to_owned(),
            translation: [0.0, 0.0, z],
            ..Reference::default()
        }
    }

    fn test_policy() -> UnclipPolicy {
        UnclipPolicy {
            write_actions: WriteActions::all(),
            contact_epsilon: 0.5,
            origin_epsilon: 0.5,
            orientation_epsilon_degrees: 1.0,
            relocation: RelocationPolicy {
                step: 32.0,
                steps: 8,
            },
            target_filter: IdFilter::new(&[], &[]).unwrap(),
            occluder_filter: IdFilter::new(&[], &[]).unwrap(),
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
