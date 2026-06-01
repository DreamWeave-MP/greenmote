use glam::{EulerRot, Quat, Vec3};
use rapier3d::{
    na::{Quaternion, UnitQuaternion},
    prelude::{Cuboid, Isometry, Point, Translation, Vector},
};

use super::mesh::{LocalObb, MeshAabb, MeshColliderParts, WorldAabb};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RapierCollider {
    bounds: WorldAabb,
    cuboids: Vec<RapierCuboid>,
}

#[derive(Clone, Debug, PartialEq)]
struct RapierCuboid {
    half_extents: [f32; 3],
    position: Isometry<f32>,
}

const CLEARANCE_CENTER_FOOTPRINT_FRACTION: f32 = 0.16;
const CLEARANCE_FOOTPRINT_PROBE_FRACTION: f32 = 0.10;
const CLEARANCE_FOOTPRINT_OFFSET_FRACTION: f32 = 0.35;
const CLEARANCE_MIN_HALF_WIDTH: f32 = 4.0;
const CLEARANCE_MAX_HALF_WIDTH: f32 = 48.0;
const CLEARANCE_MIN_HALF_HEIGHT: f32 = 12.0;
const CLEARANCE_MAX_HALF_HEIGHT: f32 = 160.0;

impl RapierCollider {
    #[must_use]
    pub(crate) fn from_mesh_bounds(
        bounds: MeshAabb,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        Self::from_mesh_collider_parts(
            &MeshColliderParts::from_mesh_aabb(bounds),
            translation,
            rotation,
            scale,
        )
    }

    #[must_use]
    pub(crate) fn from_mesh_collider_parts(
        parts: &MeshColliderParts,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        Self::from_local_obbs(
            parts.iter().map(|part| part.obb),
            translation,
            rotation,
            scale,
        )
    }

    #[must_use]
    pub(crate) fn from_world_aabb(bounds: WorldAabb) -> Self {
        let min = Vec3::from(bounds.min);
        let max = Vec3::from(bounds.max);
        let center = (min + max) * 0.5;
        let half = (max - min).abs() * 0.5;
        let cuboid = RapierCuboid {
            half_extents: [
                half.x.max(f32::EPSILON),
                half.y.max(f32::EPSILON),
                half.z.max(f32::EPSILON),
            ],
            position: isometry(center, Quat::IDENTITY),
        };
        Self {
            bounds,
            cuboids: vec![cuboid],
        }
    }

    #[must_use]
    pub(crate) fn placement_clearance_from_mesh_bounds(
        bounds: MeshAabb,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        Self::from_local_obbs(
            placement_clearance_cuboids(bounds),
            translation,
            rotation,
            scale,
        )
    }

    #[must_use]
    pub(crate) const fn bounds(&self) -> WorldAabb {
        self.bounds
    }

    #[must_use]
    pub(crate) fn intersects(&self, other: &Self) -> bool {
        self.bounds.intersection(other.bounds).is_some()
            && self.cuboids.iter().any(|left| {
                other.cuboids.iter().any(|right| {
                    rapier3d::parry::query::intersection_test(
                        &left.position,
                        &left.shape(),
                        &right.position,
                        &right.shape(),
                    )
                    .unwrap_or(false)
                })
            })
    }

    #[must_use]
    pub(crate) fn contains(&self, other: &Self) -> bool {
        if self.cuboids.len() != 1 {
            return false;
        }
        other
            .world_corners()
            .into_iter()
            .all(|corner| self.contains_world_point(corner))
    }

    #[must_use]
    pub(crate) fn translated_xy(&self, x: f32, y: f32) -> Self {
        let mut moved = self.clone();
        moved.bounds.min[0] += x;
        moved.bounds.max[0] += x;
        moved.bounds.min[1] += y;
        moved.bounds.max[1] += y;
        for cuboid in &mut moved.cuboids {
            cuboid.position.translation.vector.x += x;
            cuboid.position.translation.vector.y += y;
        }
        moved
    }

    fn from_local_obbs(
        local_obbs: impl IntoIterator<Item = LocalObb>,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        let scale = scale.unwrap_or(1.0);
        let rotation = openmw_rotation(rotation);
        let translation = Vec3::from(translation);
        let cuboids = local_obbs
            .into_iter()
            .map(|local| local.to_world(translation, rotation, scale))
            .collect::<Vec<_>>();
        let bounds = world_bounds_for_cuboids(&cuboids);
        Self { bounds, cuboids }
    }

    fn contains_world_point(&self, point: [f32; 3]) -> bool {
        let Some(cuboid) = self.cuboids.first() else {
            return false;
        };
        let local = cuboid
            .position
            .inverse_transform_point(&Point::new(point[0], point[1], point[2]));
        local.x.abs() <= cuboid.half_extents[0] + f32::EPSILON
            && local.y.abs() <= cuboid.half_extents[1] + f32::EPSILON
            && local.z.abs() <= cuboid.half_extents[2] + f32::EPSILON
    }

    fn world_corners(&self) -> Vec<[f32; 3]> {
        self.cuboids
            .iter()
            .flat_map(RapierCuboid::world_corners)
            .collect()
    }
}

impl RapierCuboid {
    fn shape(&self) -> Cuboid {
        Cuboid::new(Vector::new(
            self.half_extents[0],
            self.half_extents[1],
            self.half_extents[2],
        ))
    }

    fn world_corners(&self) -> [[f32; 3]; 8] {
        let half = self.half_extents;
        let local = [
            [-half[0], -half[1], -half[2]],
            [-half[0], -half[1], half[2]],
            [-half[0], half[1], -half[2]],
            [-half[0], half[1], half[2]],
            [half[0], -half[1], -half[2]],
            [half[0], -half[1], half[2]],
            [half[0], half[1], -half[2]],
            [half[0], half[1], half[2]],
        ];
        local.map(|point| {
            let world = self
                .position
                .transform_point(&Point::new(point[0], point[1], point[2]));
            [world.x, world.y, world.z]
        })
    }
}

impl LocalObb {
    fn to_world(self, translation: Vec3, rotation: Quat, scale: f32) -> RapierCuboid {
        let center = Vec3::from(self.center) * scale;
        let half = Vec3::from(self.half_extents) * scale.abs();
        let world_center = rotation * center + translation;
        RapierCuboid {
            half_extents: positive_half_extents(half),
            position: isometry(world_center, rotation * self.orientation),
        }
    }
}

fn placement_clearance_cuboids(bounds: MeshAabb) -> [LocalObb; 5] {
    let min = Vec3::from(bounds.min);
    let max = Vec3::from(bounds.max);
    let half_size = (max - min).abs() * 0.5;
    let footprint = half_size.x.max(half_size.y);
    let center_half_width = (footprint * CLEARANCE_CENTER_FOOTPRINT_FRACTION)
        .clamp(CLEARANCE_MIN_HALF_WIDTH, CLEARANCE_MAX_HALF_WIDTH);
    let small_half_width = (footprint * CLEARANCE_FOOTPRINT_PROBE_FRACTION)
        .clamp(CLEARANCE_MIN_HALF_WIDTH, CLEARANCE_MAX_HALF_WIDTH);
    let offset_x = (half_size.x * CLEARANCE_FOOTPRINT_OFFSET_FRACTION).max(center_half_width);
    let offset_y = (half_size.y * CLEARANCE_FOOTPRINT_OFFSET_FRACTION).max(center_half_width);
    let half_height = half_size
        .z
        .clamp(CLEARANCE_MIN_HALF_HEIGHT, CLEARANCE_MAX_HALF_HEIGHT);
    let center_z = (min.z + max.z) * 0.5;

    [
        clearance_cuboid([0.0, 0.0, center_z], center_half_width, half_height),
        clearance_cuboid([offset_x, 0.0, center_z], small_half_width, half_height),
        clearance_cuboid([-offset_x, 0.0, center_z], small_half_width, half_height),
        clearance_cuboid([0.0, offset_y, center_z], small_half_width, half_height),
        clearance_cuboid([0.0, -offset_y, center_z], small_half_width, half_height),
    ]
}

fn clearance_cuboid(center: [f32; 3], half_width: f32, half_height: f32) -> LocalObb {
    LocalObb {
        center,
        half_extents: [half_width, half_width, half_height],
        orientation: Quat::IDENTITY,
    }
}

fn positive_half_extents(half: Vec3) -> [f32; 3] {
    [
        half.x.max(f32::EPSILON),
        half.y.max(f32::EPSILON),
        half.z.max(f32::EPSILON),
    ]
}

fn world_bounds_for_cuboids(cuboids: &[RapierCuboid]) -> WorldAabb {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for corner in cuboids.iter().flat_map(RapierCuboid::world_corners) {
        let corner = Vec3::from(corner);
        min = min.min(corner);
        max = max.max(corner);
    }
    WorldAabb {
        min: min.to_array(),
        max: max.to_array(),
    }
}

fn openmw_rotation(rotation: [f32; 3]) -> Quat {
    // Match the existing mesh transform: OpenMW applies Z, then Y, then X with negated axes.
    Quat::from_euler(EulerRot::ZYX, -rotation[2], -rotation[1], -rotation[0])
}

fn isometry(translation: Vec3, rotation: Quat) -> Isometry<f32> {
    Isometry::from_parts(
        Translation::new(translation.x, translation.y, translation.z),
        UnitQuaternion::from_quaternion(Quaternion::new(
            rotation.w, rotation.x, rotation.y, rotation.z,
        )),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn separated_compound_cuboids_leave_gap_clear() {
        let compound = RapierCollider::from_local_obbs(
            [
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
            ],
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let gap_probe = RapierCollider::from_world_aabb(world_bounds([-1.0; 3], [1.0; 3]));

        assert!(compound.bounds().intersection(gap_probe.bounds()).is_some());
        assert!(!compound.intersects(&gap_probe));
    }

    #[test]
    fn mesh_collider_parts_leave_gap_clear() {
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

        assert_eq!(from_bounds.bounds(), from_parts.bounds());
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

    fn mesh_bounds(min: [f32; 3], max: [f32; 3]) -> MeshAabb {
        MeshAabb { min, max }
    }

    fn world_bounds(min: [f32; 3], max: [f32; 3]) -> WorldAabb {
        WorldAabb { min, max }
    }
}
