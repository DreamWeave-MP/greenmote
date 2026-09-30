// SPDX-License-Identifier: MIT OR Apache-2.0

//! Terrain-aligned orientation for groundcover references.
//!
//! Groundcover generators tilt each instance with the terrain by writing the slope angles
//! straight into the X and Y rotation fields and keeping the random yaw in Z. Unclip reproduces
//! that convention so re-oriented refs look like freshly generated ones.

use super::terrain::TerrainSample;

/// Rotation a reference should have to lie flat on the sampled terrain.
#[must_use]
pub(crate) fn terrain_rotation(current_rotation: [f32; 3], terrain: &TerrainSample) -> [f32; 3] {
    [terrain.angle.xrot, terrain.angle.yrot, current_rotation[2]]
}

/// Largest change in X or Y tilt, in degrees, between two rotations.
#[must_use]
pub(crate) fn tilt_delta_degrees(current: [f32; 3], target: [f32; 3]) -> f32 {
    (current[0] - target[0])
        .abs()
        .max((current[1] - target[1]).abs())
        .to_degrees()
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use crate::unclip::terrain::{TerrainAngle, TerrainSample};

    use super::{terrain_rotation, tilt_delta_degrees};

    fn sample(xrot: f32, yrot: f32) -> TerrainSample {
        TerrainSample {
            height: 0.0,
            normal: [0.0, 0.0, 1.0],
            angle: TerrainAngle { xrot, yrot },
        }
    }

    #[test]
    fn terrain_rotation_keeps_yaw_and_takes_generator_tilt() {
        let rotation = terrain_rotation([0.1, 0.2, -1.3], &sample(0.25, -0.45));
        assert_eq!(rotation, [0.25, -0.45, -1.3]);
    }

    #[test]
    fn tilt_delta_ignores_yaw() {
        assert!((tilt_delta_degrees([0.0, 0.0, 1.0], [0.0, 0.0, -1.0])).abs() < 1e-6);
        let delta = tilt_delta_degrees([0.0, 0.0, 0.0], [0.1_f32.to_radians(), 0.0, 0.0]);
        assert!((delta - 0.1).abs() < 1e-4);
    }
}
