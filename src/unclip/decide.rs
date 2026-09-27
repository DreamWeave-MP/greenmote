// SPDX-License-Identifier: GPL-3.0-only

//! The per-reference rule pipeline.
//!
//! Each target reference gets exactly one [`Verdict`]. Rules run in a fixed order and the first
//! terminal outcome wins:
//!
//! 1. Refs that are deleted, unresolved, or unmeasurable are skipped with a reason.
//! 2. `road-delete` deletes refs standing on a road texture.
//! 3. `water-delete` deletes refs standing on ground below the exterior water plane.
//! 4. `orient` tilts the ref to the terrain, then `terrain-z` re-grounds the mesh base.
//! 5. `static-move` moves refs that clip into a static occluder to the nearest clear spot in the
//!    same cell; `static-delete` removes refs that are inside an occluder or cannot be moved.
//!
//! A disabled action never silently changes a verdict: the reason the ref was left alone is
//! recorded so the report can say why.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use super::{
    cells::CellCoord,
    measure::{EXTERIOR_WATER_LEVEL, GroundContact, Occlusion, RefTransform, Surveyor},
    mesh::{MeshGeometry, WorldAabb},
    orientation::{terrain_rotation, tilt_delta_degrees},
    terrain::TerrainSample,
};

const CELL_SIZE: f32 = 8192.0;

/// Fixed relocation directions, tried in this order for each ring.
const RELOCATION_DIRECTIONS: [[f32; 2]; 8] = [
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
    [
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ],
    [
        -std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ],
    [
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ],
    [
        -std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ],
];

/// A target reference plus the mesh it renders with.
pub(crate) struct RefInput<'a> {
    pub(crate) cell: CellCoord,
    pub(crate) key: (u32, u32),
    pub(crate) id: &'a str,
    pub(crate) deleted: bool,
    pub(crate) transform: RefTransform,
    pub(crate) geometry: Option<&'a Result<Arc<MeshGeometry>, String>>,
}

/// One decided reference.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct RefVerdict {
    pub(crate) cell: [i32; 2],
    pub(crate) key: [u32; 2],
    pub(crate) id: String,
    pub(crate) verdict: Verdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) measured: Option<Measured>,
}

/// What was measured before deciding, for the verbose report.
#[derive(Clone, Copy, Debug, Serialize)]
pub(crate) struct Measured {
    pub(crate) terrain_z: f32,
    /// Local vertical extent of the mesh, the reference for `max_sink_fraction`.
    pub(crate) mesh_height: f32,
    pub(crate) contact: GroundContact,
    pub(crate) tilt_delta_degrees: f32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub(crate) enum Verdict {
    Keep { reason: KeepReason },
    Fix(Fix),
    Delete { reason: DeleteReason },
    Skip { reason: SkipReason },
}

/// Why a reference that could have changed was left alone.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum KeepReason {
    /// Nothing was wrong with it.
    Fine,
    RoadDeleteDisabled {
        texture: String,
    },
    WaterDeleteDisabled {
        terrain_z: f32,
    },
    OrientDisabled {
        tilt_delta_degrees: f32,
    },
    TerrainZDisabled {
        gap: f32,
    },
    StaticActionsDisabled {
        occluder: String,
    },
    NoRelocationFound {
        occluder: String,
    },
}

/// A new placement for a reference.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Fix {
    pub(crate) translation: [f32; 3],
    pub(crate) rotation: [f32; 3],
    pub(crate) grounded: bool,
    pub(crate) oriented: bool,
    pub(crate) moved: bool,
    pub(crate) gap_before: f32,
    pub(crate) gap_after: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) moved_from: Option<[f32; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) occluder: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DeleteReason {
    Road {
        texture: String,
    },
    Water {
        terrain_z: f32,
    },
    InsideStatic {
        occluder: String,
    },
    NoRelocation {
        occluder: String,
    },
    /// The ref's XY lies outside its own cell record. `OpenMW` renders it only from far away and
    /// drops it up close, so it pops in and out.
    OutsideCell,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SkipReason {
    DeletedRef,
    UnresolvedStatic,
    MeshError { error: String },
    NoTerrain,
}

impl Verdict {
    pub(crate) const fn label(&self) -> &'static str {
        match self {
            Self::Keep { reason } => reason.label(),
            Self::Fix(fix) => fix.label(),
            Self::Delete { reason } => reason.label(),
            Self::Skip { reason } => reason.label(),
        }
    }

    pub(crate) const fn changes_plugin(&self) -> bool {
        matches!(self, Self::Fix(_) | Self::Delete { .. })
    }
}

impl KeepReason {
    pub(crate) const fn label(&self) -> &'static str {
        match self {
            Self::Fine => "keep",
            Self::RoadDeleteDisabled { .. } => "keep_road_delete_disabled",
            Self::WaterDeleteDisabled { .. } => "keep_water_delete_disabled",
            Self::OrientDisabled { .. } => "keep_orient_disabled",
            Self::TerrainZDisabled { .. } => "keep_terrain_z_disabled",
            Self::StaticActionsDisabled { .. } => "keep_static_actions_disabled",
            Self::NoRelocationFound { .. } => "keep_no_relocation_found",
        }
    }
}

impl Fix {
    pub(crate) const fn label(&self) -> &'static str {
        match (self.moved, self.grounded, self.oriented) {
            (true, _, _) => "fix_move",
            (false, true, true) => "fix_ground_and_orient",
            (false, true, false) => "fix_ground",
            (false, false, true) => "fix_orient",
            (false, false, false) => "fix",
        }
    }
}

impl DeleteReason {
    pub(crate) const fn label(&self) -> &'static str {
        match self {
            Self::Road { .. } => "delete_road",
            Self::Water { .. } => "delete_water",
            Self::InsideStatic { .. } => "delete_inside_static",
            Self::NoRelocation { .. } => "delete_no_relocation",
            Self::OutsideCell => "delete_outside_cell",
        }
    }
}

impl SkipReason {
    pub(crate) const fn label(&self) -> &'static str {
        match self {
            Self::DeletedRef => "skip_deleted_ref",
            Self::UnresolvedStatic => "skip_unresolved_static",
            Self::MeshError { .. } => "skip_mesh_error",
            Self::NoTerrain => "skip_no_terrain",
        }
    }
}

/// Placement computed for one XY position: the tilt and height the ref should have there.
struct Placement {
    transform: RefTransform,
    terrain: TerrainSample,
    contact_before: GroundContact,
    contact_after: GroundContact,
    tilt_delta_degrees: f32,
    oriented: bool,
    grounded: bool,
    notes: Vec<KeepReason>,
}

/// Decides one reference. Pure: the same inputs always give the same verdict.
#[must_use]
pub(crate) fn decide(input: &RefInput<'_>, surveyor: &Surveyor<'_>) -> RefVerdict {
    let (verdict, measured) = decide_verdict(input, surveyor);
    RefVerdict {
        cell: [input.cell.0, input.cell.1],
        key: [input.key.0, input.key.1],
        id: input.id.to_owned(),
        verdict,
        measured,
    }
}

fn decide_verdict(input: &RefInput<'_>, surveyor: &Surveyor<'_>) -> (Verdict, Option<Measured>) {
    let skip = |reason| (Verdict::Skip { reason }, None);
    if input.deleted {
        return skip(SkipReason::DeletedRef);
    }
    let geometry = match input.geometry {
        None => return skip(SkipReason::UnresolvedStatic),
        Some(Err(error)) => {
            return skip(SkipReason::MeshError {
                error: error.clone(),
            });
        }
        Some(Ok(geometry)) => geometry,
    };
    let [x, y, _] = input.transform.translation;
    let policy = surveyor.policy;
    if !cell_contains_xy(input.cell, x, y) {
        // Not a policy choice: OpenMW draws such refs only from afar and drops them up close.
        return (
            Verdict::Delete {
                reason: DeleteReason::OutsideCell,
            },
            None,
        );
    }
    let mut notes = Vec::new();

    let road = if surveyor.is_road_at(x, y) {
        surveyor.road_at(x, y)
    } else {
        None
    };
    if let Some(texture) = road {
        if policy.actions.road_delete() {
            return (
                Verdict::Delete {
                    reason: DeleteReason::Road { texture },
                },
                None,
            );
        }
        notes.push(KeepReason::RoadDeleteDisabled { texture });
    }

    let Some(contact_before) = surveyor.ground_contact(geometry, &input.transform) else {
        return skip(SkipReason::NoTerrain);
    };
    let Some(placement) = place(
        geometry,
        &input.transform,
        contact_before,
        [x, y],
        surveyor,
        true,
    ) else {
        return skip(SkipReason::NoTerrain);
    };
    let measured = Some(Measured {
        terrain_z: placement.terrain.height,
        mesh_height: geometry.contact.height(),
        contact: placement.contact_before,
        tilt_delta_degrees: placement.tilt_delta_degrees,
    });

    if Surveyor::submerged_ground(&placement.terrain) {
        if policy.actions.water_delete() {
            return (
                Verdict::Delete {
                    reason: DeleteReason::Water {
                        terrain_z: placement.terrain.height,
                    },
                },
                measured,
            );
        }
        notes.push(KeepReason::WaterDeleteDisabled {
            terrain_z: placement.terrain.height,
        });
    }
    notes.extend(placement.notes.iter().cloned());

    let (final_placement, moved, occluder) =
        match resolve_statics(geometry, input, &placement, surveyor, &mut notes) {
            StaticOutcome::Delete(reason) => return (Verdict::Delete { reason }, measured),
            StaticOutcome::Moved(relocated, occluder) => (relocated, true, Some(occluder)),
            StaticOutcome::Stay => (placement, false, None),
        };
    let changed = moved || final_placement.grounded || final_placement.oriented;
    if !changed {
        let reason = notes.into_iter().next().unwrap_or(KeepReason::Fine);
        return (Verdict::Keep { reason }, measured);
    }

    (
        Verdict::Fix(Fix {
            translation: final_placement.transform.translation,
            rotation: final_placement.transform.rotation,
            grounded: final_placement.grounded,
            oriented: final_placement.oriented,
            moved,
            gap_before: final_placement.contact_before.gap,
            gap_after: final_placement.contact_after.gap,
            moved_from: moved.then_some([x, y]),
            occluder,
        }),
        measured,
    )
}

enum StaticOutcome {
    Stay,
    Moved(Placement, String),
    Delete(DeleteReason),
}

/// Applies the static occluder rules to a placed reference.
fn resolve_statics(
    geometry: &MeshGeometry,
    input: &RefInput<'_>,
    placement: &Placement,
    surveyor: &Surveyor<'_>,
    notes: &mut Vec<KeepReason>,
) -> StaticOutcome {
    let actions = surveyor.policy.actions;
    let collider = Surveyor::collider(geometry, &placement.transform);
    let occluder = match surveyor.occlusion(&collider) {
        Occlusion::Clear => return StaticOutcome::Stay,
        Occlusion::Inside(occluder) if actions.static_delete() => {
            return StaticOutcome::Delete(DeleteReason::InsideStatic {
                occluder: occluder.describe(),
            });
        }
        Occlusion::Inside(occluder) | Occlusion::Intersects(occluder) => occluder,
    };
    let away = {
        let bounds = occluder.bounds;
        let centre = [
            f32::midpoint(bounds.min[0], bounds.max[0]),
            f32::midpoint(bounds.min[1], bounds.max[1]),
        ];
        [
            placement.transform.translation[0] - centre[0],
            placement.transform.translation[1] - centre[1],
        ]
    };
    let occluder = occluder.describe();
    if !actions.static_move() {
        if actions.static_delete() {
            return StaticOutcome::Delete(DeleteReason::NoRelocation { occluder });
        }
        notes.push(KeepReason::StaticActionsDisabled { occluder });
        return StaticOutcome::Stay;
    }
    let relocated = relocate(
        geometry,
        &input.transform,
        placement.contact_before,
        &placement.transform,
        input.cell,
        away,
        surveyor,
    );
    match relocated {
        Some(relocated) => StaticOutcome::Moved(relocated, occluder),
        None if actions.static_delete() => {
            StaticOutcome::Delete(DeleteReason::NoRelocation { occluder })
        }
        None => {
            notes.push(KeepReason::NoRelocationFound { occluder });
            StaticOutcome::Stay
        }
    }
}

/// Computes the tilt and height a reference should have at an XY position.
///
/// `respect_actions` is false for relocation candidates, which are always re-grounded so a moved
/// ref never floats at its new spot.
fn place(
    geometry: &MeshGeometry,
    current: &RefTransform,
    contact_before: GroundContact,
    xy: [f32; 2],
    surveyor: &Surveyor<'_>,
    respect_actions: bool,
) -> Option<Placement> {
    let policy = surveyor.policy;
    let terrain = surveyor.terrain_at(xy[0], xy[1])?;
    let mut transform = RefTransform {
        translation: [xy[0], xy[1], current.translation[2]],
        rotation: current.rotation,
        scale: current.scale,
    };
    let mut notes = Vec::new();

    let target_rotation = terrain_rotation(current.rotation, &terrain);
    let tilt_delta_degrees = tilt_delta_degrees(current.rotation, target_rotation);
    let wants_orient = tilt_delta_degrees > policy.orientation_epsilon_degrees;
    let oriented = if wants_orient && (policy.actions.orient() || !respect_actions) {
        transform.rotation = target_rotation;
        true
    } else {
        if wants_orient {
            notes.push(KeepReason::OrientDisabled { tilt_delta_degrees });
        }
        false
    };

    let contact_here = surveyor.ground_contact(geometry, &transform)?;
    let max_sink = policy
        .max_sink
        .max(policy.max_sink_fraction * geometry.contact.height());
    let wants_ground = contact_here.gap > policy.float_tolerance || contact_here.gap < -max_sink;
    let grounded = if wants_ground && (policy.actions.terrain_z() || !respect_actions) {
        transform.translation[2] -= contact_here.gap + policy.sink;
        true
    } else {
        if wants_ground {
            notes.push(KeepReason::TerrainZDisabled {
                gap: contact_here.gap,
            });
        }
        false
    };
    let contact_after = if grounded {
        surveyor.ground_contact(geometry, &transform)?
    } else {
        contact_here
    };

    Some(Placement {
        transform,
        terrain,
        contact_before,
        contact_after,
        tilt_delta_degrees,
        oriented,
        grounded,
        notes,
    })
}

/// Searches outward in rings for the nearest spot in the same cell that is clear of occluders,
/// roads, and water.
fn relocate(
    geometry: &MeshGeometry,
    current: &RefTransform,
    contact_before: GroundContact,
    grounded: &RefTransform,
    cell: CellCoord,
    away: [f32; 2],
    surveyor: &Surveyor<'_>,
) -> Option<Placement> {
    let relocation = surveyor.policy.relocation;
    let [x, y, _] = current.translation;
    // Probe the directions pointing away from the blocking occluder first: most refs are clear
    // one step out, and the spot found is the closest one that is clear, whichever way it lies.
    let mut directions = RELOCATION_DIRECTIONS;
    directions.sort_by(|left, right| {
        let dot = |direction: &[f32; 2]| direction[0] * away[0] + direction[1] * away[1];
        dot(right).total_cmp(&dot(left))
    });
    // Every probe lies within `reach` of the grounded placement, and a probe's box, whatever
    // its tilt, fits inside a sphere of the mesh diagonal around its origin. A window grown by
    // both therefore holds every occluder any probe could touch, so it is fetched once. The
    // extra vertical margin allows for steep ground under distant probes.
    let reach = relocation.step * f32::from(relocation.steps);
    let scale = current.scale.unwrap_or(1.0).abs();
    let diagonal = {
        let size = [
            geometry.bounds.max[0] - geometry.bounds.min[0],
            geometry.bounds.max[1] - geometry.bounds.min[1],
            geometry.bounds.max[2] - geometry.bounds.min[2],
        ];
        (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() * scale
    };
    let margin = reach + diagonal;
    let [gx, gy, gz] = grounded.translation;
    let nearby = surveyor.occluders_within(WorldAabb {
        min: [gx - margin, gy - margin, gz - margin - 4.0 * reach],
        max: [gx + margin, gy + margin, gz + margin + 4.0 * reach],
    });
    for ring in 1..=u32::from(relocation.steps) {
        #[allow(clippy::cast_precision_loss)]
        let radius = relocation.step * ring as f32;
        for direction in directions {
            let candidate = [x + direction[0] * radius, y + direction[1] * radius];
            if !cell_contains_xy(cell, candidate[0], candidate[1]) {
                continue;
            }
            // Cheap rejection first: the spot must have ground above water that is not a road.
            let Some(height) = surveyor.height_at(candidate[0], candidate[1]) else {
                continue;
            };
            if height < EXTERIOR_WATER_LEVEL || surveyor.is_road_at(candidate[0], candidate[1]) {
                continue;
            }
            let placed = place(
                geometry,
                current,
                contact_before,
                candidate,
                surveyor,
                false,
            );
            let Some(placement) = placed else {
                continue;
            };
            if Surveyor::submerged_ground(&placement.terrain) {
                continue;
            }
            let collider = Surveyor::collider(geometry, &placement.transform);
            let clear = !Surveyor::blocked_by_any(&nearby, &collider);
            if clear {
                return Some(placement);
            }
        }
    }
    None
}

#[allow(clippy::cast_precision_loss)]
fn cell_contains_xy(cell: CellCoord, x: f32, y: f32) -> bool {
    let min_x = cell.0 as f32 * CELL_SIZE;
    let min_y = cell.1 as f32 * CELL_SIZE;
    x >= min_x && x < min_x + CELL_SIZE && y >= min_y && y < min_y + CELL_SIZE
}

/// Counts of verdict labels, for summaries.
#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct VerdictCounts {
    pub(crate) total: usize,
    pub(crate) keep: usize,
    pub(crate) fix: usize,
    pub(crate) delete: usize,
    pub(crate) skip: usize,
    pub(crate) by_label: std::collections::BTreeMap<&'static str, usize>,
}

impl VerdictCounts {
    pub(crate) fn from_verdicts(verdicts: &[RefVerdict]) -> Self {
        let mut counts = Self {
            total: verdicts.len(),
            ..Self::default()
        };
        for entry in verdicts {
            match &entry.verdict {
                Verdict::Keep { .. } => counts.keep += 1,
                Verdict::Fix(_) => counts.fix += 1,
                Verdict::Delete { .. } => counts.delete += 1,
                Verdict::Skip { .. } => counts.skip += 1,
            }
            *counts.by_label.entry(entry.verdict.label()).or_default() += 1;
        }
        counts
    }
}

/// Mesh geometry shared across refs, keyed by lowercase static id.
pub(crate) type GeometryTable = HashMap<String, Result<Arc<MeshGeometry>, String>>;

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::large_stack_arrays)]
mod tests {
    use std::sync::Arc;

    use crate::unclip::{
        args::{ActionArg, UnclipPolicy},
        measure::{RefTransform, Surveyor},
        mesh::{MeshAabb, MeshContact, MeshGeometry, WorldAabb},
        occlusion::{StaticOccluder, StaticOccluderIndex},
        physics::RapierCollider,
        terrain::{TerrainIndex, TerrainTextureIndex},
    };

    use super::{DeleteReason, KeepReason, RefInput, SkipReason, Verdict, decide};

    #[allow(clippy::unnecessary_wraps)]
    fn geometry() -> Result<Arc<MeshGeometry>, String> {
        let vertices = vec![
            [-10.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [0.0, -10.0, 0.0],
            [0.0, 10.0, 0.0],
            [0.0, 0.0, 40.0],
        ];
        Ok(Arc::new(MeshGeometry::new_for_test(
            MeshContact::new(vertices),
            MeshAabb {
                min: [-10.0, -10.0, 0.0],
                max: [10.0, 10.0, 40.0],
            },
        )))
    }

    fn sloped_terrain() -> TerrainIndex {
        // Height rises 8 units per vertex column along X (slope 1/16) and is flat along Y.
        let mut heights = Box::new([[0.0_f32; 65]; 65]);
        for row in heights.iter_mut() {
            for (column, height) in row.iter_mut().enumerate() {
                #[allow(clippy::cast_precision_loss)]
                {
                    *height = 100.0 + column as f32 * 8.0;
                }
            }
        }
        TerrainIndex::from_decoded_heights((0, 0), heights)
    }

    fn flat_terrain(height: f32) -> TerrainIndex {
        TerrainIndex::from_decoded_heights((0, 0), Box::new([[height; 65]; 65]))
    }

    struct World {
        terrain: TerrainIndex,
        textures: TerrainTextureIndex,
        occluders: StaticOccluderIndex,
        policy: UnclipPolicy,
    }

    impl World {
        fn new(terrain: TerrainIndex) -> Self {
            Self {
                terrain,
                textures: TerrainTextureIndex::from_parts(std::iter::empty(), std::iter::empty()),
                occluders: StaticOccluderIndex::default(),
                policy: UnclipPolicy::for_test(),
            }
        }

        fn surveyor(&self) -> Surveyor<'_> {
            Surveyor {
                terrain: &self.terrain,
                textures: &self.textures,
                occluders: &self.occluders,
                policy: &self.policy,
            }
        }
    }

    fn input(
        geometry: &Result<Arc<MeshGeometry>, String>,
        translation: [f32; 3],
        rotation: [f32; 3],
    ) -> RefInput<'_> {
        RefInput {
            cell: (0, 0),
            key: (0, 7),
            id: "flora_grass_01",
            deleted: false,
            transform: RefTransform {
                translation,
                rotation,
                scale: None,
            },
            geometry: Some(geometry),
        }
    }

    #[test]
    fn grounded_ref_on_flat_terrain_is_kept() {
        let world = World::new(flat_terrain(100.0));
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 99.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Keep {
                reason: KeepReason::Fine
            }
        ));
    }

    #[test]
    fn floating_ref_is_lowered_to_sink_depth() {
        let world = World::new(flat_terrain(100.0));
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 130.0], [0.0; 3]),
            &world.surveyor(),
        );
        let Verdict::Fix(fix) = result.verdict else {
            panic!("expected fix, got {:?}", result.verdict);
        };
        assert!(fix.grounded && !fix.oriented && !fix.moved);
        assert!((fix.translation[2] - (100.0 - world.policy.sink)).abs() < 1e-3);
        assert!((fix.gap_after + world.policy.sink).abs() < 1e-3);
    }

    #[test]
    fn deeply_buried_ref_is_raised_but_slight_burial_is_kept() {
        let world = World::new(flat_terrain(100.0));
        let geometry = geometry();
        let buried = decide(
            &input(&geometry, [1000.0, 1000.0, 60.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(buried.verdict, Verdict::Fix(ref fix) if fix.grounded));

        // The test mesh is 40 units tall, so 75% of its height (30) is the effective limit.
        let slight = decide(
            &input(&geometry, [1000.0, 1000.0, 100.0 - 29.5], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(
            matches!(slight.verdict, Verdict::Keep { .. }),
            "{:?}",
            slight.verdict
        );

        let mut strict = World::new(flat_terrain(100.0));
        strict.policy.max_sink_fraction = 0.0;
        let slight = decide(
            &input(&geometry, [1000.0, 1000.0, 100.0 - 29.5], [0.0; 3]),
            &strict.surveyor(),
        );
        assert!(matches!(slight.verdict, Verdict::Fix(ref fix) if fix.grounded));
    }

    #[test]
    fn disabled_terrain_z_reports_why_the_ref_was_kept() {
        let mut world = World::new(flat_terrain(100.0));
        world.policy.actions.disable(ActionArg::TerrainZ);
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 130.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Keep {
                reason: KeepReason::TerrainZDisabled { gap }
            } if (gap - 30.0).abs() < 1e-3
        ));
    }

    #[test]
    fn sloped_terrain_orients_then_grounds_with_the_new_tilt() {
        let world = World::new(sloped_terrain());
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 200.0], [0.0; 3]),
            &world.surveyor(),
        );
        let Verdict::Fix(fix) = result.verdict else {
            panic!("expected fix, got {:?}", result.verdict);
        };
        assert!(fix.oriented && fix.grounded);
        assert!((fix.rotation[1] - (8.0_f32 / 128.0).atan()).abs() < 1e-4);
        assert_eq!(fix.rotation[0], 0.0);
        assert!((fix.gap_after + world.policy.sink).abs() < 0.5);
    }

    #[test]
    fn submerged_ground_deletes_or_reports() {
        let mut world = World::new(flat_terrain(-50.0));
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, -50.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Delete {
                reason: DeleteReason::Water { .. }
            }
        ));

        world.policy.actions.disable(ActionArg::WaterDelete);
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, -52.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Keep {
                reason: KeepReason::WaterDeleteDisabled { .. }
            }
        ));
    }

    #[test]
    fn ref_outside_its_cell_is_always_deleted() {
        let mut world = World::new(flat_terrain(0.0));
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [9000.0, 1000.0, 0.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Delete {
                reason: DeleteReason::OutsideCell
            }
        ));

        world.policy.actions = crate::unclip::args::Actions::empty();
        let result = decide(
            &input(&geometry, [9000.0, 1000.0, 0.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Delete {
                reason: DeleteReason::OutsideCell
            }
        ));
    }

    #[test]
    fn missing_geometry_and_deleted_refs_are_skipped() {
        let world = World::new(flat_terrain(0.0));
        let error: Result<Arc<MeshGeometry>, String> = Err("boom".to_owned());
        let result = decide(
            &input(&error, [10.0, 10.0, 0.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Skip {
                reason: SkipReason::MeshError { .. }
            }
        ));

        let geometry = geometry();
        let mut deleted = input(&geometry, [10.0, 10.0, 0.0], [0.0; 3]);
        deleted.deleted = true;
        assert!(matches!(
            decide(&deleted, &world.surveyor()).verdict,
            Verdict::Skip {
                reason: SkipReason::DeletedRef
            }
        ));

        let mut unresolved = input(&geometry, [10.0, 10.0, 0.0], [0.0; 3]);
        unresolved.geometry = None;
        assert!(matches!(
            decide(&unresolved, &world.surveyor()).verdict,
            Verdict::Skip {
                reason: SkipReason::UnresolvedStatic
            }
        ));
    }

    fn rock(min: [f32; 3], max: [f32; 3]) -> StaticOccluder {
        StaticOccluder::new_for_test(
            "terrain_rock",
            RapierCollider::from_world_aabb(WorldAabb { min, max }),
        )
    }

    #[test]
    fn ref_inside_a_static_is_deleted() {
        let mut world = World::new(flat_terrain(100.0));
        world.occluders =
            StaticOccluderIndex::new(vec![rock([900.0, 900.0, 50.0], [1100.0, 1100.0, 300.0])]);
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 99.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Delete {
                reason: DeleteReason::InsideStatic { .. }
            }
        ));
    }

    #[test]
    fn ref_touching_a_static_moves_to_the_nearest_clear_spot_and_regrounds() {
        let mut world = World::new(flat_terrain(100.0));
        world.occluders =
            StaticOccluderIndex::new(vec![rock([1005.0, 900.0, 50.0], [1200.0, 1100.0, 300.0])]);
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 120.0], [0.0; 3]),
            &world.surveyor(),
        );
        let Verdict::Fix(fix) = result.verdict else {
            panic!("expected move, got {:?}", result.verdict);
        };
        assert!(fix.moved);
        assert_eq!(fix.moved_from, Some([1000.0, 1000.0]));
        // +X is blocked by the rock, so the first clear direction is -X one step away.
        assert!((fix.translation[0] - (1000.0 - world.policy.relocation.step)).abs() < 1e-3);
        assert!((fix.translation[2] - (100.0 - world.policy.sink)).abs() < 1e-3);
        assert!(fix.occluder.as_deref().unwrap().starts_with("terrain_rock"));
    }

    #[test]
    fn blocked_ref_with_no_clear_spot_is_deleted_or_reported() {
        let mut world = World::new(flat_terrain(100.0));
        // The whole cell is one rock except the grass' own spot is only touched at its edge.
        world.occluders = StaticOccluderIndex::new(vec![
            rock([1005.0, -100.0, 50.0], [9000.0, 9000.0, 300.0]),
            rock([-100.0, -100.0, 50.0], [995.0, 9000.0, 300.0]),
            rock([995.0, 1005.0, 50.0], [1005.0, 9000.0, 300.0]),
            rock([995.0, -100.0, 50.0], [1005.0, 995.0, 300.0]),
        ]);
        let geometry = geometry();
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 99.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Delete {
                reason: DeleteReason::NoRelocation { .. }
            }
        ));

        world.policy.actions.disable(ActionArg::StaticDelete);
        let result = decide(
            &input(&geometry, [1000.0, 1000.0, 99.0], [0.0; 3]),
            &world.surveyor(),
        );
        assert!(matches!(
            result.verdict,
            Verdict::Keep {
                reason: KeepReason::NoRelocationFound { .. }
            }
        ));
    }

    #[test]
    fn deciding_a_fixed_ref_again_keeps_it() {
        let world = World::new(sloped_terrain());
        let geometry = geometry();
        let first = decide(
            &input(&geometry, [1000.0, 1000.0, 200.0], [0.0, 0.0, 0.7]),
            &world.surveyor(),
        );
        let Verdict::Fix(fix) = first.verdict else {
            panic!("expected fix");
        };
        let second = decide(
            &input(&geometry, fix.translation, fix.rotation),
            &world.surveyor(),
        );
        assert!(
            matches!(second.verdict, Verdict::Keep { .. }),
            "second pass should be a no-op, got {:?}",
            second.verdict
        );
    }
}
