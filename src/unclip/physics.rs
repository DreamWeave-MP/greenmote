// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::Arc;

use glam::{Quat, Vec3};
use rapier3d::{
    math::{Pose3, Rot3, Vec3 as RapierVec3},
    parry::{
        query::{PointQuery, details::intersection_test_support_map_support_map},
        shape::Cuboid,
    },
};

use super::mesh::{
    LocalTriangle, MeshAabb, MeshColliderParts, TRIANGLE_BLOCK, WorldAabb, aabb_corners,
};

/// A reference's collision volume plus the world AABB of all parts for broad-phase pruning.
///
/// Grass references are one box (their visible volume). Static occluders are their collision
/// shapes' exact triangles, shared per mesh and never copied per reference: each part only
/// records where the reference placed the mesh and how it scaled it.
#[derive(Clone, Debug)]
pub(crate) struct RapierCollider {
    bounds: WorldAabb,
    parts: Vec<RapierPart>,
}

#[derive(Clone, Debug)]
enum RapierPart {
    Box {
        cuboid: Cuboid,
        pose: Pose3,
    },
    Mesh {
        /// Unscaled mesh-local triangles, shared with every other reference of the mesh.
        triangles: Arc<[LocalTriangle]>,
        /// Bounds of each triangle, parallel to `triangles` and shared the same way.
        triangle_bounds: Arc<[MeshAabb]>,
        /// Bounds of each block of [`TRIANGLE_BLOCK`] triangles, shared the same way.
        block_bounds: Arc<[MeshAabb]>,
        /// Unscaled mesh-local bounds of `triangles`.
        bounds: MeshAabb,
        /// Mesh-local (scaled) to world.
        pose: Pose3,
        /// The reference's uniform scale, applied before `pose`.
        scale: f32,
    },
}

/// Ray hits closer than this along the ray are ignored, so a corner lying exactly on a face
/// never counts that face.
const RAY_HIT_MIN_T: f32 = 0.000_01;

/// Barycentric distance to a triangle edge under which a ray hit is ambiguous (the ray may
/// also hit the neighbouring face) and the cast is retried along a tilted direction.
const RAY_EDGE_EPSILON: f32 = 0.000_001;

impl RapierCollider {
    /// One axis-aligned box in mesh space, e.g. the visible volume of a grass mesh.
    #[must_use]
    pub(crate) fn from_mesh_bounds(
        bounds: MeshAabb,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        let scale = scale.unwrap_or(1.0);
        let rotation = openmw_rotation(rotation);
        let min = Vec3::from(bounds.min) * scale;
        let max = Vec3::from(bounds.max) * scale;
        let part = box_part(
            min.min(max),
            min.max(max),
            Vec3::from(translation),
            rotation,
        );
        Self::from_parts(vec![part])
    }

    /// The mesh's collision triangles, scaled by the reference's uniform scale and placed with
    /// its rotation and translation. Triangles are shared, not copied.
    #[must_use]
    pub(crate) fn from_mesh_collider_parts(
        parts: &MeshColliderParts,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        let scale = scale.unwrap_or(1.0);
        let pose = isometry(Vec3::from(translation), openmw_rotation(rotation));
        let parts = parts
            .iter()
            .map(|part| RapierPart::Mesh {
                triangles: Arc::clone(&part.triangles),
                triangle_bounds: Arc::clone(&part.triangle_bounds),
                block_bounds: Arc::clone(&part.block_bounds),
                bounds: part.bounds,
                pose,
                scale,
            })
            .collect();
        Self::from_parts(parts)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_world_aabb(bounds: WorldAabb) -> Self {
        let part = box_part(
            Vec3::from(bounds.min),
            Vec3::from(bounds.max),
            Vec3::ZERO,
            Quat::IDENTITY,
        );
        Self {
            bounds,
            parts: vec![part],
        }
    }

    #[must_use]
    pub(crate) const fn bounds(&self) -> WorldAabb {
        self.bounds
    }

    /// True when any part of `self` overlaps any part of `other`. Symmetric: a box against
    /// triangles is tested the same way whichever side holds the triangles.
    #[must_use]
    pub(crate) fn intersects(&self, other: &Self) -> bool {
        self.bounds.intersection(other.bounds).is_some()
            && self
                .parts
                .iter()
                .any(|left| other.parts.iter().any(|right| left.intersects(right)))
    }

    /// True when every corner of every box part of `other` lies inside at least one part of
    /// `self`. Triangle parts count a corner as inside by ray parity, so hollow or open meshes
    /// (an arch, a mushroom cap above the ground) do not swallow what stands under them.
    #[must_use]
    pub(crate) fn contains(&self, other: &Self) -> bool {
        if self.parts.is_empty() || other.parts.is_empty() {
            return false;
        }
        let mut corners = Vec::new();
        for part in &other.parts {
            let RapierPart::Box { cuboid, pose } = part else {
                return false;
            };
            corners.extend(cuboid_world_corners(cuboid, pose));
        }
        corners.into_iter().all(|corner| {
            self.parts
                .iter()
                .any(|part| part.contains_world_point(corner))
        })
    }

    fn from_parts(parts: Vec<RapierPart>) -> Self {
        let bounds = world_bounds_for_parts(&parts);
        Self { bounds, parts }
    }
}

impl RapierPart {
    fn intersects(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Box { cuboid, pose },
                Self::Box {
                    cuboid: other_cuboid,
                    pose: other_pose,
                },
            ) => intersection_test_support_map_support_map(
                &pose.inv_mul(other_pose),
                cuboid,
                other_cuboid,
            ),
            (Self::Mesh { .. }, Self::Box { cuboid, pose })
            | (Self::Box { cuboid, pose }, Self::Mesh { .. }) => {
                let mesh = if matches!(self, Self::Mesh { .. }) {
                    self
                } else {
                    other
                };
                mesh.mesh_intersects_cuboid(cuboid, pose)
            }
            // Occluders are never tested against each other; keep the broad-phase answer.
            (Self::Mesh { .. }, Self::Mesh { .. }) => self
                .world_bounds()
                .intersection(other.world_bounds())
                .is_some(),
        }
    }

    /// Exact test of a box against this part's triangles: the box is taken into the unscaled
    /// mesh frame (a uniform scale keeps it a box) and run through GJK against every triangle
    /// whose bounds overlap it. A box that touches no triangle still intersects when it lies
    /// entirely inside a closed mesh.
    fn mesh_intersects_cuboid(&self, cuboid: &Cuboid, cuboid_pose: &Pose3) -> bool {
        let Self::Mesh {
            triangles,
            triangle_bounds,
            block_bounds,
            bounds,
            pose,
            scale,
        } = self
        else {
            return false;
        };
        let scaled_local = pose.inv_mul(cuboid_pose);
        let local_pose =
            Pose3::from_parts(scaled_local.translation / *scale, scaled_local.rotation);
        let local_cuboid = Cuboid::new(cuboid.half_extents / *scale);
        let (local_min, local_max) =
            points_bounds(cuboid_world_corners(&local_cuboid, &local_pose));
        if !aabbs_overlap(local_min, local_max, bounds.min.into(), bounds.max.into()) {
            return false;
        }

        // Triangles go into the cuboid's own frame, where the cuboid is axis-aligned and the
        // classic 13-axis separating-axis test applies.
        let to_cuboid = local_pose.inverse();
        let touches_surface = triangles
            .chunks(TRIANGLE_BLOCK)
            .zip(triangle_bounds.chunks(TRIANGLE_BLOCK))
            .zip(block_bounds.iter())
            .filter(|(_, block)| {
                aabbs_overlap(local_min, local_max, block.min.into(), block.max.into())
            })
            .any(|((triangles, triangle_bounds), _)| {
                triangles
                    .iter()
                    .zip(triangle_bounds)
                    .any(|(triangle, bounds)| {
                        aabbs_overlap(local_min, local_max, bounds.min.into(), bounds.max.into())
                            && triangle_box_overlap(
                                triangle.map(|vertex| to_cuboid.transform_point(vertex.into())),
                                local_cuboid.half_extents,
                            )
                            .is_some()
                    })
            });
        let centre = local_pose.translation;
        let within_bounds = centre.x >= bounds.min[0]
            && centre.x <= bounds.max[0]
            && centre.y >= bounds.min[1]
            && centre.y <= bounds.max[1]
            && centre.z >= bounds.min[2]
            && centre.z <= bounds.max[2];
        touches_surface
            || (within_bounds
                && point_inside_triangles(triangles, triangle_bounds, block_bounds, centre))
    }

    fn contains_world_point(&self, point: RapierVec3) -> bool {
        match self {
            Self::Box { cuboid, pose } => {
                cuboid.contains_local_point(pose.inverse_transform_point(point))
            }
            Self::Mesh {
                triangles,
                triangle_bounds,
                block_bounds,
                bounds,
                pose,
                scale,
            } => {
                let local = pose.inverse_transform_point(point) / *scale;
                let min = RapierVec3::from(bounds.min);
                let max = RapierVec3::from(bounds.max);
                // The +Z ray can only cross faces when the point is inside the footprint and
                // below the top of the mesh.
                local.x >= min.x
                    && local.x <= max.x
                    && local.y >= min.y
                    && local.y <= max.y
                    && local.z >= min.z
                    && local.z <= max.z
                    && point_inside_triangles(triangles, triangle_bounds, block_bounds, local)
            }
        }
    }

    fn world_vertices(&self) -> Vec<RapierVec3> {
        match self {
            Self::Box { cuboid, pose } => cuboid_world_corners(cuboid, pose).to_vec(),
            Self::Mesh {
                bounds,
                pose,
                scale,
                ..
            } => aabb_corners(bounds.min, bounds.max)
                .into_iter()
                .map(|corner| pose.transform_point(RapierVec3::from(corner) * *scale))
                .collect(),
        }
    }

    fn world_bounds(&self) -> WorldAabb {
        let (min, max) = points_bounds(self.world_vertices());
        WorldAabb {
            min: min.to_array(),
            max: max.to_array(),
        }
    }
}

/// Ray-parity point-in-mesh test: a point is inside when a ray from it crosses the surface an
/// odd number of times. A hit that grazes a triangle edge could be counted twice through the
/// neighbouring face, so such casts are retried along a slightly tilted ray.
fn point_inside_triangles(
    triangles: &[LocalTriangle],
    triangle_bounds: &[MeshAabb],
    block_bounds: &[MeshAabb],
    point: RapierVec3,
) -> bool {
    const DIRECTIONS: [RapierVec3; 2] = [
        RapierVec3::new(0.0, 0.0, 1.0),
        RapierVec3::new(0.017_3, 0.031_1, 1.0),
    ];
    let mut parity = false;
    for (pass, direction) in DIRECTIONS.into_iter().enumerate() {
        let straight_up = pass == 0;
        // Cheap rejection: the ray only rises, and a vertical ray also stays at its x/y.
        let rejects = |bounds: &MeshAabb| {
            bounds.max[2] < point.z
                || (straight_up
                    && (point.x < bounds.min[0]
                        || point.x > bounds.max[0]
                        || point.y < bounds.min[1]
                        || point.y > bounds.max[1]))
        };
        let mut hits = 0_u32;
        let mut ambiguous = false;
        for ((triangles, triangle_bounds), _) in triangles
            .chunks(TRIANGLE_BLOCK)
            .zip(triangle_bounds.chunks(TRIANGLE_BLOCK))
            .zip(block_bounds)
            .filter(|(_, block)| !rejects(block))
        {
            for (triangle, bounds) in triangles.iter().zip(triangle_bounds) {
                if rejects(bounds) {
                    continue;
                }
                if let Some(hit) = ray_hits_triangle(point, direction, triangle) {
                    hits += 1;
                    ambiguous |= hit.on_edge;
                }
            }
        }
        parity = hits % 2 == 1;
        if !ambiguous {
            break;
        }
    }
    parity
}

struct RayHit {
    on_edge: bool,
}

/// Möller–Trumbore ray/triangle intersection, ignoring hits at or behind the ray origin.
fn ray_hits_triangle(
    origin: RapierVec3,
    direction: RapierVec3,
    triangle: &LocalTriangle,
) -> Option<RayHit> {
    let [first, second, third] = triangle.map(RapierVec3::from);
    let edge_to_second = second - first;
    let edge_to_third = third - first;
    let normal_part = direction.cross(edge_to_third);
    let det = edge_to_second.dot(normal_part);
    if det.abs() <= f32::EPSILON * edge_to_second.length() * edge_to_third.length() {
        return None;
    }
    let inv_det = 1.0 / det;
    let from_first = origin - first;
    let bary_second = from_first.dot(normal_part) * inv_det;
    if !(0.0..=1.0).contains(&bary_second) {
        return None;
    }
    let cross_part = from_first.cross(edge_to_second);
    let bary_third = direction.dot(cross_part) * inv_det;
    if bary_third < 0.0 || bary_second + bary_third > 1.0 {
        return None;
    }
    let distance = edge_to_third.dot(cross_part) * inv_det;
    if distance <= RAY_HIT_MIN_T {
        return None;
    }
    Some(RayHit {
        on_edge: bary_second < RAY_EDGE_EPSILON
            || bary_third < RAY_EDGE_EPSILON
            || bary_second + bary_third > 1.0 - RAY_EDGE_EPSILON,
    })
}

/// Akenine-Möller triangle/box overlap: the triangle's vertices are in the frame of a box
/// centred at the origin with the given half extents. Tests the 3 box axes, the triangle
/// normal and the 9 edge cross products; `None` when some axis separates them, otherwise the
/// smallest overlap over the tested axes (the penetration depth along the best axis).
fn triangle_box_overlap(vertices: [RapierVec3; 3], half_extents: RapierVec3) -> Option<f32> {
    let [v0, v1, v2] = vertices;
    let edges = [v1 - v0, v2 - v1, v0 - v2];
    let mut depth = f32::INFINITY;
    let mut test = |axis: RapierVec3| -> bool {
        let length = axis.length();
        if length <= f32::EPSILON {
            return true;
        }
        let projections = [v0.dot(axis), v1.dot(axis), v2.dot(axis)];
        let low = projections[0].min(projections[1]).min(projections[2]);
        let high = projections[0].max(projections[1]).max(projections[2]);
        let radius = half_extents.dot(axis.abs());
        if low > radius || high < -radius {
            return false;
        }
        depth = depth.min((high.min(radius) - low.max(-radius)) / length);
        true
    };
    let separated = !(test(RapierVec3::X)
        && test(RapierVec3::Y)
        && test(RapierVec3::Z)
        && test(edges[0].cross(edges[1]))
        && edges.iter().all(|edge| {
            test(RapierVec3::X.cross(*edge))
                && test(RapierVec3::Y.cross(*edge))
                && test(RapierVec3::Z.cross(*edge))
        }));
    (!separated).then_some(depth)
}

fn cuboid_world_corners(cuboid: &Cuboid, pose: &Pose3) -> [RapierVec3; 8] {
    let half = cuboid.half_extents;
    aabb_corners((-half).into(), half.into()).map(|corner| pose.transform_point(corner.into()))
}

fn aabbs_overlap(
    left_min: RapierVec3,
    left_max: RapierVec3,
    right_min: RapierVec3,
    right_max: RapierVec3,
) -> bool {
    left_min.x <= right_max.x
        && left_max.x >= right_min.x
        && left_min.y <= right_max.y
        && left_max.y >= right_min.y
        && left_min.z <= right_max.z
        && left_max.z >= right_min.z
}

fn points_bounds(points: impl IntoIterator<Item = RapierVec3>) -> (RapierVec3, RapierVec3) {
    points.into_iter().fold(
        (
            RapierVec3::splat(f32::INFINITY),
            RapierVec3::splat(f32::NEG_INFINITY),
        ),
        |(min, max), point| (min.min(point), max.max(point)),
    )
}

/// A box spanning `min..max` in the reference's local frame, placed by `rotation` and
/// `translation`. Half extents are floored at `f32::EPSILON` so flat grass still has a volume.
fn box_part(min: Vec3, max: Vec3, translation: Vec3, rotation: Quat) -> RapierPart {
    let center = (min + max) * 0.5;
    let half = ((max - min).abs() * 0.5).max(Vec3::splat(f32::EPSILON));
    RapierPart::Box {
        cuboid: Cuboid::new(RapierVec3::new(half.x, half.y, half.z)),
        pose: isometry(rotation * center + translation, rotation),
    }
}

fn world_bounds_for_parts(parts: &[RapierPart]) -> WorldAabb {
    let (min, max) = points_bounds(parts.iter().flat_map(RapierPart::world_vertices));
    WorldAabb {
        min: min.to_array(),
        max: max.to_array(),
    }
}

fn openmw_rotation(rotation: [f32; 3]) -> Quat {
    super::transform::world_quat(rotation)
}

fn isometry(translation: Vec3, rotation: Quat) -> Pose3 {
    Pose3::from_parts(
        RapierVec3::new(translation.x, translation.y, translation.z),
        Rot3::from_xyzw(rotation.x, rotation.y, rotation.z, rotation.w),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unclip::mesh::{LocalObb, box_triangles};
    use rapier3d::parry::shape::Triangle;

    #[test]
    fn broad_aabb_overlap_without_obb_collision_is_clear() {
        let left = RapierCollider::from_mesh_bounds(
            mesh_bounds([-5.0, -0.5, -0.5], [5.0, 0.5, 0.5]),
            [0.0; 3],
            [0.0, 0.0, std::f32::consts::FRAC_PI_4],
            None,
        );
        let right =
            RapierCollider::from_world_aabb(world_bounds([-0.25, 3.25, -0.25], [0.25, 3.75, 0.25]));

        assert!(left.bounds().intersection(right.bounds()).is_some());
        assert!(!left.intersects(&right));
    }

    #[test]
    fn actual_obb_collision_is_detected() {
        let left = RapierCollider::from_world_aabb(world_bounds([0.0; 3], [2.0; 3]));
        let right = RapierCollider::from_world_aabb(world_bounds([1.0, 1.0, 1.0], [3.0; 3]));

        assert!(left.intersects(&right));
    }

    #[test]
    fn separated_compound_parts_leave_gap_clear() {
        let parts = MeshColliderParts::from_local_obbs([
            LocalObb {
                center: [-10.0, 0.0, 0.0],
                half_extents: [2.0, 2.0, 2.0],
                orientation: Quat::IDENTITY,
            },
            LocalObb {
                center: [10.0, 0.0, 0.0],
                half_extents: [2.0, 2.0, 2.0],
                orientation: Quat::IDENTITY,
            },
        ]);
        let compound = RapierCollider::from_mesh_collider_parts(&parts, [0.0; 3], [0.0; 3], None);
        let gap_probe = RapierCollider::from_world_aabb(world_bounds([-1.0; 3], [1.0; 3]));

        assert_eq!(compound.parts.len(), 2);
        assert!(compound.bounds().intersection(gap_probe.bounds()).is_some());
        assert!(!compound.intersects(&gap_probe));
        assert!(!gap_probe.intersects(&compound));
    }

    #[test]
    fn mesh_collider_part_orientation_combines_with_reference_rotation() {
        let parts = MeshColliderParts::from_local_obbs([LocalObb {
            center: [0.0; 3],
            half_extents: [5.0, 0.5, 0.5],
            orientation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        }]);
        let collider = RapierCollider::from_mesh_collider_parts(
            &parts,
            [0.0; 3],
            [0.0, 0.0, std::f32::consts::FRAC_PI_4],
            None,
        );
        let probe =
            RapierCollider::from_world_aabb(world_bounds([-0.25, 3.25, -0.25], [0.25, 3.75, 0.25]));

        assert!(collider.bounds().intersection(probe.bounds()).is_some());
        assert!(!collider.intersects(&probe));
    }

    #[test]
    fn mesh_bounds_and_single_part_fallback_are_equivalent_for_rotated_scaled_refs() {
        let bounds = mesh_bounds([-3.0, -1.0, -0.5], [5.0, 2.0, 4.0]);
        let translation = [12.0, -8.0, 3.0];
        let rotation = [0.4, -0.2, 0.9];
        let scale = Some(1.75);
        let from_bounds = RapierCollider::from_mesh_bounds(bounds, translation, rotation, scale);
        let from_parts = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_mesh_aabb(bounds),
            translation,
            rotation,
            scale,
        );
        let inside =
            RapierCollider::from_world_aabb(world_bounds([12.0, -8.0, 3.0], [13.0, -7.0, 4.0]));
        let outside =
            RapierCollider::from_world_aabb(world_bounds([40.0, -8.0, 3.0], [41.0, -7.0, 4.0]));

        assert_bounds_close(from_bounds.bounds(), from_parts.bounds(), 0.01);
        assert!(from_bounds.intersects(&inside));
        assert!(from_parts.intersects(&inside));
        assert!(!from_bounds.intersects(&outside));
        assert!(!from_parts.intersects(&outside));
    }

    #[test]
    fn single_part_containment_matches_mesh_bounds_fallback() {
        let outer_bounds = mesh_bounds([-4.0, -4.0, -4.0], [4.0, 4.0, 4.0]);
        let inner = RapierCollider::from_world_aabb(world_bounds([-1.0; 3], [1.0; 3]));
        let from_bounds = RapierCollider::from_mesh_bounds(outer_bounds, [0.0; 3], [0.0; 3], None);
        let from_parts = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_mesh_aabb(outer_bounds),
            [0.0; 3],
            [0.0; 3],
            None,
        );

        assert!(from_bounds.contains(&inner));
        assert!(from_parts.contains(&inner));
    }

    #[test]
    fn mushroom_cap_does_not_block_ground_grass_but_trunk_does() {
        let tree =
            RapierCollider::from_mesh_collider_parts(&mushroom_parts(), [0.0; 3], [0.0; 3], None);
        let under_cap =
            RapierCollider::from_world_aabb(world_bounds([195.0, -5.0, 0.0], [205.0, 5.0, 40.0]));
        let at_trunk =
            RapierCollider::from_world_aabb(world_bounds([15.0, -5.0, 0.0], [25.0, 5.0, 40.0]));
        let inside_trunk =
            RapierCollider::from_world_aabb(world_bounds([-5.0, -5.0, 10.0], [5.0, 5.0, 30.0]));

        assert!(tree.bounds().intersection(under_cap.bounds()).is_some());
        assert!(!tree.intersects(&under_cap));
        assert!(!tree.contains(&under_cap));
        assert!(tree.intersects(&at_trunk));
        assert!(at_trunk.intersects(&tree));
        assert!(!tree.contains(&at_trunk));
        assert!(tree.contains(&inside_trunk));
        assert!(tree.intersects(&inside_trunk));
    }

    #[test]
    fn closed_rock_contains_inner_grass_and_intersects_straddling_grass() {
        let rock = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_triangles([box_triangles(aabb_corners(
                [-100.0, -100.0, -20.0],
                [100.0, 100.0, 150.0],
            ))]),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let inside = RapierCollider::from_world_aabb(world_bounds([-5.0; 3], [5.0; 3]));
        let straddling =
            RapierCollider::from_world_aabb(world_bounds([90.0, -5.0, 0.0], [110.0, 5.0, 40.0]));
        let clear =
            RapierCollider::from_world_aabb(world_bounds([110.0, -5.0, 0.0], [120.0, 5.0, 40.0]));

        assert!(rock.contains(&inside));
        assert!(rock.intersects(&inside));
        assert!(rock.intersects(&straddling));
        assert!(!rock.contains(&straddling));
        assert!(!rock.intersects(&clear));
    }

    #[test]
    fn open_arch_does_not_contain_grass_under_its_lintel() {
        let arch = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_triangles([[
                box_triangles(aabb_corners([-100.0, -20.0, 0.0], [-80.0, 20.0, 200.0])),
                box_triangles(aabb_corners([80.0, -20.0, 0.0], [100.0, 20.0, 200.0])),
                box_triangles(aabb_corners([-100.0, -20.0, 200.0], [100.0, 20.0, 220.0])),
            ]
            .concat()]),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let under_lintel =
            RapierCollider::from_world_aabb(world_bounds([-5.0, -5.0, 0.0], [5.0, 5.0, 40.0]));
        let in_pillar =
            RapierCollider::from_world_aabb(world_bounds([85.0, -5.0, 10.0], [95.0, 5.0, 40.0]));

        assert!(arch.bounds().intersection(under_lintel.bounds()).is_some());
        assert!(!arch.contains(&under_lintel));
        assert!(!arch.intersects(&under_lintel));
        assert!(arch.contains(&in_pillar));
    }

    #[test]
    fn reference_scale_doubles_the_effective_size() {
        let probe =
            RapierCollider::from_world_aabb(world_bounds([1030.0, -5.0, 0.0], [1035.0, 5.0, 40.0]));
        let unscaled = RapierCollider::from_mesh_collider_parts(
            &mushroom_parts(),
            [1000.0, 0.0, 0.0],
            [0.0; 3],
            None,
        );
        let doubled = RapierCollider::from_mesh_collider_parts(
            &mushroom_parts(),
            [1000.0, 0.0, 0.0],
            [0.0; 3],
            Some(2.0),
        );

        assert!(!unscaled.intersects(&probe));
        assert!(doubled.intersects(&probe));
        assert_bounds_close(
            doubled.bounds(),
            world_bounds([400.0, -600.0, 0.0], [1600.0, 600.0, 900.0]),
            0.01,
        );
        let inside_doubled_trunk = RapierCollider::from_world_aabb(world_bounds(
            [1030.0, -5.0, 10.0],
            [1035.0, 5.0, 40.0],
        ));
        assert!(doubled.contains(&inside_doubled_trunk));
        assert!(!unscaled.contains(&inside_doubled_trunk));
    }

    #[test]
    fn rotated_reference_moves_the_trunk() {
        let tree = RapierCollider::from_mesh_collider_parts(
            &mushroom_parts(),
            [0.0; 3],
            [0.0, 0.0, std::f32::consts::FRAC_PI_2],
            None,
        );
        // Lying on its side after a quarter turn about x would change z; a turn about z keeps
        // the trunk at the origin, so grass at the trunk still collides and grass under the
        // cap still does not.
        let at_trunk =
            RapierCollider::from_world_aabb(world_bounds([15.0, -5.0, 0.0], [25.0, 5.0, 40.0]));
        let under_cap =
            RapierCollider::from_world_aabb(world_bounds([-5.0, 195.0, 0.0], [5.0, 205.0, 40.0]));

        assert!(tree.intersects(&at_trunk));
        assert!(!tree.intersects(&under_cap));
    }

    #[test]
    fn triangles_are_shared_between_references() {
        let parts = mushroom_parts();
        let first = RapierCollider::from_mesh_collider_parts(&parts, [0.0; 3], [0.0; 3], None);
        let second =
            RapierCollider::from_mesh_collider_parts(&parts, [500.0, 0.0, 0.0], [0.0; 3], None);

        let (RapierPart::Mesh { triangles: a, .. }, RapierPart::Mesh { triangles: b, .. }) =
            (&first.parts[0], &second.parts[0])
        else {
            panic!("expected mesh parts");
        };
        assert!(Arc::ptr_eq(a, b));
    }

    #[test]
    fn empty_collider_parts_never_intersect_or_contain() {
        let empty = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_triangles(Vec::<Vec<LocalTriangle>>::new()),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let probe = RapierCollider::from_world_aabb(world_bounds([-1.0; 3], [1.0; 3]));

        assert!(empty.parts.is_empty());
        assert!(!empty.intersects(&probe));
        assert!(!empty.contains(&probe));
        assert!(!probe.contains(&empty));
    }

    #[test]
    fn ray_parity_ignores_hits_at_the_origin_and_edges_consistently() {
        let cube = box_triangles(aabb_corners([-10.0; 3], [10.0; 3]));

        assert!(inside(&cube, RapierVec3::ZERO));
        // On the diagonal edge of the top face's triangles, straight below it.
        assert!(inside(&cube, RapierVec3::new(5.0, 5.0, 0.0)));
        // On the top face itself: the face is not counted, nothing lies above.
        assert!(!inside(&cube, RapierVec3::new(0.0, 0.0, 10.0)));
        // On the bottom face: the bottom is not counted but the top is.
        assert!(inside(&cube, RapierVec3::new(0.0, 0.0, -10.0)));
        assert!(!inside(&cube, RapierVec3::new(11.0, 0.0, 0.0)));
        assert!(!inside(&cube, RapierVec3::new(0.0, 0.0, 11.0)));
    }

    /// Fixed-seed LCG so the comparison below is reproducible without extra dependencies.
    struct Lcg(u64);

    impl Lcg {
        fn next_f32(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            f32::from(u16::try_from(self.0 >> 48).expect("16 high bits")) / 65_536.0
        }

        fn range(&mut self, low: f32, high: f32) -> f32 {
            low + (high - low) * self.next_f32()
        }

        fn vec(&mut self, low: f32, high: f32) -> RapierVec3 {
            RapierVec3::new(
                self.range(low, high),
                self.range(low, high),
                self.range(low, high),
            )
        }

        fn rotation(&mut self) -> Rot3 {
            let axis = Vec3::new(
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
                self.range(-1.0, 1.0),
            )
            .try_normalize()
            .unwrap_or(Vec3::Z);
            let quat = Quat::from_axis_angle(axis, self.range(0.0, std::f32::consts::TAU));
            Rot3::from_xyzw(quat.x, quat.y, quat.z, quat.w)
        }
    }

    #[test]
    fn separating_axis_test_agrees_with_parry_gjk() {
        use rapier3d::parry::query::distance;

        // parry's GJK declares an intersection once the shapes are within a tolerance
        // relative to their size; samples live within +-40 units, so allow 1e-4 of that
        // hundred-unit extent either way.
        const TOLERANCE: f32 = 0.01;
        let mut rng = Lcg(0x5eed_1234_abcd_0001);
        let mut overlaps = 0;
        let mut checked = 0;
        for sample in 0..6000 {
            let half = RapierVec3::new(
                rng.range(0.5, 12.0),
                rng.range(0.5, 12.0),
                rng.range(0.5, 25.0),
            );
            let cuboid = Cuboid::new(half);
            let cuboid_pose = Pose3::from_parts(rng.vec(-20.0, 20.0), rng.rotation());
            let mut triangle = [
                rng.vec(-40.0, 40.0),
                rng.vec(-40.0, 40.0),
                rng.vec(-40.0, 40.0),
            ];
            // Every third sample is nudged so it just touches or just misses the box: move
            // the triangle towards the box centre by its distance minus a tiny margin.
            if sample % 3 == 0 {
                let base = Triangle::new(triangle[0], triangle[1], triangle[2]);
                let apart = distance(&cuboid_pose, &cuboid, &Pose3::IDENTITY, &base)
                    .unwrap()
                    .distance;
                if apart > 0.0 {
                    let centroid = (triangle[0] + triangle[1] + triangle[2]) / 3.0;
                    let towards = (cuboid_pose.translation - centroid).normalize_or_zero();
                    let margin = if sample % 2 == 0 { 0.001 } else { -0.001 };
                    let shift = towards * (apart - margin);
                    triangle = triangle.map(|vertex| vertex + shift);
                }
            }
            let parry_triangle = Triangle::new(triangle[0], triangle[1], triangle[2]);
            let expected = intersection_test_support_map_support_map(
                &cuboid_pose.inv_mul(&Pose3::IDENTITY),
                &cuboid,
                &parry_triangle,
            );
            let apart = distance(&cuboid_pose, &cuboid, &Pose3::IDENTITY, &parry_triangle)
                .unwrap()
                .distance;
            let to_cuboid = cuboid_pose.inverse();
            let actual = triangle_box_overlap(triangle.map(|v| to_cuboid.transform_point(v)), half);

            let ambiguous = apart < TOLERANCE && actual.is_none_or(|depth| depth < TOLERANCE);
            if !ambiguous {
                assert_eq!(
                    actual.is_some(),
                    expected,
                    "sample {sample}: distance {apart}, depth {actual:?}"
                );
                checked += 1;
            }
            overlaps += usize::from(actual.is_some());
        }
        assert!(checked > 4000, "{checked} unambiguous samples");
        assert!(overlaps > 500 && overlaps < 5500, "{overlaps} overlaps");
    }

    #[test]
    fn separating_axis_test_handles_touching_and_near_miss_cases() {
        let half = RapierVec3::splat(1.0);
        let touching_vertex = [
            RapierVec3::new(1.0, 0.0, 0.0),
            RapierVec3::new(3.0, 1.0, 0.0),
            RapierVec3::new(3.0, -1.0, 1.0),
        ];
        let near_miss = touching_vertex.map(|v| v + RapierVec3::new(0.001, 0.0, 0.0));
        let edge_on_edge = [
            RapierVec3::new(1.0, 1.0, -3.0),
            RapierVec3::new(1.0, 1.0, 3.0),
            RapierVec3::new(4.0, 4.0, 0.0),
        ];
        let through = [
            RapierVec3::new(-5.0, 0.0, 0.5),
            RapierVec3::new(5.0, 3.0, 0.5),
            RapierVec3::new(5.0, -3.0, 0.5),
        ];
        // The plane x + y + z = 2.8 cuts the (1, 1, 1) corner although every vertex lies
        // outside the box; shifted out by 0.2 the plane passes 3.4 and misses it.
        let corner_clipping = [
            RapierVec3::new(0.5, 0.5, 1.8),
            RapierVec3::new(1.8, 0.5, 0.5),
            RapierVec3::new(0.5, 1.8, 0.5),
        ];
        let corner_missing = corner_clipping.map(|v| v + RapierVec3::splat(0.2));

        assert!(triangle_box_overlap(touching_vertex, half).is_some());
        assert!(triangle_box_overlap(near_miss, half).is_none());
        assert!(triangle_box_overlap(edge_on_edge, half).is_some());
        assert!(triangle_box_overlap(through, half).is_some());
        assert!(triangle_box_overlap(corner_clipping, half).is_some());
        assert!(triangle_box_overlap(corner_missing, half).is_none());
    }

    fn inside(triangles: &[LocalTriangle], point: RapierVec3) -> bool {
        let part = crate::unclip::mesh::MeshColliderPart::new(triangles.to_vec()).unwrap();
        point_inside_triangles(
            &part.triangles,
            &part.triangle_bounds,
            &part.block_bounds,
            point,
        )
    }

    /// Thin trunk from z 0..400 under a wide flat cap z 400..450 spanning +-300 in x and y.
    fn mushroom_parts() -> MeshColliderParts {
        MeshColliderParts::from_triangles([
            box_triangles(aabb_corners([-20.0, -20.0, 0.0], [20.0, 20.0, 400.0])),
            box_triangles(aabb_corners([-300.0, -300.0, 400.0], [300.0, 300.0, 450.0])),
        ])
    }

    fn mesh_bounds(min: [f32; 3], max: [f32; 3]) -> MeshAabb {
        MeshAabb { min, max }
    }

    fn world_bounds(min: [f32; 3], max: [f32; 3]) -> WorldAabb {
        WorldAabb { min, max }
    }

    fn assert_bounds_close(actual: WorldAabb, expected: WorldAabb, tolerance: f32) {
        for (actual, expected) in actual
            .min
            .into_iter()
            .chain(actual.max)
            .zip(expected.min.into_iter().chain(expected.max))
        {
            assert!(
                (actual - expected).abs() <= tolerance,
                "{actual} vs {expected}"
            );
        }
    }
}
