use std::{io, io::Write};

use serde::Serialize;

use super::{
    model::{
        ReferenceInspection, TerrainInspectionReport, UnclipPolicySummary, UnclipReportContext,
        UnclipSummary,
    },
    write_plan::{
        WriteAdjustment, WriteOrientation, WriteReport, WriteStaticBoundsDeletion,
        WriteStaticBoundsMove, WriteSummary,
    },
};

pub(crate) fn write_summary_text(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
    include_adjustments: bool,
) -> io::Result<()> {
    let summary = context.summary(inspection);
    writeln!(stdout, "Unclip inspection summary")?;
    writeln!(stdout, "Target plugin: {}", context.target_plugin)?;
    write_write_summary_text(stdout, context.write.as_ref(), include_adjustments)?;
    writeln!(stdout)?;
    write_policy_summary_text(stdout, context.write_requested(), context.policy())?;
    writeln!(stdout)?;
    write_terrain_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_reference_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_mesh_contact_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_static_bounds_summary_text(stdout, &summary)
}

pub(crate) fn write_instance_header(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
) -> io::Result<()> {
    writeln!(stdout, "Unclip reference diagnostics")?;
    writeln!(stdout, "Target plugin: {}", context.target_plugin)?;
    if let Some(write) = &context.write
        && (!write.adjustments.is_empty()
            || !write.deletions.is_empty()
            || !write.moves.is_empty()
            || !write.orientations.is_empty())
    {
        writeln!(stdout)?;
        writeln!(stdout, "Write changes")?;
        write_change_lines(stdout, write)?;
    }
    writeln!(stdout)
}

pub(crate) fn write_structured_summary(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
) -> io::Result<()> {
    let report = StructuredSummaryReport {
        kind: "greenmote_unclip_terrain_inspection",
        target_plugin: &context.target_plugin,
        write_requested: context.write_requested(),
        policy: context.policy(),
        write: context.write.as_ref(),
        missing_active_terrain_cells: context.missing_active_terrain_cells(),
        summary: context.summary(inspection),
    };
    write_json(stdout, &report)?;
    writeln!(stdout)
}

pub(crate) fn write_structured_header(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
) -> io::Result<()> {
    let header = StructuredHeader {
        r#type: "header",
        kind: "greenmote_unclip_terrain_inspection",
        target_plugin: &context.target_plugin,
        write_requested: context.write_requested(),
        policy: context.policy(),
        missing_active_terrain_cells: context.missing_active_terrain_cells(),
    };
    write_json(stdout, &header)?;
    writeln!(stdout)
}

pub(crate) fn write_structured_write_records(
    stdout: &mut dyn Write,
    write: Option<&WriteReport>,
) -> io::Result<()> {
    if let Some(write) = write {
        for adjustment in &write.adjustments {
            let record = StructuredWriteAdjustmentRecord {
                r#type: "write_adjustment",
                adjustment,
            };
            write_json(stdout, &record)?;
            writeln!(stdout)?;
        }
        for deletion in &write.deletions {
            let record = StructuredWriteDeletionRecord {
                r#type: "write_static_bounds_deletion",
                deletion,
            };
            write_json(stdout, &record)?;
            writeln!(stdout)?;
        }
        for move_ in &write.moves {
            let record = StructuredWriteMoveRecord {
                r#type: "write_static_bounds_move",
                move_,
            };
            write_json(stdout, &record)?;
            writeln!(stdout)?;
        }
        for orientation in &write.orientations {
            let record = StructuredWriteOrientationRecord {
                r#type: "write_orientation",
                orientation,
            };
            write_json(stdout, &record)?;
            writeln!(stdout)?;
        }
    }
    Ok(())
}

pub(crate) fn write_structured_reference_record(
    stdout: &mut dyn Write,
    reference: &ReferenceInspection,
) -> io::Result<()> {
    let record = StructuredReferenceRecord {
        r#type: "ref",
        reference,
    };
    write_json(stdout, &record)?;
    writeln!(stdout)
}

pub(crate) fn write_structured_summary_record(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
) -> io::Result<()> {
    let write = context.write.as_ref().map(WriteReport::summary);
    let summary = StructuredSummaryRecord {
        r#type: "summary",
        write_requested: context.write_requested(),
        policy: context.policy(),
        write: write.as_ref(),
        summary: context.summary(inspection),
    };
    write_json(stdout, &summary)?;
    writeln!(stdout)
}

fn write_write_summary_text(
    stdout: &mut dyn Write,
    write: Option<&WriteReport>,
    include_adjustments: bool,
) -> io::Result<()> {
    if let Some(write) = write {
        if write.written {
            writeln!(stdout, "Written plugin: {}", write.destination_plugin)?;
            if let Some(backup) = &write.backup_plugin {
                writeln!(stdout, "Backup plugin: {backup}")?;
            }
        } else {
            writeln!(
                stdout,
                "No plugin written: {} at {}",
                text_no_write_reason(write.no_write_reason),
                write.destination_plugin
            )?;
        }
        writeln!(stdout, "Adjusted refs: {}", write.adjusted_refs)?;
        writeln!(stdout, "Deleted refs: {}", write.deleted_refs)?;
        writeln!(stdout, "Moved refs: {}", write.moved_refs)?;
        writeln!(stdout, "Oriented refs: {}", write.oriented_refs)?;
        if include_adjustments {
            write_change_lines(stdout, write)?;
        }
    }
    Ok(())
}

fn write_change_lines(stdout: &mut dyn Write, write: &WriteReport) -> io::Result<()> {
    for adjustment in &write.adjustments {
        write_adjustment_text(stdout, adjustment)?;
    }
    for deletion in &write.deletions {
        write_static_bounds_deletion_text(stdout, deletion)?;
    }
    for move_ in &write.moves {
        write_static_bounds_move_text(stdout, move_)?;
    }
    for orientation in &write.orientations {
        write_orientation_text(stdout, orientation)?;
    }
    Ok(())
}

fn write_terrain_summary_text(stdout: &mut dyn Write, summary: &UnclipSummary) -> io::Result<()> {
    writeln!(stdout, "Terrain cells:")?;
    writeln!(
        stdout,
        "  target exterior cells: {}",
        summary.target_exterior_cells
    )?;
    writeln!(stdout, "  active 3x3 cells: {}", summary.active_cells)?;
    writeln!(
        stdout,
        "  loaded terrain cells total: {}",
        summary.loaded_terrain_cells_total
    )?;
    writeln!(
        stdout,
        "  active terrain cells loaded: {}",
        summary.active_terrain_cells_loaded
    )?;
    writeln!(
        stdout,
        "  active terrain cells missing: {}",
        summary.active_terrain_cells_missing
    )
}

fn write_reference_summary_text(stdout: &mut dyn Write, summary: &UnclipSummary) -> io::Result<()> {
    writeln!(stdout, "References:")?;
    writeln!(
        stdout,
        "  target exterior refs total: {}",
        summary.target_refs_total
    )?;
    writeln!(
        stdout,
        "  matching grass id filter: {}",
        summary.target_refs_matching_filter
    )?;
    writeln!(
        stdout,
        "  filtered out: {}",
        summary.target_refs_filtered_out
    )?;
    writeln!(
        stdout,
        "  matching actionable: {}",
        summary.filtered_refs_actionable
    )?;
    writeln!(
        stdout,
        "  matching deleted/skipped: {}",
        summary.filtered_refs_deleted
    )?;
    writeln!(
        stdout,
        "  matching with origin terrain: {}",
        summary.filtered_refs_with_origin_terrain
    )?;
    writeln!(
        stdout,
        "  matching missing origin terrain: {}",
        summary.filtered_refs_missing_origin_terrain
    )?;
    writeln!(
        stdout,
        "  matching origin above terrain: {}",
        summary.filtered_refs_origin_above_terrain
    )?;
    writeln!(
        stdout,
        "  matching origin below terrain: {}",
        summary.filtered_refs_origin_below_terrain
    )
}

fn write_mesh_contact_summary_text(
    stdout: &mut dyn Write,
    summary: &UnclipSummary,
) -> io::Result<()> {
    writeln!(stdout, "Mesh contacts:")?;
    writeln!(
        stdout,
        "  matching resolved: {}",
        summary.filtered_refs_with_mesh_contact
    )?;
    writeln!(
        stdout,
        "  matching unresolved static: {}",
        summary.filtered_refs_without_resolved_static
    )?;
    writeln!(
        stdout,
        "  matching missing contact: {}",
        summary.filtered_refs_missing_mesh_contact
    )?;
    writeln!(
        stdout,
        "  matching contact above terrain: {}",
        summary.filtered_refs_mesh_contact_above_terrain
    )?;
    writeln!(
        stdout,
        "  matching contact below terrain: {}",
        summary.filtered_refs_mesh_contact_below_terrain
    )?;
    writeln!(
        stdout,
        "  matching contact missing terrain: {}",
        summary.filtered_refs_mesh_contact_missing_terrain
    )
}

fn write_policy_summary_text(
    stdout: &mut dyn Write,
    write_requested: bool,
    policy: &UnclipPolicySummary,
) -> io::Result<()> {
    writeln!(stdout, "Policy:")?;
    writeln!(
        stdout,
        "  write mode: {}",
        if write_requested {
            "write"
        } else {
            "inspect-only"
        }
    )?;
    let actions = if policy.write_actions.is_empty() {
        "none".to_owned()
    } else {
        policy.write_actions.join(", ")
    };
    writeln!(stdout, "  write actions: {actions}")?;
    if policy.has_target_filter() {
        writeln!(
            stdout,
            "  include grass ids: {}",
            pattern_list(&policy.include_grass_ids)
        )?;
        writeln!(
            stdout,
            "  exclude grass ids: {}",
            pattern_list(&policy.exclude_grass_ids)
        )?;
    } else {
        writeln!(stdout, "  grass id filter: none")?;
    }
    if policy.has_occluder_filter() {
        writeln!(
            stdout,
            "  include occluder ids: {}",
            pattern_list(&policy.include_occluder_ids)
        )?;
        writeln!(
            stdout,
            "  exclude occluder ids: {}",
            pattern_list(&policy.exclude_occluder_ids)
        )?;
    } else {
        writeln!(stdout, "  occluder id filter: none")?;
    }
    writeln!(
        stdout,
        "  origin terrain epsilon: {:.3}",
        policy.origin_terrain_epsilon
    )?;
    writeln!(
        stdout,
        "  mesh contact terrain epsilon: {:.3}",
        policy.mesh_contact_terrain_epsilon
    )?;
    writeln!(
        stdout,
        "  orientation epsilon: {:.3} degrees",
        policy.orientation_epsilon_degrees
    )?;
    writeln!(stdout, "  relocation step: {:.3}", policy.relocation_step)?;
    writeln!(stdout, "  relocation steps: {}", policy.relocation_steps)
}

fn pattern_list(patterns: &[String]) -> String {
    if patterns.is_empty() {
        "none".to_owned()
    } else {
        patterns.join(", ")
    }
}

fn text_no_write_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("all_write_actions_disabled") => "all write actions disabled",
        Some("no_refs_changed") | None => "no refs changed",
        Some(_) => "write skipped",
    }
}

fn write_static_bounds_summary_text(
    stdout: &mut dyn Write,
    summary: &UnclipSummary,
) -> io::Result<()> {
    writeln!(stdout, "Static bounds occlusion:")?;
    writeln!(
        stdout,
        "  matching occluded: {}",
        summary.filtered_refs_static_bounds_occluded
    )?;
    writeln!(
        stdout,
        "  matching fully occluded: {}",
        summary.filtered_refs_static_bounds_fully_occluded
    )?;
    writeln!(
        stdout,
        "  matching relocatable: {}",
        summary.filtered_refs_static_bounds_relocatable
    )?;
    writeln!(
        stdout,
        "  matching blocked: {}",
        summary.filtered_refs_static_bounds_blocked
    )
}

pub(crate) fn write_reference_text(
    stdout: &mut dyn Write,
    reference: &ReferenceInspection,
) -> io::Result<()> {
    writeln!(
        stdout,
        "CELL {:?} REF {:?} {}",
        reference.cell, reference.reference_key, reference.id
    )?;
    writeln!(stdout, "  static: {}", reference.static_resolution)?;
    writeln!(stdout, "  deleted: {}", reference.deleted)?;
    writeln!(stdout, "  write status: {}", reference.write_status)?;
    if let Some(static_mesh) = &reference.static_mesh {
        writeln!(stdout, "  static id: {}", static_mesh.id)?;
        writeln!(stdout, "  mesh: {}", static_mesh.mesh)?;
    }
    if let Some(error) = &reference.mesh_contact_error {
        writeln!(stdout, "  mesh contact error: {error}")?;
    }
    writeln!(stdout, "  mesh contact: {}", reference.mesh_contact_status)?;
    writeln!(
        stdout,
        "  origin: position={:?} terrain_z={} delta={} classification={}",
        reference.origin.position,
        optional_f32(reference.origin.terrain_z),
        optional_f32(reference.origin.delta),
        reference.origin.classification
    )?;
    if let Some(contact) = &reference.mesh_contact {
        writeln!(
            stdout,
            "  contact: position={:?} terrain_z={} terrain_normal={} delta={} orientation_angle={} classification={}",
            contact.position,
            optional_f32(contact.terrain_z),
            optional_vec3(contact.terrain_normal),
            optional_f32(contact.delta),
            optional_f32(contact.orientation_angle_degrees),
            contact.classification
        )?;
    }
    if let Some(occlusion) = &reference.static_bounds_occlusion {
        writeln!(
            stdout,
            "  static bounds occlusion: status={} ratio={:.3}",
            occlusion.status, occlusion.ratio
        )?;
    }
    Ok(())
}

fn write_adjustment_text(stdout: &mut dyn Write, adjustment: &WriteAdjustment) -> io::Result<()> {
    writeln!(
        stdout,
        "WRITE CELL {:?} REF {:?} {} old_z={:.3} new_z={:.3} applied_delta={:.3} contact={:?} terrain_z={:.3}",
        adjustment.cell,
        adjustment.reference_key,
        adjustment.id,
        adjustment.old_z,
        adjustment.new_z,
        adjustment.applied_delta,
        adjustment.contact_position,
        adjustment.terrain_z
    )
}

fn write_static_bounds_deletion_text(
    stdout: &mut dyn Write,
    deletion: &WriteStaticBoundsDeletion,
) -> io::Result<()> {
    writeln!(
        stdout,
        "DELETE_STATIC_BOUNDS CELL {:?} REF {:?} {} ratio={:.3} occluder={} occluder_cell={:?} occluder_ref={:?}",
        deletion.cell,
        deletion.reference_key,
        deletion.id,
        deletion.occlusion_ratio,
        deletion.occluder_id,
        deletion.occluder_cell,
        deletion.occluder_reference_key
    )
}

fn write_static_bounds_move_text(
    stdout: &mut dyn Write,
    move_: &WriteStaticBoundsMove,
) -> io::Result<()> {
    writeln!(
        stdout,
        "MOVE_STATIC_BOUNDS CELL {:?} REF {:?} {} ratio={:.3} old_position={:?} new_position={:?} occluder={} occluder_cell={:?} occluder_ref={:?}",
        move_.cell,
        move_.reference_key,
        move_.id,
        move_.occlusion_ratio,
        move_.old_position,
        move_.new_position,
        move_.occluder_id,
        move_.occluder_cell,
        move_.occluder_reference_key
    )
}

fn write_orientation_text(
    stdout: &mut dyn Write,
    orientation: &WriteOrientation,
) -> io::Result<()> {
    writeln!(
        stdout,
        "ORIENT CELL {:?} REF {:?} {} angle={:.3} old_rotation={:?} new_rotation={:?} terrain_normal={:?} contact={:?}",
        orientation.cell,
        orientation.reference_key,
        orientation.id,
        orientation.angle_degrees,
        orientation.old_rotation,
        orientation.new_rotation,
        orientation.terrain_normal,
        orientation.contact_position
    )
}

fn write_json(stdout: &mut dyn Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(stdout, value).map_err(|error| {
        if let Some(kind) = error.io_error_kind() {
            io::Error::new(kind, error)
        } else {
            io::Error::other(error.to_string())
        }
    })
}

fn optional_f32(value: Option<f32>) -> String {
    value.map_or_else(|| "missing".to_owned(), |value| format!("{value:.3}"))
}

fn optional_vec3(value: Option<[f32; 3]>) -> String {
    value.map_or_else(
        || "missing".to_owned(),
        |value| format!("[{:.3}, {:.3}, {:.3}]", value[0], value[1], value[2]),
    )
}

#[derive(Serialize)]
struct StructuredSummaryReport<'a> {
    kind: &'static str,
    target_plugin: &'a str,
    write_requested: bool,
    policy: &'a UnclipPolicySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    write: Option<&'a WriteReport>,
    missing_active_terrain_cells: Vec<[i32; 2]>,
    summary: UnclipSummary,
}

#[derive(Serialize)]
struct StructuredHeader<'a> {
    r#type: &'static str,
    kind: &'static str,
    target_plugin: &'a str,
    write_requested: bool,
    policy: &'a UnclipPolicySummary,
    missing_active_terrain_cells: Vec<[i32; 2]>,
}

#[derive(Serialize)]
struct StructuredSummaryRecord<'a> {
    r#type: &'static str,
    write_requested: bool,
    policy: &'a UnclipPolicySummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    write: Option<&'a WriteSummary>,
    summary: UnclipSummary,
}

#[derive(Serialize)]
struct StructuredReferenceRecord<'a> {
    r#type: &'static str,
    #[serde(flatten)]
    reference: &'a ReferenceInspection,
}

#[derive(Serialize)]
struct StructuredWriteAdjustmentRecord<'a> {
    r#type: &'static str,
    #[serde(flatten)]
    adjustment: &'a WriteAdjustment,
}

#[derive(Serialize)]
struct StructuredWriteDeletionRecord<'a> {
    r#type: &'static str,
    #[serde(flatten)]
    deletion: &'a WriteStaticBoundsDeletion,
}

#[derive(Serialize)]
struct StructuredWriteMoveRecord<'a> {
    r#type: &'static str,
    #[serde(flatten)]
    move_: &'a WriteStaticBoundsMove,
}

#[derive(Serialize)]
struct StructuredWriteOrientationRecord<'a> {
    r#type: &'static str,
    #[serde(flatten)]
    orientation: &'a WriteOrientation,
}

#[cfg(test)]
mod tests {
    use super::{write_structured_header, write_structured_write_records, write_summary_text};
    use crate::unclip::{
        model::{TerrainInspectionReport, UnclipReportContext},
        write_plan::{WriteAdjustment, WritePlan, WriteReport},
    };

    #[test]
    fn write_summary_prints_adjustments_only_when_requested() {
        let mut context = UnclipReportContext::new_for_test("plugin.omwaddon");
        context.write = Some(WriteReport::not_written(
            std::path::Path::new("plugin.omwaddon"),
            WritePlan {
                adjusted_refs: 1,
                adjustments: vec![write_adjustment()],
                ..WritePlan::default()
            },
            "no_refs_changed",
        ));
        let mut with_adjustments = Vec::new();
        let mut without_adjustments = Vec::new();

        write_summary_text(
            &mut with_adjustments,
            &context,
            &TerrainInspectionReport::default(),
            true,
        )
        .unwrap();
        write_summary_text(
            &mut without_adjustments,
            &context,
            &TerrainInspectionReport::default(),
            false,
        )
        .unwrap();

        let with_adjustments = String::from_utf8(with_adjustments).unwrap();
        let without_adjustments = String::from_utf8(without_adjustments).unwrap();
        assert!(with_adjustments.contains("WRITE CELL"));
        assert!(with_adjustments.contains("Policy:"));
        assert!(with_adjustments.contains("write mode: inspect-only"));
        assert!(with_adjustments.contains("target exterior refs total:"));
        assert!(!without_adjustments.contains("WRITE CELL"));
    }

    #[test]
    fn write_summary_reports_no_write_reason() {
        let mut context = UnclipReportContext::new_for_test("plugin.omwaddon");
        context.write = Some(WriteReport::not_written(
            std::path::Path::new("plugin.omwaddon"),
            WritePlan::default(),
            "all_write_actions_disabled",
        ));
        let mut output = Vec::new();

        write_summary_text(
            &mut output,
            &context,
            &TerrainInspectionReport::default(),
            false,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("No plugin written: all write actions disabled at plugin.omwaddon")
        );
    }

    #[test]
    fn structured_header_includes_policy() {
        let context = UnclipReportContext::new_for_test("plugin.omwaddon");
        let mut output = Vec::new();

        write_structured_header(&mut output, &context).unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\"policy\":"));
        assert!(output.contains("\"write_requested\":false"));
        assert!(output.contains(
            "\"write_actions\":[\"terrain-z\",\"static-delete\",\"static-move\",\"orient\"]"
        ));
        assert!(output.contains("\"include_grass_ids\":[]"));
        assert!(output.contains("\"exclude_grass_ids\":[]"));
        assert!(output.contains("\"include_occluder_ids\":[]"));
        assert!(output.contains("\"exclude_occluder_ids\":[]"));
    }

    #[test]
    fn structured_write_records_use_sorted_write_report_order() {
        let report = WriteReport::not_written(
            std::path::Path::new("plugin.omwaddon"),
            WritePlan {
                adjusted_refs: 2,
                adjustments: vec![
                    write_adjustment_at([1, 0], [4, 0]),
                    write_adjustment_at([0, 0], [9, 0]),
                ],
                ..WritePlan::default()
            },
            "no_refs_changed",
        );
        let mut output = Vec::new();

        write_structured_write_records(&mut output, Some(&report)).unwrap();

        let output = String::from_utf8(output).unwrap();
        let first = output.find("\"cell\":[0,0]").unwrap();
        let second = output.find("\"cell\":[1,0]").unwrap();
        assert!(first < second);
    }

    fn write_adjustment() -> WriteAdjustment {
        write_adjustment_at([1, 2], [3, 4])
    }

    fn write_adjustment_at(cell: [i32; 2], reference_key: [u32; 2]) -> WriteAdjustment {
        WriteAdjustment {
            cell,
            reference_key,
            id: "grass".to_owned(),
            old_z: 10.0,
            new_z: 12.0,
            applied_delta: 2.0,
            contact_position: [0.0, 0.0, 7.0],
            terrain_z: 9.0,
        }
    }
}
