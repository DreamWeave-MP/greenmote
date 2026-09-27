// SPDX-License-Identifier: GPL-3.0-only

use glam::{Quat, Vec3};
use rapier3d::{
    math::{Pose3, Rot3, Vec3 as RapierVec3},
    parry::{
        query::{PointQuery, details::intersection_test_support_map_support_map},
        shape::{Ball, Cuboid, SupportMap},
    },
};

use super::mesh::{MeshAabb, MeshColliderParts, WorldAabb, aabb_corners};

/// A reference's collision volume: one convex part per collision shape, each with its own
/// world pose, plus the world AABB of all parts for broad-phase pruning.
#[derive(Clone, Debug)]
pub(crate) struct RapierCollider {
    bounds: WorldAabb,
    parts: Vec<RapierPart>,
}

#[derive(Clone, Debug)]
struct RapierPart {
    shape: PartShape,
    pose: Pose3,
}

/// The convex shapes a part can take. `parry`'s `ConvexPolyhedron` and hull builder need its
/// `alloc` feature, which this crate's `rapier3d` dependency does not enable, so hulls are
/// represented here by their point set and fed to `parry`'s GJK through [`SupportMap`]: the
/// support function of a point cloud is exactly that of its convex hull.
#[derive(Clone, Debug)]
enum PartShape {
    Hull(ConvexHull),
    Box(Cuboid),
}

/// The convex hull of a point set in the part's local frame, kept implicit: the points span a
/// volume, and every query goes through the support function.
#[derive(Clone, Debug)]
struct ConvexHull {
    vertices: Vec<RapierVec3>,
}

/// Smallest half extent of a box that stands in for a degenerate (flat or tiny) part.
const DEGENERATE_PART_MIN_HALF_EXTENT: f32 = 0.5;

/// A point set whose extent off its best-fitting line or plane is below this fraction of its
/// largest extent is treated as flat and replaced by a box.
const HULL_MIN_RELATIVE_THICKNESS: f32 = 0.000_1;

/// Tolerance for point-in-hull tests, relative to the hull's largest extent.
const HULL_CONTAINS_RELATIVE_TOLERANCE: f32 = 0.000_01;

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
        let part = cuboid_part(
            min.min(max),
            min.max(max),
            f32::EPSILON,
            Vec3::from(translation),
            rotation,
        );
        Self::from_parts(vec![part])
    }

    /// One convex hull per collider part, scaled by the reference's uniform scale and placed
    /// with the reference's rotation and translation.
    #[must_use]
    pub(crate) fn from_mesh_collider_parts(
        parts: &MeshColliderParts,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        let scale = scale.unwrap_or(1.0);
        let rotation = openmw_rotation(rotation);
        let translation = Vec3::from(translation);
        let parts = parts
            .iter()
            .filter_map(|part| convex_part(&part.points, scale, translation, rotation))
            .collect();
        Self::from_parts(parts)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_world_aabb(bounds: WorldAabb) -> Self {
        let part = cuboid_part(
            Vec3::from(bounds.min),
            Vec3::from(bounds.max),
            f32::EPSILON,
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

    /// True when any part of `self` overlaps any part of `other`.
    #[must_use]
    pub(crate) fn intersects(&self, other: &Self) -> bool {
        self.bounds.intersection(other.bounds).is_some()
            && self.parts.iter().any(|left| {
                other.parts.iter().any(|right| {
                    let pose12 = left.pose.inv_mul(&right.pose);
                    intersection_test_support_map_support_map(
                        &pose12,
                        left.shape.support_map(),
                        right.shape.support_map(),
                    )
                })
            })
    }

    /// True when every vertex of every part of `other` lies inside at least one part of `self`.
    /// For convex `other` parts this means `other` is entirely inside a convex part of `self`
    /// (or the union of several, whenever the vertices happen to fall that way).
    #[must_use]
    pub(crate) fn contains(&self, other: &Self) -> bool {
        if self.parts.is_empty() || other.parts.is_empty() {
            return false;
        }
        other
            .parts
            .iter()
            .flat_map(RapierPart::world_vertices)
            .all(|vertex| {
                self.parts
                    .iter()
                    .any(|part| part.contains_world_point(vertex))
            })
    }

    fn from_parts(parts: Vec<RapierPart>) -> Self {
        let bounds = world_bounds_for_parts(&parts);
        Self { bounds, parts }
    }
}

impl RapierPart {
    /// The part's defining vertices (hull vertices or box corners) in world space.
    fn world_vertices(&self) -> Vec<RapierVec3> {
        self.shape
            .local_vertices()
            .into_iter()
            .map(|point| self.pose.transform_point(point))
            .collect()
    }

    fn contains_world_point(&self, point: RapierVec3) -> bool {
        self.shape
            .contains_local_point(self.pose.inverse_transform_point(point))
    }
}

impl PartShape {
    fn support_map(&self) -> &dyn SupportMap {
        match self {
            Self::Hull(hull) => hull,
            Self::Box(cuboid) => cuboid,
        }
    }

    fn local_vertices(&self) -> Vec<RapierVec3> {
        match self {
            Self::Hull(hull) => hull.vertices.clone(),
            Self::Box(cuboid) => {
                let half = cuboid.half_extents;
                aabb_corners((-half).into(), half.into())
                    .into_iter()
                    .map(RapierVec3::from)
                    .collect()
            }
        }
    }

    fn contains_local_point(&self, point: RapierVec3) -> bool {
        match self {
            Self::Hull(hull) => hull.contains_local_point(point),
            Self::Box(cuboid) => cuboid.contains_local_point(point),
        }
    }
}

impl ConvexHull {
    /// Wraps `points` when they span a volume; `None` for fewer than four points or a point set
    /// that is (numerically) a point, a line, or a plane.
    fn new(points: &[RapierVec3]) -> Option<Self> {
        if points.len() < 4 || !spans_volume(points) {
            return None;
        }
        Some(Self {
            vertices: points.to_vec(),
        })
    }

    /// GJK between the hull and a tiny ball at `point`; points on the surface count as inside.
    fn contains_local_point(&self, point: RapierVec3) -> bool {
        let tolerance = HULL_CONTAINS_RELATIVE_TOLERANCE * largest_extent(&self.vertices);
        let pose = Pose3::from_parts(point, Rot3::IDENTITY);
        intersection_test_support_map_support_map(&pose, self, &Ball::new(tolerance))
    }
}

/// Quickhull's initial-simplex test: the point farthest from a line through two extreme points
/// and then the point farthest from that plane must both be clearly off it.
fn spans_volume(points: &[RapierVec3]) -> bool {
    let threshold = HULL_MIN_RELATIVE_THICKNESS * largest_extent(points);
    let Some(&a) = points.first() else {
        return false;
    };
    let Some(&b) = farthest_by(points, |point| (point - a).length_squared()) else {
        return false;
    };
    let axis = b - a;
    if axis.length() <= threshold {
        return false;
    }
    let Some(&c) = farthest_by(points, |point| (point - a).cross(axis).length_squared()) else {
        return false;
    };
    let Some(normal) = (b - a).cross(c - a).try_normalize() else {
        return false;
    };
    if (c - a).cross(axis).length() / axis.length() <= threshold {
        return false;
    }
    farthest_by(points, |point| (point - a).dot(normal).abs())
        .is_some_and(|d| (*d - a).dot(normal).abs() > threshold)
}

fn farthest_by(points: &[RapierVec3], metric: impl Fn(RapierVec3) -> f32) -> Option<&RapierVec3> {
    points
        .iter()
        .max_by(|left, right| metric(**left).total_cmp(&metric(**right)))
}

impl SupportMap for ConvexHull {
    fn local_support_point(&self, dir: RapierVec3) -> RapierVec3 {
        self.vertices
            .iter()
            .copied()
            .max_by(|left, right| left.dot(dir).total_cmp(&right.dot(dir)))
            .unwrap_or(RapierVec3::ZERO)
    }
}

fn largest_extent(points: &[RapierVec3]) -> f32 {
    let (min, max) = points_bounds(points);
    (max - min).max_element().max(1.0)
}

fn points_bounds(points: &[RapierVec3]) -> (RapierVec3, RapierVec3) {
    points.iter().fold(
        (
            RapierVec3::splat(f32::INFINITY),
            RapierVec3::splat(f32::NEG_INFINITY),
        ),
        |(min, max), point| (min.min(*point), max.max(*point)),
    )
}

/// Wraps one part's mesh-local points, scaled by the reference scale, as a convex hull. Falls
/// back to a box around the points when the hull is degenerate (fewer than four points,
/// collinear, or coplanar), so thin collision planes still block.
fn convex_part(
    points: &[[f32; 3]],
    scale: f32,
    translation: Vec3,
    rotation: Quat,
) -> Option<RapierPart> {
    if points.is_empty() {
        return None;
    }
    let scaled: Vec<RapierVec3> = points
        .iter()
        .map(|point| RapierVec3::from(*point) * scale)
        .collect();

    if let Some(hull) = ConvexHull::new(&scaled) {
        return Some(RapierPart {
            shape: PartShape::Hull(hull),
            pose: isometry(translation, rotation),
        });
    }

    let (min, max) = points_bounds(&scaled);
    Some(cuboid_part(
        Vec3::from(min.to_array()),
        Vec3::from(max.to_array()),
        DEGENERATE_PART_MIN_HALF_EXTENT,
        translation,
        rotation,
    ))
}

/// A box spanning `min..max` in the reference's local frame, placed by `rotation` and
/// `translation`. Half extents are floored at `min_half_extent`.
fn cuboid_part(
    min: Vec3,
    max: Vec3,
    min_half_extent: f32,
    translation: Vec3,
    rotation: Quat,
) -> RapierPart {
    let center = (min + max) * 0.5;
    let half = ((max - min).abs() * 0.5).max(Vec3::splat(min_half_extent));
    RapierPart {
        shape: PartShape::Box(Cuboid::new(RapierVec3::new(half.x, half.y, half.z))),
        pose: isometry(rotation * center + translation, rotation),
    }
}

fn world_bounds_for_parts(parts: &[RapierPart]) -> WorldAabb {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for vertex in parts.iter().flat_map(RapierPart::world_vertices) {
        let vertex = Vec3::from(vertex.to_array());
        min = min.min(vertex);
        max = max.max(vertex);
    }
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
    use crate::unclip::mesh::LocalObb;

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
        assert!(
            compound
                .parts
                .iter()
                .all(|part| matches!(part.shape, PartShape::Hull(_)))
        );
        assert!(compound.bounds().intersection(gap_probe.bounds()).is_some());
        assert!(!compound.intersects(&gap_probe));
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
        let probe =
            RapierCollider::from_world_aabb(world_bounds([12.0, -8.0, 3.0], [13.0, -7.0, 4.0]));

        assert_bounds_close(from_bounds.bounds(), from_parts.bounds(), 0.01);
        assert_eq!(
            from_bounds.intersects(&probe),
            from_parts.intersects(&probe)
        );
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
        assert_eq!(from_bounds.contains(&inner), from_parts.contains(&inner));
    }

    #[test]
    fn rounded_rock_hull_leaves_its_box_corner_region_clear() {
        let rock = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_points([octahedron(100.0)]),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        // Well inside the old 200-unit box but outside the octahedron (|x|+|y|+|z| > 100).
        let corner_grass =
            RapierCollider::from_world_aabb(world_bounds([60.0, 60.0, 60.0], [70.0, 70.0, 70.0]));
        let axis_grass =
            RapierCollider::from_world_aabb(world_bounds([80.0, -5.0, -5.0], [90.0, 5.0, 5.0]));

        assert!(rock.bounds().intersection(corner_grass.bounds()).is_some());
        assert!(!rock.intersects(&corner_grass));
        assert!(rock.intersects(&axis_grass));
    }

    #[test]
    fn grass_entirely_inside_convex_rock_is_contained() {
        let rock = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_points([octahedron(100.0)]),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let inside = RapierCollider::from_world_aabb(world_bounds([-5.0; 3], [5.0; 3]));
        let straddling =
            RapierCollider::from_world_aabb(world_bounds([50.0, -5.0, -5.0], [110.0, 5.0, 5.0]));

        assert!(rock.contains(&inside));
        assert!(rock.intersects(&straddling));
        assert!(!rock.contains(&straddling));
    }

    #[test]
    fn tree_canopy_footprint_does_not_block_ground_grass_but_trunk_does() {
        let trunk = aabb_corners([-20.0, -20.0, 0.0], [20.0, 20.0, 400.0]).to_vec();
        let canopy = aabb_corners([-300.0, -300.0, 400.0], [300.0, 300.0, 600.0]).to_vec();
        let tree = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_points([trunk, canopy]),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let under_canopy =
            RapierCollider::from_world_aabb(world_bounds([195.0, -5.0, 0.0], [205.0, 5.0, 20.0]));
        let at_trunk =
            RapierCollider::from_world_aabb(world_bounds([15.0, -5.0, 0.0], [25.0, 5.0, 20.0]));
        let inside_trunk =
            RapierCollider::from_world_aabb(world_bounds([-5.0, -5.0, 10.0], [5.0, 5.0, 30.0]));

        assert_eq!(tree.parts.len(), 2);
        assert!(tree.bounds().intersection(under_canopy.bounds()).is_some());
        assert!(!tree.intersects(&under_canopy));
        assert!(tree.intersects(&at_trunk));
        assert!(!tree.contains(&at_trunk));
        assert!(tree.contains(&inside_trunk));
    }

    #[test]
    fn reference_scale_grows_hull_before_placement() {
        let rock = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_points([octahedron(100.0)]),
            [1000.0, 0.0, 0.0],
            [0.0; 3],
            Some(2.0),
        );
        let probe =
            RapierCollider::from_world_aabb(world_bounds([1150.0, -5.0, -5.0], [1160.0, 5.0, 5.0]));

        assert_bounds_close(
            rock.bounds(),
            world_bounds([800.0, -200.0, -200.0], [1200.0, 200.0, 200.0]),
            0.01,
        );
        assert!(rock.intersects(&probe));
    }

    #[test]
    fn coplanar_part_falls_back_to_thin_box() {
        let plane = vec![
            [-10.0, -10.0, 0.0],
            [10.0, -10.0, 0.0],
            [10.0, 10.0, 0.0],
            [-10.0, 10.0, 0.0],
        ];
        let collider = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_points([plane]),
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let above =
            RapierCollider::from_world_aabb(world_bounds([-1.0, -1.0, 0.2], [1.0, 1.0, 2.0]));
        let clear =
            RapierCollider::from_world_aabb(world_bounds([-1.0, -1.0, 1.0], [1.0, 1.0, 2.0]));

        assert_eq!(collider.parts.len(), 1);
        assert!(matches!(collider.parts[0].shape, PartShape::Box(_)));
        assert_bounds_close(
            collider.bounds(),
            world_bounds([-10.0, -10.0, -0.5], [10.0, 10.0, 0.5]),
            0.001,
        );
        assert!(collider.intersects(&above));
        assert!(!collider.intersects(&clear));
    }

    #[test]
    fn empty_collider_parts_never_intersect_or_contain() {
        let empty = RapierCollider::from_mesh_collider_parts(
            &MeshColliderParts::from_points(Vec::<Vec<[f32; 3]>>::new()),
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

    fn octahedron(radius: f32) -> Vec<[f32; 3]> {
        vec![
            [radius, 0.0, 0.0],
            [-radius, 0.0, 0.0],
            [0.0, radius, 0.0],
            [0.0, -radius, 0.0],
            [0.0, 0.0, radius],
            [0.0, 0.0, -radius],
        ]
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
