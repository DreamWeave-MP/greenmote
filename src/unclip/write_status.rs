// SPDX-License-Identifier: GPL-3.0-only

use super::{args::WriteActions, cells::CellCoord, write_plan::WriteStatusIndex};

pub(crate) struct WriteStatusInput {
    pub(crate) reference_deleted: bool,
    pub(crate) mesh_status: MeshResolutionStatus,
    pub(crate) contact_delta: Option<f32>,
    pub(crate) static_bounds_status: Option<&'static str>,
    pub(crate) water_crossing: Option<bool>,
    pub(crate) orientation_angle_degrees: Option<f32>,
    pub(crate) write: WriteStatusEvidence,
    pub(crate) contact_epsilon: f32,
    pub(crate) orientation_epsilon_degrees: f32,
}

#[derive(Clone, Copy)]
pub(crate) enum MeshResolutionStatus {
    Resolved,
    MissingContact,
    UnresolvedStatic,
}

#[derive(Clone, Copy)]
pub(crate) struct WriteStatusEvidence {
    pub(crate) plan: WritePlanEvidence,
    pub(crate) actions: WriteActions,
}

#[derive(Clone, Copy)]
pub(crate) enum WritePlanEvidence {
    NotPlanned,
    Unchanged,
    Adjusted,
    WaterDeleted,
    Deleted,
    Moved,
    Oriented,
}

pub(crate) fn write_plan_evidence(
    write: Option<&WriteStatusIndex>,
    cell: CellCoord,
    key: (u32, u32),
) -> WritePlanEvidence {
    let Some(write) = write else {
        return WritePlanEvidence::NotPlanned;
    };
    if write.is_water_deleted(cell, key) {
        WritePlanEvidence::WaterDeleted
    } else if write.is_deleted(cell, key) {
        WritePlanEvidence::Deleted
    } else if write.is_moved(cell, key) {
        WritePlanEvidence::Moved
    } else if write.is_adjusted(cell, key) {
        WritePlanEvidence::Adjusted
    } else if write.is_oriented(cell, key) {
        WritePlanEvidence::Oriented
    } else {
        WritePlanEvidence::Unchanged
    }
}

pub(crate) fn write_status_label(input: &WriteStatusInput) -> &'static str {
    match input.write.plan {
        WritePlanEvidence::WaterDeleted => return "deleted_water_crossing",
        WritePlanEvidence::Deleted => return "deleted_static_bounds_occluded",
        WritePlanEvidence::Moved => return "moved_static_bounds_occluded",
        WritePlanEvidence::Adjusted => return "adjusted",
        WritePlanEvidence::Oriented => return "oriented_to_terrain",
        WritePlanEvidence::NotPlanned | WritePlanEvidence::Unchanged => {}
    }
    if input.reference_deleted {
        return "skipped_deleted_ref";
    }
    if let Some(status) = water_write_status(input) {
        return status;
    }
    if let Some(occlusion) = input.static_bounds_status {
        match occlusion {
            "static_bounds_fully_occluded" if input.write.actions.static_delete() => {
                if matches!(input.write.plan, WritePlanEvidence::NotPlanned) {
                    return "would_delete_static_bounds_occluded";
                }
            }
            "static_bounds_deleted_no_relocation" | "static_clearance_deleted_no_relocation"
                if input.write.actions.static_delete() =>
            {
                if matches!(input.write.plan, WritePlanEvidence::NotPlanned) {
                    return "would_delete_static_bounds_no_relocation";
                }
            }
            "static_bounds_relocatable" | "static_clearance_relocatable"
                if input.write.actions.static_move() =>
            {
                if matches!(input.write.plan, WritePlanEvidence::NotPlanned) {
                    return "would_move_static_bounds_occluded";
                }
            }
            _ => {}
        }
    }
    if let Some(status) = terrain_write_status(input) {
        return status;
    }
    if let Some(status) = orientation_write_status(input) {
        return status;
    }
    if let Some(occlusion) = input.static_bounds_status {
        return match occlusion {
            "static_bounds_fully_occluded" if !input.write.actions.static_delete() => {
                "skipped_static_delete_disabled"
            }
            "static_bounds_relocatable"
            | "static_bounds_blocked"
            | "static_clearance_relocatable"
            | "static_clearance_blocked"
                if !input.write.actions.static_move() =>
            {
                "skipped_static_move_disabled"
            }
            "static_bounds_blocked" | "static_clearance_blocked"
                if input.write.actions.static_move() =>
            {
                "static_bounds_blocked_no_relocation"
            }
            _ => mesh_resolution_write_status(input),
        };
    }

    mesh_resolution_write_status(input)
}

fn terrain_write_status(input: &WriteStatusInput) -> Option<&'static str> {
    let MeshResolutionStatus::Resolved = input.mesh_status else {
        return None;
    };
    input.contact_delta.and_then(|delta| {
        if delta.abs() <= input.contact_epsilon {
            return None;
        }
        if !input.write.actions.terrain_z() {
            return Some("skipped_terrain_z_disabled");
        }
        Some(match input.write.plan {
            WritePlanEvidence::NotPlanned => "would_adjust_terrain_z",
            WritePlanEvidence::Unchanged => "skipped_write_plan_unchanged",
            WritePlanEvidence::Adjusted
            | WritePlanEvidence::WaterDeleted
            | WritePlanEvidence::Deleted
            | WritePlanEvidence::Moved
            | WritePlanEvidence::Oriented => unreachable!("handled before terrain status"),
        })
    })
}

fn water_write_status(input: &WriteStatusInput) -> Option<&'static str> {
    input.water_crossing.and_then(|crossing| {
        if !crossing {
            return None;
        }
        if !input.write.actions.terrain_z() {
            return Some("skipped_terrain_z_disabled");
        }
        if !input.write.actions.water_delete() {
            return Some("skipped_water_delete_disabled");
        }
        Some(match input.write.plan {
            WritePlanEvidence::NotPlanned => "would_delete_water_crossing",
            WritePlanEvidence::Unchanged => "skipped_write_plan_unchanged",
            WritePlanEvidence::Adjusted
            | WritePlanEvidence::WaterDeleted
            | WritePlanEvidence::Deleted
            | WritePlanEvidence::Moved
            | WritePlanEvidence::Oriented => unreachable!("handled before water status"),
        })
    })
}

fn orientation_write_status(input: &WriteStatusInput) -> Option<&'static str> {
    let MeshResolutionStatus::Resolved = input.mesh_status else {
        return None;
    };
    input.orientation_angle_degrees.and_then(|angle| {
        if angle <= input.orientation_epsilon_degrees {
            return None;
        }
        if !input.write.actions.orient() {
            return Some("skipped_orient_disabled");
        }
        Some(match input.write.plan {
            WritePlanEvidence::NotPlanned => "would_orient_to_terrain",
            WritePlanEvidence::Unchanged => "skipped_write_plan_unchanged",
            WritePlanEvidence::Adjusted
            | WritePlanEvidence::WaterDeleted
            | WritePlanEvidence::Deleted
            | WritePlanEvidence::Moved
            | WritePlanEvidence::Oriented => unreachable!("handled before orientation status"),
        })
    })
}

fn mesh_resolution_write_status(input: &WriteStatusInput) -> &'static str {
    match input.mesh_status {
        MeshResolutionStatus::UnresolvedStatic => "skipped_unresolved_static",
        MeshResolutionStatus::MissingContact => "skipped_missing_mesh_contact",
        MeshResolutionStatus::Resolved => input.contact_delta.map_or(
            "skipped_missing_contact_terrain",
            |_| "skipped_within_epsilon",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MeshResolutionStatus, WritePlanEvidence, WriteStatusEvidence, WriteStatusInput,
        write_status_label,
    };
    use crate::unclip::args::WriteActions;

    #[test]
    fn write_status_prefers_adjusted_plan_evidence() {
        let mut input = base_input(MeshResolutionStatus::UnresolvedStatic);
        input.write.plan = WritePlanEvidence::Adjusted;

        assert_eq!(write_status_label(&input), "adjusted");
    }

    #[test]
    fn write_status_reports_disabled_terrain_adjustment() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.contact_delta = Some(10.0);
        input.write.actions.disable_terrain_z();

        assert_eq!(write_status_label(&input), "skipped_terrain_z_disabled");
    }

    #[test]
    fn write_status_reports_disabled_static_actions() {
        let mut input = base_input(MeshResolutionStatus::UnresolvedStatic);
        input.write.actions.disable_static_delete();
        input.write.actions.disable_static_move();
        input.static_bounds_status = Some("static_bounds_fully_occluded");

        assert_eq!(write_status_label(&input), "skipped_static_delete_disabled");
        input.static_bounds_status = Some("static_bounds_relocatable");
        assert_eq!(write_status_label(&input), "skipped_static_move_disabled");
    }

    #[test]
    fn write_status_reports_dry_run_static_candidates() {
        let mut input = base_input(MeshResolutionStatus::UnresolvedStatic);
        input.static_bounds_status = Some("static_bounds_fully_occluded");

        assert_eq!(
            write_status_label(&input),
            "would_delete_static_bounds_occluded"
        );
        input.static_bounds_status = Some("static_bounds_relocatable");
        assert_eq!(
            write_status_label(&input),
            "would_move_static_bounds_occluded"
        );
        input.static_bounds_status = Some("static_clearance_deleted_no_relocation");
        assert_eq!(
            write_status_label(&input),
            "would_delete_static_bounds_no_relocation"
        );
    }

    #[test]
    fn write_status_reports_water_crossing_candidates() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.contact_delta = Some(10.0);
        input.water_crossing = Some(true);

        assert_eq!(write_status_label(&input), "would_delete_water_crossing");
        input.write.actions.disable_water_delete();
        assert_eq!(write_status_label(&input), "skipped_water_delete_disabled");
    }

    #[test]
    fn write_status_reports_terrain_disabled_for_water_crossing_prerequisite() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.contact_delta = Some(10.0);
        input.water_crossing = Some(true);
        input.write.actions.disable_terrain_z();

        assert_eq!(write_status_label(&input), "skipped_terrain_z_disabled");
    }

    #[test]
    fn write_status_checks_terrain_after_disabled_static_action() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.contact_delta = Some(10.0);
        input.static_bounds_status = Some("static_bounds_fully_occluded");
        input.write.actions.disable_static_delete();

        assert_eq!(write_status_label(&input), "would_adjust_terrain_z");
    }

    #[test]
    fn write_status_reports_write_plan_terrain_mismatch() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.contact_delta = Some(10.0);
        input.write.plan = WritePlanEvidence::Unchanged;

        assert_eq!(write_status_label(&input), "skipped_write_plan_unchanged");
    }

    #[test]
    fn write_status_reports_orientation_candidates() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.orientation_angle_degrees = Some(12.0);

        assert_eq!(write_status_label(&input), "would_orient_to_terrain");
        input.write.actions.disable_orient();
        assert_eq!(write_status_label(&input), "skipped_orient_disabled");
    }

    #[test]
    fn write_status_prefers_oriented_plan_evidence() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.orientation_angle_degrees = Some(12.0);
        input.write.plan = WritePlanEvidence::Oriented;

        assert_eq!(write_status_label(&input), "oriented_to_terrain");
    }

    #[test]
    fn write_status_prefers_water_deleted_plan_evidence() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.water_crossing = Some(true);
        input.write.plan = WritePlanEvidence::WaterDeleted;

        assert_eq!(write_status_label(&input), "deleted_water_crossing");
    }

    const fn test_write_actions() -> WriteActions {
        WriteActions::all()
    }

    fn base_input(mesh_status: MeshResolutionStatus) -> WriteStatusInput {
        WriteStatusInput {
            reference_deleted: false,
            mesh_status,
            contact_delta: None,
            orientation_angle_degrees: None,
            static_bounds_status: None,
            water_crossing: None,
            write: WriteStatusEvidence {
                plan: WritePlanEvidence::NotPlanned,
                actions: test_write_actions(),
            },
            contact_epsilon: 0.5,
            orientation_epsilon_degrees: 1.0,
        }
    }
}
