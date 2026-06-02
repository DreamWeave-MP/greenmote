// SPDX-License-Identifier: GPL-3.0-only

use glam::{EulerRot, Mat3, Vec3};

use super::terrain::TerrainSample;

const MIN_UPWARD_NORMAL_Z: f32 = 0.001;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OrientationResult {
    pub(crate) new_rotation: [f32; 3],
    pub(crate) terrain_normal: [f32; 3],
    pub(crate) angle_degrees: f32,
}

#[must_use]
pub(crate) fn orientation_to_terrain(
    current_rotation: [f32; 3],
    terrain: TerrainSample,
    epsilon_degrees: f32,
) -> Option<OrientationResult> {
    let normal = Vec3::from(terrain.normal).try_normalize()?;
    if normal.z <= MIN_UPWARD_NORMAL_Z {
        return None;
    }

    let target_rotation = [terrain.angle.xrot, terrain.angle.yrot, current_rotation[2]];
    let angle_degrees = target_rotation_delta_degrees(current_rotation, target_rotation);
    if angle_degrees <= epsilon_degrees {
        return None;
    }

    Some(OrientationResult {
        new_rotation: target_rotation,
        terrain_normal: normal.to_array(),
        angle_degrees,
    })
}

fn target_rotation_delta_degrees(current_rotation: [f32; 3], target_rotation: [f32; 3]) -> f32 {
    (current_rotation[0] - target_rotation[0])
        .abs()
        .max((current_rotation[1] - target_rotation[1]).abs())
        .to_degrees()
}

#[must_use]
pub(crate) fn orientation_angle_degrees(
    current_rotation: [f32; 3],
    terrain_normal: [f32; 3],
) -> Option<f32> {
    let normal = Vec3::from(terrain_normal).try_normalize()?;
    if normal.z <= MIN_UPWARD_NORMAL_Z {
        return None;
    }
    Some(
        (world_rotation(current_rotation) * Vec3::Z)
            .angle_between(normal)
            .to_degrees(),
    )
}

fn world_rotation(rotation: [f32; 3]) -> Mat3 {
    // Match OpenMW's ESM-to-scene conversion: Z, then Y, then X, with negated axes.
    Mat3::from_euler(EulerRot::ZYX, -rotation[2], -rotation[1], -rotation[0])
}

#[cfg(test)]
mod tests {
    use glam::{EulerRot, Mat3, Vec3};

    use crate::unclip::terrain::{TerrainAngle, TerrainSample};

    use super::{orientation_angle_degrees, orientation_to_terrain, world_rotation};

    const EPSILON: f32 = 0.000_1;

    #[test]
    fn flat_normal_leaves_flat_ref_unchanged() {
        assert!(orientation_to_terrain([0.0, 0.0, 0.0], flat_sample(), 1.0).is_none());
    }

    #[test]
    fn generator_style_orientation_preserves_stored_yaw_exactly() {
        let current_rotation = [0.0, 0.0, -std::f32::consts::FRAC_PI_2];
        let rotation =
            orientation_to_terrain(current_rotation, sloped_sample(0.12, -0.34), 0.0).unwrap();

        assert_close(rotation.new_rotation[2], current_rotation[2]);
        assert_close(rotation.new_rotation[0], 0.12);
        assert_close(rotation.new_rotation[1], -0.34);
    }

    #[test]
    fn world_rotation_matches_openmw_direct_object_order_for_multi_axis_rotation() {
        let rotation = [0.31, -0.47, 0.83];
        let expected = Mat3::from_rotation_z(-rotation[2])
            * Mat3::from_rotation_y(-rotation[1])
            * Mat3::from_rotation_x(-rotation[0]);
        let wrong_inverse_order =
            Mat3::from_euler(EulerRot::XYZ, -rotation[0], -rotation[1], -rotation[2]);

        assert_matrix_close(world_rotation(rotation), expected);
        assert_matrix_not_close(world_rotation(rotation), wrong_inverse_order);
    }

    #[test]
    fn orientation_result_uses_generator_tilt_not_full_basis_alignment() {
        let terrain = sloped_sample(0.25, -0.45);
        let result = orientation_to_terrain([0.0, 0.0, 1.1], terrain, 0.0).unwrap();

        assert_close(result.new_rotation[0], 0.25);
        assert_close(result.new_rotation[1], -0.45);
        assert_close(result.new_rotation[2], 1.1);
    }

    #[test]
    fn gate_uses_generator_target_rotation_not_current_up_vs_normal() {
        let current_rotation = [0.0, 0.0, 1.3];
        let terrain = TerrainSample {
            height: 0.0,
            normal: (world_rotation(current_rotation) * Vec3::Z).to_array(),
            angle: TerrainAngle {
                xrot: 0.12,
                yrot: -0.34,
            },
        };

        let result = orientation_to_terrain(current_rotation, terrain, 1.0).unwrap();

        assert_close(result.new_rotation[0], 0.12);
        assert_close(result.new_rotation[1], -0.34);
        assert_close(result.new_rotation[2], current_rotation[2]);
    }

    #[test]
    fn moderate_generator_stencil_does_not_create_near_perpendicular_rotation() {
        let terrain = TerrainSample {
            height: 0.0,
            normal: Vec3::new(20.0, 0.0, 1.0).normalize().to_array(),
            angle: TerrainAngle {
                xrot: 0.2,
                yrot: 0.0,
            },
        };
        let result = orientation_to_terrain([0.0, 0.0, 0.0], terrain, 0.0).unwrap();
        let world_up = world_rotation(result.new_rotation) * Vec3::Z;

        assert!(world_up.angle_between(Vec3::Z).to_degrees() < 15.0);
    }

    #[test]
    fn orientation_angle_reports_tilt_from_current_up() {
        let angle = orientation_angle_degrees([0.0, 0.0, 0.0], [0.0, 1.0, 1.0]).unwrap();

        assert!((angle - 45.0).abs() < 0.000_01);
    }

    fn assert_matrix_close(actual: Mat3, expected: Mat3) {
        for (actual, expected) in actual
            .to_cols_array()
            .into_iter()
            .zip(expected.to_cols_array())
        {
            assert!(
                (actual - expected).abs() < EPSILON,
                "{actual} != {expected}"
            );
        }
    }

    fn assert_matrix_not_close(actual: Mat3, expected: Mat3) {
        assert!(
            actual
                .to_cols_array()
                .into_iter()
                .zip(expected.to_cols_array())
                .any(|(actual, expected)| (actual - expected).abs() >= EPSILON)
        );
    }

    fn flat_sample() -> TerrainSample {
        TerrainSample {
            height: 0.0,
            normal: [0.0, 0.0, 1.0],
            angle: TerrainAngle {
                xrot: 0.0,
                yrot: 0.0,
            },
        }
    }

    fn sloped_sample(xrot: f32, yrot: f32) -> TerrainSample {
        TerrainSample {
            height: 0.0,
            normal: Vec3::new(0.25, -0.45, 1.0).normalize().to_array(),
            angle: TerrainAngle { xrot, yrot },
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{actual} != {expected}"
        );
    }
}
