use std::{collections::BTreeMap, io, io::Write};

use serde::Serialize;

use super::{
    contact_baseline::{ContactBaselineDiagnostic, ContactBaselineIndex},
    model::{
        ReferenceInspection, TerrainInspectionReport, UnclipPolicySummary, UnclipReportContext,
        UnclipSummary,
    },
    write_plan::{
        WriteAdjustment, WriteOrientation, WriteReport, WriteStaticBoundsDeletion,
        WriteStaticBoundsMove, WriteSummary,
    },
};

#[derive(Clone, Default)]
struct BaselineActionCounts {
    adjusted: usize,
    deleted: usize,
    moved: usize,
    oriented: usize,
    adjustment_deltas: Vec<f32>,
}

impl BaselineActionCounts {
    const fn total(&self) -> usize {
        self.adjusted + self.deleted + self.moved + self.oriented
    }
}

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
    write_occluder_summary_text(stdout, context)?;
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
    let write = context.write.as_ref().map(WriteReport::summary);
    let report = StructuredSummaryReport {
        kind: "greenmote_unclip_terrain_inspection",
        target_plugin: &context.target_plugin,
        write_requested: context.write_requested(),
        policy: context.policy(),
        write: write.as_ref(),
        missing_active_terrain_cells: context.missing_active_terrain_cells(),
        static_occluders: &context.static_occluder_report,
        summary: context.summary(inspection),
    };
    write_json(stdout, &report)?;
    writeln!(stdout)
}

pub(crate) fn write_contact_baseline_diagnostics(
    stdout: &mut dyn Write,
    baselines: &ContactBaselineIndex,
    write: Option<&WriteReport>,
) -> io::Result<()> {
    let mut diagnostics = baselines.diagnostics();
    if diagnostics.is_empty() {
        writeln!(stdout, "Contact baselines:")?;
        return writeln!(stdout, "  calibrated statics: 0");
    }

    let actions = write.map_or_else(BTreeMap::new, baseline_action_counts);
    diagnostics.sort_by(|left, right| {
        let left_count = actions.get(&left.id).map_or(0, BaselineActionCounts::total);
        let right_count = actions
            .get(&right.id)
            .map_or(0, BaselineActionCounts::total);
        right_count
            .cmp(&left_count)
            .then_with(|| left.id.cmp(&right.id))
    });

    writeln!(stdout, "Contact baselines:")?;
    writeln!(
        stdout,
        "  calibrated statics: {} (baseline_delta = contact_z - terrain_z_at_origin)",
        diagnostics.len()
    )?;
    for diagnostic in diagnostics {
        write_contact_baseline_line(stdout, &diagnostic, actions.get(&diagnostic.id))?;
    }
    Ok(())
}

fn write_contact_baseline_line(
    stdout: &mut dyn Write,
    diagnostic: &ContactBaselineDiagnostic,
    actions: Option<&BaselineActionCounts>,
) -> io::Result<()> {
    let baseline = diagnostic.baseline;
    let actions = actions.cloned().unwrap_or_default();
    let adjustment_delta_summary = adjustment_delta_summary_text(&actions.adjustment_deltas);
    writeln!(
        stdout,
        "  {} mesh=\"{}\": samples={} baseline_median={:.3} min={:.3} p05={:.3} p95={:.3} max={:.3} adjusted={} adjustment_delta={} deleted={} moved={} oriented={}",
        diagnostic.id,
        diagnostic.mesh,
        baseline.samples,
        baseline.delta,
        baseline.min_delta,
        baseline.p05_delta,
        baseline.p95_delta,
        baseline.max_delta,
        actions.adjusted,
        adjustment_delta_summary,
        actions.deleted,
        actions.moved,
        actions.oriented,
    )
}

fn adjustment_delta_summary_text(deltas: &[f32]) -> String {
    if deltas.is_empty() {
        return "none".to_owned();
    }
    let mut deltas = deltas.to_vec();
    deltas.sort_by(f32::total_cmp);
    format!(
        "min={:.3},median={:.3},max={:.3}",
        deltas[0],
        median_sorted(&deltas),
        deltas[deltas.len() - 1]
    )
}

fn median_sorted(values: &[f32]) -> f32 {
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) * 0.5
    } else {
        values[middle]
    }
}

fn baseline_action_counts(write: &WriteReport) -> BTreeMap<String, BaselineActionCounts> {
    let mut counts = BTreeMap::<String, BaselineActionCounts>::new();
    for adjustment in &write.adjustments {
        let count = counts.entry(adjustment.id.to_lowercase()).or_default();
        count.adjusted += 1;
        count.adjustment_deltas.push(adjustment.applied_delta);
    }
    for deletion in &write.deletions {
        counts
            .entry(deletion.id.to_lowercase())
            .or_default()
            .deleted += 1;
    }
    for move_ in &write.moves {
        counts.entry(move_.id.to_lowercase()).or_default().moved += 1;
    }
    for orientation in &write.orientations {
        counts
            .entry(orientation.id.to_lowercase())
            .or_default()
            .oriented += 1;
    }
    counts
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
        Some("inspect_only") => "inspect-only dry run",
        Some("no_refs_changed") | None => "no refs changed",
        Some(_) => "write skipped",
    }
}

fn write_static_bounds_summary_text(
    stdout: &mut dyn Write,
    summary: &UnclipSummary,
) -> io::Result<()> {
    writeln!(stdout, "Static occlusion and clearance:")?;
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

fn write_occluder_summary_text(
    stdout: &mut dyn Write,
    context: &UnclipReportContext,
) -> io::Result<()> {
    let occluders = &context.static_occluder_report;
    writeln!(stdout, "Static occluders:")?;
    writeln!(
        stdout,
        "  active refs scanned: {}",
        occluders.active_refs_scanned
    )?;
    writeln!(
        stdout,
        "  target refs excluded: {}",
        occluders.target_refs_excluded
    )?;
    writeln!(stdout, "  regex excluded: {}", occluders.regex_excluded)?;
    writeln!(
        stdout,
        "  unresolved static: {}",
        occluders.unresolved_static
    )?;
    writeln!(stdout, "  missing bounds: {}", occluders.missing_bounds)?;
    writeln!(stdout, "  resolved bounds: {}", occluders.resolved_bounds)?;
    writeln!(
        stdout,
        "  huge footprint: {} over {:.0} units",
        occluders.huge_footprint, occluders.huge_footprint_side_threshold
    )?;
    if occluders.huge_footprint > 0 {
        writeln!(
            stdout,
            "  warning: huge occluder bounds can cause false static-bounds occlusion; consider --exclude-occluder-id"
        )?;
    }
    Ok(())
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
        if let Some(occluder_id) = &occlusion.occluder_id {
            writeln!(
                stdout,
                "    occluder: id={} cell={:?} ref={:?} intersection_volume={}",
                occluder_id,
                occlusion.occluder_cell,
                occlusion.occluder_reference_key,
                optional_f32(occlusion.intersection_volume)
            )?;
        }
        if let Some(bounds) = &occlusion.target_bounds {
            writeln!(
                stdout,
                "    target bounds: min={:?} max={:?}",
                bounds.min, bounds.max
            )?;
        }
        if let Some(bounds) = &occlusion.occluder_bounds {
            writeln!(
                stdout,
                "    occluder bounds: min={:?} max={:?}",
                bounds.min, bounds.max
            )?;
        }
    }
    Ok(())
}

fn write_adjustment_text(stdout: &mut dyn Write, adjustment: &WriteAdjustment) -> io::Result<()> {
    writeln!(
        stdout,
        "WRITE CELL {:?} REF {:?} {} old_z={:.3} new_z={:.3} applied_delta={:.3} {}={:?} terrain_z={:.3}",
        adjustment.cell,
        adjustment.reference_key,
        adjustment.id,
        adjustment.old_z,
        adjustment.new_z,
        adjustment.applied_delta,
        adjustment.sample_kind,
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
        "MOVE_STATIC_BOUNDS CELL {:?} REF {:?} {} reason={} ratio={:.3} old_position={:?} new_position={:?} occluder={} occluder_cell={:?} occluder_ref={:?}",
        move_.cell,
        move_.reference_key,
        move_.id,
        move_.block_reason,
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
        "ORIENT CELL {:?} REF {:?} {} angle={:.3} old_rotation={:?} new_rotation={:?} terrain_normal={:?} {}={:?}",
        orientation.cell,
        orientation.reference_key,
        orientation.id,
        orientation.angle_degrees,
        orientation.old_rotation,
        orientation.new_rotation,
        orientation.terrain_normal,
        orientation.sample_kind,
        orientation.sample_position
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
    write: Option<&'a WriteSummary>,
    missing_active_terrain_cells: Vec<[i32; 2]>,
    static_occluders: &'a super::static_occluders::StaticOccluderBuildReport,
    summary: UnclipSummary,
}

#[cfg(test)]
mod tests {
    use super::{write_structured_summary, write_summary_text};
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
    fn structured_summary_uses_compact_write_summary() {
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
        let mut output = Vec::new();

        write_structured_summary(&mut output, &context, &TerrainInspectionReport::default())
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\"adjusted_refs\":1"));
        assert!(!output.contains("\"adjustments\""));
        assert!(!output.contains("\"old_z\""));
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
            sample_kind: "contact",
            contact_position: [0.0, 0.0, 7.0],
            terrain_z: 9.0,
        }
    }
}
