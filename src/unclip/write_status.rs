use super::{args::WriteActions, cells::CellCoord, write_plan::WriteStatusIndex};

pub(crate) struct WriteStatusInput {
    pub(crate) reference_deleted: bool,
    pub(crate) mesh_status: MeshResolutionStatus,
    pub(crate) contact_delta: Option<f32>,
    pub(crate) static_bounds_status: Option<&'static str>,
    pub(crate) write: WriteStatusEvidence,
    pub(crate) contact_epsilon: f32,
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
    Deleted,
    Moved,
}

pub(crate) fn write_plan_evidence(
    write: Option<&WriteStatusIndex>,
    cell: CellCoord,
    key: (u32, u32),
) -> WritePlanEvidence {
    let Some(write) = write else {
        return WritePlanEvidence::NotPlanned;
    };
    if write.is_deleted(cell, key) {
        WritePlanEvidence::Deleted
    } else if write.is_moved(cell, key) {
        WritePlanEvidence::Moved
    } else if write.is_adjusted(cell, key) {
        WritePlanEvidence::Adjusted
    } else {
        WritePlanEvidence::Unchanged
    }
}

pub(crate) fn write_status_label(input: &WriteStatusInput) -> &'static str {
    match input.write.plan {
        WritePlanEvidence::Deleted => return "deleted_static_bounds_occluded",
        WritePlanEvidence::Moved => return "moved_static_bounds_occluded",
        WritePlanEvidence::Adjusted => return "adjusted",
        WritePlanEvidence::NotPlanned | WritePlanEvidence::Unchanged => {}
    }
    if input.reference_deleted {
        return "skipped_deleted_ref";
    }
    if let Some(occlusion) = input.static_bounds_status {
        match occlusion {
            "static_bounds_fully_occluded" if input.write.actions.static_delete => {
                if matches!(input.write.plan, WritePlanEvidence::NotPlanned) {
                    return "would_delete_static_bounds_occluded";
                }
            }
            "static_bounds_relocatable" if input.write.actions.static_move => {
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
    if let Some(occlusion) = input.static_bounds_status {
        return match occlusion {
            "static_bounds_fully_occluded" if !input.write.actions.static_delete => {
                "skipped_static_delete_disabled"
            }
            "static_bounds_relocatable" | "static_bounds_blocked"
                if !input.write.actions.static_move =>
            {
                "skipped_static_move_disabled"
            }
            "static_bounds_blocked" if input.write.actions.static_move => {
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
        if delta.abs() > input.contact_epsilon && input.write.actions.terrain_z {
            Some("adjusted_or_adjustable")
        } else if delta.abs() > input.contact_epsilon {
            Some("skipped_terrain_z_disabled")
        } else {
            None
        }
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
        input.write.actions.terrain_z = false;

        assert_eq!(write_status_label(&input), "skipped_terrain_z_disabled");
    }

    #[test]
    fn write_status_reports_disabled_static_actions() {
        let mut input = base_input(MeshResolutionStatus::UnresolvedStatic);
        input.write.actions.static_delete = false;
        input.write.actions.static_move = false;
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
    }

    #[test]
    fn write_status_checks_terrain_after_disabled_static_action() {
        let mut input = base_input(MeshResolutionStatus::Resolved);
        input.contact_delta = Some(10.0);
        input.static_bounds_status = Some("static_bounds_fully_occluded");
        input.write.actions.static_delete = false;

        assert_eq!(write_status_label(&input), "adjusted_or_adjustable");
    }

    const fn test_write_actions() -> WriteActions {
        WriteActions {
            terrain_z: true,
            static_delete: true,
            static_move: true,
        }
    }

    fn base_input(mesh_status: MeshResolutionStatus) -> WriteStatusInput {
        WriteStatusInput {
            reference_deleted: false,
            mesh_status,
            contact_delta: None,
            static_bounds_status: None,
            write: WriteStatusEvidence {
                plan: WritePlanEvidence::NotPlanned,
                actions: test_write_actions(),
            },
            contact_epsilon: 0.5,
        }
    }
}
