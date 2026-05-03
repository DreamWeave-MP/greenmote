use glam::{EulerRot, Mat3, Vec3};

const MIN_UPWARD_NORMAL_Z: f32 = 0.001;
const MIN_PROJECTED_AXIS_LENGTH_SQUARED: f32 = 0.000_001;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct OrientationResult {
    pub(crate) new_rotation: [f32; 3],
    pub(crate) terrain_normal: [f32; 3],
    pub(crate) angle_degrees: f32,
}

#[must_use]
pub(crate) fn orientation_to_terrain(
    current_rotation: [f32; 3],
    terrain_normal: [f32; 3],
    epsilon_degrees: f32,
) -> Option<OrientationResult> {
    let normal = Vec3::from(terrain_normal).try_normalize()?;
    if normal.z <= MIN_UPWARD_NORMAL_Z {
        return None;
    }

    let current_up = world_rotation(current_rotation) * Vec3::Z;
    let angle_degrees = current_up.angle_between(normal).to_degrees();
    if angle_degrees <= epsilon_degrees {
        return None;
    }

    let yaw = -current_rotation[2];
    let yaw_axis = Vec3::new(yaw.cos(), yaw.sin(), 0.0);
    let projected_x = yaw_axis - normal * yaw_axis.dot(normal);
    if projected_x.length_squared() <= MIN_PROJECTED_AXIS_LENGTH_SQUARED {
        return None;
    }

    let x_axis = projected_x.normalize();
    let y_axis = normal.cross(x_axis).normalize();
    let rotation = Mat3::from_cols(x_axis, y_axis, normal);
    let (z, y, x) = rotation.to_euler(EulerRot::ZYX);

    Some(OrientationResult {
        new_rotation: [-x, -y, -z],
        terrain_normal: normal.to_array(),
        angle_degrees,
    })
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
    use super::{orientation_angle_degrees, orientation_to_terrain};

    #[test]
    fn flat_normal_leaves_flat_ref_unchanged() {
        assert!(orientation_to_terrain([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0).is_none());
    }

    #[test]
    fn sloped_normal_preserves_stored_yaw() {
        let rotation = orientation_to_terrain(
            [0.0, 0.0, -std::f32::consts::FRAC_PI_2],
            [0.0, -0.5, 1.0],
            0.0,
        )
        .unwrap();

        assert!((rotation.new_rotation[2] + std::f32::consts::FRAC_PI_2).abs() < 0.000_01);
        assert!(rotation.new_rotation[0].abs() > 0.1 || rotation.new_rotation[1].abs() > 0.1);
    }

    #[test]
    fn orientation_angle_reports_tilt_from_current_up() {
        let angle = orientation_angle_degrees([0.0, 0.0, 0.0], [0.0, 1.0, 1.0]).unwrap();

        assert!((angle - 45.0).abs() < 0.000_01);
    }
}
