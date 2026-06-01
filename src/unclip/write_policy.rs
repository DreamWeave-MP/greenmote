use std::collections::BTreeMap;

use tes3::esp::{Plugin, TES3Object};

use super::{
    args::{RelocationPolicy, UnclipPolicy},
    cells::CellCoord,
    generated_placement::{GeneratedPlacement, GeneratedPlacementIndex},
    mesh::{MeshAabb, MeshCache, MeshGeometry, StaticMeshIndex, WorldAabb},
    occlusion::{
        StaticBoundsAction, StaticBoundsBlockReason, StaticOccluder, StaticOccluderIndex,
        decide_static_bounds_action,
    },
    orientation::{OrientationResult, orientation_to_terrain},
    physics::RapierCollider,
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

pub(crate) struct UnclipWritePlanningInput<'a, 'b> {
    pub(crate) plugin: &'a Plugin,
    pub(crate) target_refs: &'a TargetRefIndex,
    pub(crate) terrain: &'a TerrainIndex,
    pub(crate) static_index: &'a StaticMeshIndex,
    pub(crate) mesh_contacts: &'a mut MeshCache<'b>,
    pub(crate) static_occluders: &'a StaticOccluderIndex,
    pub(crate) policy: &'a UnclipPolicy,
    pub(crate) generated_placements: &'a GeneratedPlacementIndex,
}

pub(crate) fn plan_unclip_adjustments(input: UnclipWritePlanningInput<'_, '_>) -> WritePlan {
    let UnclipWritePlanningInput {
        plugin,
        target_refs,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        policy,
        generated_placements,
    } = input;
    let mut plan = WritePlan::default();
    let mut context = WritePlanningContext {
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        policy,
        generated_placements,
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
    generated_placements: &'a GeneratedPlacementIndex,
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
    reason: StaticBoundsBlockReason,
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
    let Some(static_mesh) = context.static_index.get(&reference.id) else {
        return WriteReferenceChange::None(None);
    };
    let generated_placement = context.generated_placements.get(static_mesh);
    let orientation_context = OrientationPlanningContext {
        terrain,
        static_occluders,
        policy,
    };
    let static_bounds_context = StaticBoundsPlanningContext {
        terrain,
        static_occluders,
        policy,
        generated_placement,
    };
    let Ok(geometry) = context.mesh_contacts.geometry(static_mesh) else {
        return WriteReferenceChange::None(None);
    };

    let terrain_z = origin_terrain_z(terrain, reference.translation, generated_placement);
    let adjustment_context = AdjustmentOrientationContext {
        target,
        reference,
        terrain_z,
        policy,
        generated_placement,
        geometry,
        orientation: orientation_context,
    };
    let mut corrected_translation = reference.translation;
    if let Some(terrain_z) = terrain_z {
        corrected_translation[2] -=
            terrain_delta(reference.translation, terrain_z, generated_placement);
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
    generated_placement: Option<GeneratedPlacement>,
}

fn plan_static_bounds_change(
    changes: &mut WriteReferenceChanges,
    target: WriteTarget,
    reference: &tes3::esp::Reference,
    corrected_translation: [f32; 3],
    geometry: &MeshGeometry,
    context: StaticBoundsPlanningContext<'_>,
) -> StaticBoundsPlanResult {
    let (corrected_bounds, corrected_collider, clearance_collider) = corrected_static_bounds(
        geometry.bounds,
        corrected_translation,
        reference.rotation,
        reference.scale,
    );
    match decide_static_bounds_action(
        corrected_bounds,
        &corrected_collider,
        &clearance_collider,
        context.static_occluders,
    ) {
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
            let move_ = context.policy.write_actions.static_move().then(|| {
                try_static_bounds_move(
                    target,
                    reference,
                    corrected_translation,
                    relocation_search_context(geometry, context),
                    StaticBoundsMoveCause {
                        ratio,
                        occluder,
                        reason: StaticBoundsBlockReason::Volume,
                    },
                )
            });
            if let Some(Some(move_)) = move_ {
                changes.static_bounds_analysis = Some(static_bounds_analysis(
                    target,
                    "static_bounds_relocatable",
                    ratio,
                    Some(occluder),
                    corrected_bounds,
                ));
                let new_position = move_.new_position;
                changes.move_ = Some(move_);
                return StaticBoundsPlanResult::Continue(new_position);
            }
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
        StaticBoundsAction::Move {
            ratio,
            occluder,
            reason,
        } if context.policy.write_actions.static_move() => {
            let move_ = try_static_bounds_move(
                target,
                reference,
                corrected_translation,
                relocation_search_context(geometry, context),
                StaticBoundsMoveCause {
                    ratio,
                    occluder,
                    reason,
                },
            );
            changes.static_bounds_analysis = Some(static_bounds_analysis(
                target,
                static_bounds_move_status(reason, move_.is_some()),
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

fn corrected_static_bounds(
    bounds: MeshAabb,
    translation: [f32; 3],
    rotation: [f32; 3],
    scale: Option<f32>,
) -> (WorldAabb, RapierCollider, RapierCollider) {
    (
        bounds.world_aabb(translation, rotation, scale),
        RapierCollider::from_mesh_bounds(bounds, translation, rotation, scale),
        RapierCollider::placement_clearance_from_mesh_bounds(bounds, translation, rotation, scale),
    )
}

const fn static_bounds_move_status(
    reason: StaticBoundsBlockReason,
    relocated: bool,
) -> &'static str {
    match (reason, relocated) {
        (StaticBoundsBlockReason::Clearance, true) => "static_clearance_relocatable",
        (StaticBoundsBlockReason::Clearance, false) => "static_clearance_blocked",
        (StaticBoundsBlockReason::Volume, true) => "static_bounds_relocatable",
        (StaticBoundsBlockReason::Volume, false) => "static_bounds_blocked",
    }
}

fn relocation_search_context<'a>(
    geometry: &'a MeshGeometry,
    context: StaticBoundsPlanningContext<'a>,
) -> RelocationSearchContext<'a> {
    RelocationSearchContext {
        bounds: geometry.bounds,
        terrain: context.terrain,
        static_occluders: context.static_occluders,
        relocation: context.policy.relocation,
        generated_placement: context.generated_placement,
    }
}

fn origin_terrain_z(
    terrain: &TerrainIndex,
    translation: [f32; 3],
    generated_placement: Option<GeneratedPlacement>,
) -> Option<f32> {
    generated_placement?;
    terrain.height_at(translation[0], translation[1])
}

fn terrain_delta(
    translation: [f32; 3],
    terrain_z: f32,
    generated_placement: Option<GeneratedPlacement>,
) -> f32 {
    generated_placement.map_or(0.0, |placement| {
        translation[2] - terrain_z - placement.z_offset
    })
}

#[derive(Clone, Copy)]
struct AdjustmentOrientationContext<'a> {
    target: WriteTarget,
    reference: &'a tes3::esp::Reference,
    terrain_z: Option<f32>,
    policy: &'a UnclipPolicy,
    generated_placement: Option<GeneratedPlacement>,
    geometry: &'a MeshGeometry,
    orientation: OrientationPlanningContext<'a>,
}

fn finish_with_terrain_and_orientation(
    mut changes: WriteReferenceChanges,
    mut final_translation: [f32; 3],
    context: AdjustmentOrientationContext<'_>,
) -> WriteReferenceChange {
    apply_terrain_adjustment_to_changes(&mut changes, &mut final_translation, context);
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
    search: RelocationSearchContext<'_>,
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
    context: AdjustmentOrientationContext<'_>,
) {
    if let Some(terrain_z) = context.terrain_z
        && let Some(adjustment) = plan_contact_adjustment(ContactAdjustmentInput {
            target: context.target,
            reference: context.reference,
            terrain_z,
            policy: context.policy,
            generated_placement: context.generated_placement,
        })
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
    search: RelocationSearchContext<'_>,
    cause: StaticBoundsMoveCause<'_>,
) -> Option<WriteStaticBoundsMove> {
    let old_position = transform.translation;
    let new_position = find_valid_relocation_transform(target.cell, transform, search)?;

    Some(WriteStaticBoundsMove {
        cell: [target.cell.0, target.cell.1],
        reference_key: [target.key.0, target.key.1],
        id: reference.id.clone(),
        occlusion_ratio: cause.ratio,
        block_reason: static_bounds_block_reason(cause.reason),
        old_position,
        new_position,
        occluder_id: cause.occluder.id.clone(),
        occluder_cell: cause.occluder.cell,
        occluder_reference_key: cause.occluder.reference_key,
    })
}

const fn static_bounds_block_reason(reason: StaticBoundsBlockReason) -> &'static str {
    match reason {
        StaticBoundsBlockReason::Volume => "volume",
        StaticBoundsBlockReason::Clearance => "clearance",
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RelocationSearchContext<'a> {
    pub(crate) bounds: MeshAabb,
    pub(crate) terrain: &'a TerrainIndex,
    pub(crate) static_occluders: &'a StaticOccluderIndex,
    pub(crate) relocation: RelocationPolicy,
    pub(crate) generated_placement: Option<GeneratedPlacement>,
}

pub(crate) fn find_valid_relocation_transform(
    cell: CellCoord,
    transform: RefTransform,
    search: RelocationSearchContext<'_>,
) -> Option<[f32; 3]> {
    let RefTransform {
        translation,
        rotation,
        scale,
    } = transform;
    let original = [translation[0], translation[1]];
    let grass_collider =
        RapierCollider::from_mesh_bounds(search.bounds, translation, rotation, scale);
    let clearance_collider = RapierCollider::placement_clearance_from_mesh_bounds(
        search.bounds,
        translation,
        rotation,
        scale,
    );

    for step in 1..=search.relocation.steps {
        let radius = f32::from(step) * search.relocation.step;
        for direction in RELOCATION_DIRECTIONS {
            let candidate_xy = [
                original[0] + direction[0] * radius,
                original[1] + direction[1] * radius,
            ];
            if !cell_contains_xy(cell, candidate_xy) {
                continue;
            }

            let moved_collider = grass_collider
                .translated_xy(candidate_xy[0] - original[0], candidate_xy[1] - original[1]);
            let moved_clearance = clearance_collider
                .translated_xy(candidate_xy[0] - original[0], candidate_xy[1] - original[1]);
            if search.static_occluders.intersects_shape(&moved_collider)
                || search.static_occluders.intersects_shape(&moved_clearance)
            {
                continue;
            }

            let mut candidate_translation = translation;
            candidate_translation[0] = candidate_xy[0];
            candidate_translation[1] = candidate_xy[1];
            let terrain_z = origin_terrain_z(
                search.terrain,
                candidate_translation,
                search.generated_placement,
            )?;
            candidate_translation[2] -=
                terrain_delta(candidate_translation, terrain_z, search.generated_placement);
            let final_collider = RapierCollider::from_mesh_bounds(
                search.bounds,
                candidate_translation,
                rotation,
                scale,
            );
            let final_clearance = RapierCollider::placement_clearance_from_mesh_bounds(
                search.bounds,
                candidate_translation,
                rotation,
                scale,
            );
            if !search.static_occluders.intersects_shape(&final_collider)
                && !search.static_occluders.intersects_shape(&final_clearance)
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

#[derive(Clone, Copy)]
struct ContactAdjustmentInput<'a> {
    target: WriteTarget,
    reference: &'a tes3::esp::Reference,
    terrain_z: f32,
    policy: &'a UnclipPolicy,
    generated_placement: Option<GeneratedPlacement>,
}

fn plan_contact_adjustment(input: ContactAdjustmentInput<'_>) -> Option<WriteAdjustment> {
    if !input.policy.write_actions.terrain_z() {
        return None;
    }
    let placement = input.generated_placement?;
    let sample_kind = "origin";
    let sample_position = input.reference.translation;
    let delta = input.reference.translation[2] - input.terrain_z - placement.z_offset;
    let epsilon = placement.tolerance;
    if delta.abs() <= epsilon {
        return None;
    }
    let old_z = input.reference.translation[2];
    let new_z = old_z - delta;
    Some(WriteAdjustment {
        cell: [input.target.cell.0, input.target.cell.1],
        reference_key: [input.target.key.0, input.target.key.1],
        id: input.reference.id.clone(),
        old_z,
        new_z,
        applied_delta: -delta,
        sample_kind,
        contact_position: sample_position,
        terrain_z: input.terrain_z,
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
    let (sample_kind, sample_position) = orientation_sample_position(translation);
    let terrain_sample = context
        .terrain
        .sample_at(sample_position[0], sample_position[1])?;
    let orientation = orientation_to_terrain(
        reference.rotation,
        terrain_sample,
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
        sample_kind,
        sample_position,
    })
}

fn orientation_sample_position(translation: [f32; 3]) -> (&'static str, [f32; 3]) {
    ("origin", translation)
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
    let collider =
        RapierCollider::from_mesh_bounds(bounds, translation, orientation.new_rotation, scale);
    static_occluders.intersects_shape(&collider)
}

#[cfg(test)]
mod tests {
    use tes3::esp::{Cell, CellData, Landscape, LandscapeFlags, Plugin, Reference, TES3Object};

    use super::{
        StaticBoundsPlanResult, StaticBoundsPlanningContext, WriteReferenceChange,
        WriteReferenceChanges, apply_unclip_write_plan, orientation_sample_position,
        plan_contact_adjustment, plan_reference_orientation, plan_static_bounds_change,
        record_reference_change,
    };
    use crate::unclip::{
        args::{IdFilter, RelocationPolicy, UnclipPolicy, WriteActions},
        generated_placement::GeneratedPlacement,
        mesh::{MeshAabb, MeshContact, MeshGeometry, WorldAabb},
        occlusion::{StaticOccluder, StaticOccluderIndex},
        physics::RapierCollider,
        terrain::TerrainIndex,
        write_plan::{WriteOrientation, WritePlan, WriteStaticBoundsDeletion},
    };

    #[test]
    fn generated_origin_adjustment_preserves_configured_offset() {
        let reference = reference_at_z(14.0);

        let policy = test_policy();
        let adjustment = plan_contact_adjustment(test_contact_adjustment_input(
            &reference,
            9.0,
            &policy,
            Some(GeneratedPlacement {
                z_offset: 5.0,
                tolerance: 4.0,
            }),
        ));

        assert!(adjustment.is_none());
    }

    #[test]
    fn generated_origin_adjustment_corrects_origin_residual() {
        let reference = reference_at_z(20.0);

        let policy = test_policy();
        let adjustment = plan_contact_adjustment(test_contact_adjustment_input(
            &reference,
            9.0,
            &policy,
            Some(GeneratedPlacement {
                z_offset: 5.0,
                tolerance: 4.0,
            }),
        ))
        .unwrap();

        assert_eq!(adjustment.sample_kind, "origin");
        assert_close(adjustment.new_z, 14.0);
        assert_close(adjustment.applied_delta, -6.0);
    }

    #[test]
    fn generated_orientation_samples_origin_not_decorative_contact() {
        let reference = reference_at_z(0.0);

        let (sample_kind, sample_position) = orientation_sample_position(reference.translation);

        assert_eq!(sample_kind, "origin");
        assert_close(sample_position[0], reference.translation[0]);
        assert_close(sample_position[1], reference.translation[1]);
        assert_close(sample_position[2], reference.translation[2]);
    }

    #[test]
    fn generated_orientation_uses_origin_normal() {
        let reference = reference_at_z(0.0);
        let geometry = MeshGeometry {
            contact: MeshContact::new(vec![[224.0, 32.0, 0.0]]),
            ..test_geometry()
        };
        let terrain = terrain_flat_at_origin_sloped_at_contact();
        let static_occluders = StaticOccluderIndex::new(Vec::new());
        let policy = test_policy();

        let generated = plan_reference_orientation(
            super::WriteTarget {
                cell: (0, 0),
                key: (3, 4),
            },
            &reference,
            reference.translation,
            &geometry,
            super::OrientationPlanningContext {
                terrain: &terrain,
                static_occluders: &static_occluders,
                policy: &policy,
            },
        );
        assert!(generated.is_none());
    }

    #[test]
    fn origin_orientation_without_generated_placement_samples_origin() {
        let reference = reference_at_z(0.0);

        let (sample_kind, sample_position) = orientation_sample_position(reference.translation);

        assert_eq!(sample_kind, "origin");
        assert_close(sample_position[0], reference.translation[0]);
        assert_close(sample_position[1], reference.translation[1]);
        assert_close(sample_position[2], reference.translation[2]);
    }

    #[test]
    fn origin_adjustment_without_generated_placement_skips_contact_fallback() {
        let reference = reference_at_z(10.0);
        let policy = test_policy();

        let adjustment = plan_contact_adjustment(test_contact_adjustment_input(
            &reference, 9.0, &policy, None,
        ));

        assert!(adjustment.is_none());
    }

    #[test]
    fn orientation_is_sampled_per_reference_not_per_mesh() {
        let mut first = reference_at_z(0.0);
        first.translation = [128.0, 128.0, 0.0];
        let mut second = reference_at_z(0.0);
        second.translation = [256.0, 128.0, 0.0];
        let geometry = test_geometry();
        let terrain = varied_angle_terrain();
        let static_occluders = StaticOccluderIndex::new(Vec::new());
        let mut policy = test_policy();
        policy.orientation_epsilon_degrees = 0.0;

        let first_orientation = plan_reference_orientation(
            super::WriteTarget {
                cell: (0, 0),
                key: (3, 4),
            },
            &first,
            first.translation,
            &geometry,
            super::OrientationPlanningContext {
                terrain: &terrain,
                static_occluders: &static_occluders,
                policy: &policy,
            },
        )
        .unwrap();
        let second_orientation = plan_reference_orientation(
            super::WriteTarget {
                cell: (0, 0),
                key: (5, 6),
            },
            &second,
            second.translation,
            &geometry,
            super::OrientationPlanningContext {
                terrain: &terrain,
                static_occluders: &static_occluders,
                policy: &policy,
            },
        )
        .unwrap();

        assert_eq!(first_orientation.id, second_orientation.id);
        assert!(
            (first_orientation.new_rotation[0] - second_orientation.new_rotation[0]).abs()
                > f32::EPSILON
        );
        assert!(
            (first_orientation.new_rotation[1] - second_orientation.new_rotation[1]).abs()
                > f32::EPSILON
        );
    }

    #[test]
    fn fully_occluded_static_bounds_try_move_before_delete() {
        let reference = reference_at_z(0.0);
        let geometry = test_geometry();
        let terrain = flat_terrain();
        let occluder_bounds = WorldAabb {
            min: [-1.0, -1.0, -1.0],
            max: [2.0, 2.0, 2.0],
        };
        let static_occluders = StaticOccluderIndex::new(vec![StaticOccluder {
            id: "house".to_owned(),
            cell: [0, 0],
            reference_key: [1, 1],
            bounds: occluder_bounds,
            collider: RapierCollider::from_world_aabb(occluder_bounds),
        }]);
        let policy = test_policy();
        let mut changes = WriteReferenceChanges::default();

        let result = plan_static_bounds_change(
            &mut changes,
            super::WriteTarget {
                cell: (0, 0),
                key: (3, 4),
            },
            &reference,
            reference.translation,
            &geometry,
            StaticBoundsPlanningContext {
                terrain: &terrain,
                static_occluders: &static_occluders,
                policy: &policy,
                generated_placement: Some(GeneratedPlacement {
                    z_offset: 0.0,
                    tolerance: 4.0,
                }),
            },
        );

        let StaticBoundsPlanResult::Continue(new_position) = result else {
            panic!("relocatable fully occluded ref should continue with moved position");
        };
        assert!(changes.move_.is_some());
        assert!(changes.deletion.is_none());
        assert_close(new_position[0], 32.0);
    }

    #[test]
    fn static_bounds_move_skips_when_origin_offset_unknown() {
        let reference = reference_at_z(0.0);
        let geometry = test_geometry();
        let terrain = flat_terrain();
        let occluder_bounds = WorldAabb {
            min: [-1.0, -1.0, -1.0],
            max: [2.0, 2.0, 2.0],
        };
        let static_occluders = StaticOccluderIndex::new(vec![StaticOccluder {
            id: "house".to_owned(),
            cell: [0, 0],
            reference_key: [1, 1],
            bounds: occluder_bounds,
            collider: RapierCollider::from_world_aabb(occluder_bounds),
        }]);
        let policy = test_policy();
        let mut changes = WriteReferenceChanges::default();

        let result = plan_static_bounds_change(
            &mut changes,
            super::WriteTarget {
                cell: (0, 0),
                key: (3, 4),
            },
            &reference,
            reference.translation,
            &geometry,
            StaticBoundsPlanningContext {
                terrain: &terrain,
                static_occluders: &static_occluders,
                policy: &policy,
                generated_placement: None,
            },
        );

        assert!(matches!(result, StaticBoundsPlanResult::Stop));
        assert!(changes.move_.is_none());
        assert!(changes.deletion.is_some());
    }

    #[test]
    fn relocation_rejects_candidate_when_clearance_probe_still_intersects_static() {
        let terrain = flat_terrain();
        let bounds = MeshAabb {
            min: [-1.0, -1.0, 0.0],
            max: [1.0, 1.0, 10.0],
        };
        let static_occluders = StaticOccluderIndex::new(vec![StaticOccluder {
            id: "board".to_owned(),
            cell: [0, 0],
            reference_key: [1, 1],
            bounds: WorldAabb {
                min: [131.0, 103.0, 4.0],
                max: [133.0, 105.0, 5.0],
            },
            collider: RapierCollider::from_world_aabb(WorldAabb {
                min: [131.0, 103.0, 4.0],
                max: [133.0, 105.0, 5.0],
            }),
        }]);

        let moved = super::find_valid_relocation_transform(
            (0, 0),
            super::RefTransform {
                translation: [100.0, 100.0, 0.0],
                rotation: [0.0; 3],
                scale: None,
            },
            super::RelocationSearchContext {
                bounds,
                terrain: &terrain,
                static_occluders: &static_occluders,
                relocation: RelocationPolicy {
                    step: 32.0,
                    steps: 1,
                },
                generated_placement: Some(GeneratedPlacement {
                    z_offset: 0.0,
                    tolerance: 4.0,
                }),
            },
        )
        .unwrap();

        assert_close(moved[0], 68.0);
        assert_close(moved[1], 100.0);
    }

    #[test]
    fn deleted_write_changes_physically_remove_local_refs() {
        let mut plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((3, 4), reference_at_z(10.0)),
                ((7, 8), reference_with_id("non_target")),
            ]))],
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
        assert!(cell.references.contains_key(&(7, 8)));
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
                    sample_kind: "contact",
                    sample_position: [0.0, 0.0, 10.0],
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

    #[test]
    fn orientation_write_changes_preserve_existing_zrot_exactly() {
        let zrot = -1.234_567_9;
        let mut reference = reference_at_z(10.0);
        reference.rotation = [0.7, -0.8, zrot];
        let mut plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([((3, 4), reference)]))],
        };
        let mut plan = WritePlan::default();

        record_reference_change(
            WriteReferenceChange::Changes(Box::new(WriteReferenceChanges {
                orientation: Some(WriteOrientation {
                    cell: [1, 2],
                    reference_key: [3, 4],
                    id: "grass".to_owned(),
                    old_rotation: [0.7, -0.8, zrot],
                    new_rotation: [0.1, 0.2, zrot],
                    terrain_normal: [0.0, 0.2, 0.98],
                    angle_degrees: 10.0,
                    sample_kind: "contact",
                    sample_position: [0.0, 0.0, 10.0],
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
        assert_close(reference.rotation[0], 0.1);
        assert_close(reference.rotation[1], 0.2);
        assert_eq!(reference.rotation[2].to_bits(), zrot.to_bits());
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

    fn test_contact_adjustment_input<'a>(
        reference: &'a Reference,
        terrain_z: f32,
        policy: &'a UnclipPolicy,
        generated_placement: Option<GeneratedPlacement>,
    ) -> super::ContactAdjustmentInput<'a> {
        super::ContactAdjustmentInput {
            target: super::WriteTarget {
                cell: (1, 2),
                key: (3, 4),
            },
            reference,
            terrain_z,
            policy,
            generated_placement,
        }
    }

    fn test_geometry() -> MeshGeometry {
        MeshGeometry {
            contact: MeshContact::new(vec![[0.0; 3]]),
            bounds: MeshAabb {
                min: [0.0; 3],
                max: [1.0; 3],
            },
            occluder_bounds: MeshAabb {
                min: [0.0; 3],
                max: [1.0; 3],
            },
        }
    }

    fn flat_terrain() -> TerrainIndex {
        let landscape = Landscape {
            landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
            ..Landscape::default()
        };
        TerrainIndex::from_landscapes([&landscape])
    }

    fn terrain_flat_at_origin_sloped_at_contact() -> TerrainIndex {
        let mut landscape = Landscape {
            landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
            ..Landscape::default()
        };
        landscape.vertex_heights.data[0][2] = 16;
        landscape.vertex_heights.data[0][3] = -16;
        landscape.vertex_heights.data[1][2] = 16;
        landscape.vertex_heights.data[1][3] = -16;
        TerrainIndex::from_landscapes([&landscape])
    }

    fn varied_angle_terrain() -> TerrainIndex {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        heights[1][1] = 64.0;
        heights[0][1] = 0.0;
        heights[1][0] = 32.0;
        heights[0][0] = 0.0;
        heights[1][2] = 128.0;
        heights[0][2] = 0.0;
        TerrainIndex::from_decoded_heights((0, 0), heights)
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
