// SPDX-License-Identifier: GPL-3.0-only

//! Reference transform conventions shared by every unclip geometry module.
//!
//! `OpenMW` converts an ESM reference rotation `[rx, ry, rz]` into a scene attitude with
//! `Misc::Convert::makeOsgQuat`, which is `Quat(rz, -Z) * Quat(ry, -Y) * Quat(rx, -X)` in
//! `OpenSceneGraph`. OSG composes quaternions left to right, so the object is rotated about
//! `-Z` first, then `-Y`, then `-X`, all in the world frame. As a column-vector matrix that is
//! `Rx(-rx) * Ry(-ry) * Rz(-rz)`, which `glam` spells as intrinsic `EulerRot::XYZ`.

use glam::{EulerRot, Mat3, Quat};

/// World rotation matrix for an ESM reference rotation, matching `OpenMW`'s direct object order.
#[must_use]
pub(crate) fn world_rotation(rotation: [f32; 3]) -> Mat3 {
    Mat3::from_euler(EulerRot::XYZ, -rotation[0], -rotation[1], -rotation[2])
}

/// World rotation quaternion for an ESM reference rotation, matching `OpenMW`'s direct object order.
#[must_use]
pub(crate) fn world_quat(rotation: [f32; 3]) -> Quat {
    Quat::from_euler(EulerRot::XYZ, -rotation[0], -rotation[1], -rotation[2])
}

#[cfg(test)]
mod tests {
    use glam::Vec3;

    use super::{world_quat, world_rotation};

    // Ground truth computed by composing OpenMW's makeOsgQuat with OpenSceneGraph's quaternion
    // product and rotating (0, 0, 1) for rotation [0.3, 0.4, 1.2]. See scratch derivation in the
    // commit that introduced this module.
    const OPENMW_UP_FOR_030_040_120: [f32; 3] = [-0.38942, 0.27219, 0.87992];

    #[test]
    fn world_rotation_matches_openmw_direct_object_order() {
        let rotated = world_rotation([0.3, 0.4, 1.2]) * Vec3::Z;
        for (actual, expected) in rotated
            .to_array()
            .into_iter()
            .zip(OPENMW_UP_FOR_030_040_120)
        {
            assert!((actual - expected).abs() < 1e-4, "{rotated:?}");
        }
    }

    #[test]
    fn world_quat_agrees_with_world_rotation() {
        let rotation = [0.31, -0.47, 0.83];
        let point = Vec3::new(1.5, -2.0, 0.75);
        let via_matrix = world_rotation(rotation) * point;
        let via_quat = world_quat(rotation) * point;
        assert!((via_matrix - via_quat).length() < 1e-5);
    }

    #[test]
    fn reversed_order_is_rejected() {
        // The previous implementation used intrinsic ZYX, which applies X first in world space.
        let wrong = glam::Mat3::from_euler(glam::EulerRot::ZYX, -1.2, -0.4, -0.3) * Vec3::Z;
        let expected = Vec3::from(OPENMW_UP_FOR_030_040_120);
        assert!((wrong - expected).length() > 0.1);
    }
}
