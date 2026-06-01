use glam::{EulerRot, Quat, Vec3};
use rapier3d::{
    na::{Quaternion, UnitQuaternion},
    prelude::{Cuboid, Isometry, Point, Translation, Vector},
};

use super::mesh::{MeshAabb, WorldAabb};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RapierCollider {
    bounds: WorldAabb,
    half_extents: [f32; 3],
    position: Isometry<f32>,
}

impl RapierCollider {
    #[must_use]
    pub(crate) fn from_mesh_bounds(
        bounds: MeshAabb,
        translation: [f32; 3],
        rotation: [f32; 3],
        scale: Option<f32>,
    ) -> Self {
        let scale = scale.unwrap_or(1.0);
        let local_min = Vec3::from(bounds.min) * scale;
        let local_max = Vec3::from(bounds.max) * scale;
        let center = (local_min + local_max) * 0.5;
        let half = (local_max - local_min).abs() * 0.5;
        let world_bounds = bounds.world_aabb(translation, rotation, Some(scale));
        let rotation = openmw_rotation(rotation);
        let world_center = rotation * center + Vec3::from(translation);

        Self {
            bounds: world_bounds,
            half_extents: [
                half.x.max(f32::EPSILON),
                half.y.max(f32::EPSILON),
                half.z.max(f32::EPSILON),
            ],
            position: isometry(world_center, rotation),
        }
    }

    #[must_use]
    pub(crate) fn from_world_aabb(bounds: WorldAabb) -> Self {
        let min = Vec3::from(bounds.min);
        let max = Vec3::from(bounds.max);
        let center = (min + max) * 0.5;
        let half = (max - min).abs() * 0.5;
        Self {
            bounds,
            half_extents: [
                half.x.max(f32::EPSILON),
                half.y.max(f32::EPSILON),
                half.z.max(f32::EPSILON),
            ],
            position: isometry(center, Quat::IDENTITY),
        }
    }

    #[must_use]
    pub(crate) const fn bounds(&self) -> WorldAabb {
        self.bounds
    }

    #[must_use]
    pub(crate) fn intersects(&self, other: &Self) -> bool {
        let left = self.shape();
        let right = other.shape();
        rapier3d::parry::query::intersection_test(&self.position, &left, &other.position, &right)
            .unwrap_or(false)
    }

    #[must_use]
    pub(crate) fn contains(&self, other: &Self) -> bool {
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
        moved.position.translation.vector.x += x;
        moved.position.translation.vector.y += y;
        moved
    }

    fn shape(&self) -> Cuboid {
        Cuboid::new(Vector::new(
            self.half_extents[0],
            self.half_extents[1],
            self.half_extents[2],
        ))
    }

    fn contains_world_point(&self, point: [f32; 3]) -> bool {
        let local = self
            .position
            .inverse_transform_point(&Point::new(point[0], point[1], point[2]));
        local.x.abs() <= self.half_extents[0] + f32::EPSILON
            && local.y.abs() <= self.half_extents[1] + f32::EPSILON
            && local.z.abs() <= self.half_extents[2] + f32::EPSILON
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

    fn mesh_bounds(min: [f32; 3], max: [f32; 3]) -> MeshAabb {
        MeshAabb { min, max }
    }

    fn world_bounds(min: [f32; 3], max: [f32; 3]) -> WorldAabb {
        WorldAabb { min, max }
    }
}
