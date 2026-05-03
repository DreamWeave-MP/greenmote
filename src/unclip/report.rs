use std::{io, io::Write};

use serde::Serialize;

use super::{
    app::UnclipReportContext,
    model::{ReferenceInspection, TerrainInspectionReport, UnclipSummary},
    write_plan::{
        WriteAdjustment, WriteReport, WriteStaticBoundsDeletion, WriteStaticBoundsMove,
        WriteSummary,
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
    write_terrain_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_reference_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_mesh_contact_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_static_bounds_summary_text(stdout, &summary)?;
    writeln!(stdout)?;
    write_threshold_summary_text(stdout, &summary)
}

pub(crate) fn write_instance_header(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
) -> io::Result<()> {
    writeln!(stdout, "Unclip reference diagnostics")?;
    writeln!(stdout, "Target plugin: {}", context.target_plugin)?;
    if let Some(write) = &context.write
        && (!write.adjustments.is_empty() || !write.deletions.is_empty() || !write.moves.is_empty())
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
    let header_write = context.write.as_ref().map(WriteReport::summary);
    let header = StructuredHeader {
        r#type: "header",
        kind: "greenmote_unclip_terrain_inspection",
        target_plugin: &context.target_plugin,
        write: header_write.as_ref(),
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
    let summary = StructuredSummaryRecord {
        r#type: "summary",
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
                "No plugin written: no refs changed at {}",
                write.destination_plugin
            )?;
        }
        writeln!(stdout, "Adjusted refs: {}", write.adjusted_refs)?;
        writeln!(stdout, "Deleted refs: {}", write.deleted_refs)?;
        writeln!(stdout, "Moved refs: {}", write.moved_refs)?;
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
    writeln!(stdout, "  total: {}", summary.refs)?;
    writeln!(stdout, "  inspected: {}", summary.refs_actionable)?;
    writeln!(stdout, "  deleted/skipped: {}", summary.refs_deleted)?;
    writeln!(
        stdout,
        "  with origin terrain: {}",
        summary.refs_with_terrain
    )?;
    writeln!(
        stdout,
        "  missing origin terrain: {}",
        summary.refs_missing_terrain
    )?;
    writeln!(
        stdout,
        "  origin above terrain: {}",
        summary.refs_origin_above_terrain
    )?;
    writeln!(
        stdout,
        "  origin below terrain: {}",
        summary.refs_origin_below_terrain
    )
}

fn write_mesh_contact_summary_text(
    stdout: &mut dyn Write,
    summary: &UnclipSummary,
) -> io::Result<()> {
    writeln!(stdout, "Mesh contacts:")?;
    writeln!(stdout, "  resolved: {}", summary.refs_with_mesh_contact)?;
    writeln!(
        stdout,
        "  unresolved static: {}",
        summary.refs_without_resolved_static
    )?;
    writeln!(
        stdout,
        "  missing contact: {}",
        summary.refs_missing_mesh_contact
    )?;
    writeln!(
        stdout,
        "  contact above terrain: {}",
        summary.refs_mesh_contact_above_terrain
    )?;
    writeln!(
        stdout,
        "  contact below terrain: {}",
        summary.refs_mesh_contact_below_terrain
    )?;
    writeln!(
        stdout,
        "  contact missing terrain: {}",
        summary.refs_mesh_contact_missing_terrain
    )
}

fn write_threshold_summary_text(stdout: &mut dyn Write, summary: &UnclipSummary) -> io::Result<()> {
    writeln!(stdout, "Thresholds:")?;
    writeln!(
        stdout,
        "  origin terrain epsilon: {:.3}",
        summary.origin_terrain_epsilon
    )?;
    writeln!(
        stdout,
        "  mesh contact terrain epsilon: {:.3}",
        summary.mesh_contact_terrain_epsilon
    )
}

fn write_static_bounds_summary_text(
    stdout: &mut dyn Write,
    summary: &UnclipSummary,
) -> io::Result<()> {
    writeln!(stdout, "Static bounds occlusion:")?;
    writeln!(
        stdout,
        "  occluded: {}",
        summary.refs_static_bounds_occluded
    )?;
    writeln!(
        stdout,
        "  fully occluded: {}",
        summary.refs_static_bounds_fully_occluded
    )?;
    writeln!(
        stdout,
        "  relocatable: {}",
        summary.refs_static_bounds_lightly_occluded
    )?;
    writeln!(stdout, "  blocked: {}", summary.refs_static_bounds_blocked)
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
            "  contact: position={:?} terrain_z={} delta={} classification={}",
            contact.position,
            optional_f32(contact.terrain_z),
            optional_f32(contact.delta),
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

#[derive(Serialize)]
struct StructuredSummaryReport<'a> {
    kind: &'static str,
    target_plugin: &'a str,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    write: Option<&'a WriteSummary>,
    missing_active_terrain_cells: Vec<[i32; 2]>,
}

#[derive(Serialize)]
struct StructuredSummaryRecord {
    r#type: &'static str,
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

#[cfg(test)]
mod tests {
    use super::write_summary_text;
    use crate::unclip::{
        app::UnclipReportContext,
        model::TerrainInspectionReport,
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
        assert!(!without_adjustments.contains("WRITE CELL"));
    }

    fn write_adjustment() -> WriteAdjustment {
        WriteAdjustment {
            cell: [1, 2],
            reference_key: [3, 4],
            id: "grass".to_owned(),
            old_z: 10.0,
            new_z: 12.0,
            applied_delta: 2.0,
            contact_position: [0.0, 0.0, 7.0],
            terrain_z: 9.0,
        }
    }
}
