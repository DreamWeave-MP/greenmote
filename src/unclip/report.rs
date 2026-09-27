// SPDX-License-Identifier: GPL-3.0-only

//! Human-readable and JSON reports for an unclip run.

use std::{io, io::Write, path::PathBuf};

use serde::Serialize;

use super::{
    args::UnclipPolicy,
    decide::{RefVerdict, Verdict, VerdictCounts},
    patch::WriteOutcome,
    static_occluders::StaticOccluderBuildReport,
};

/// Everything the report needs, independent of the output format.
#[derive(Serialize)]
pub(crate) struct Report {
    pub(crate) target: PathBuf,
    pub(crate) mode: OutputSummary,
    pub(crate) policy: PolicySummary,
    pub(crate) cells: CellSummary,
    pub(crate) occluders: StaticOccluderBuildReport,
    pub(crate) counts: VerdictCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) write: Option<WriteOutcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refs: Option<Vec<RefVerdict>>,
}

#[derive(Serialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum OutputSummary {
    DryRun {
        would_write: PathBuf,
        in_place: bool,
    },
    Patch {
        path: PathBuf,
    },
    InPlace {
        path: PathBuf,
    },
}

#[derive(Serialize)]
pub(crate) struct PolicySummary {
    pub(crate) actions: Vec<&'static str>,
    pub(crate) float_tolerance: f32,
    pub(crate) max_sink: f32,
    pub(crate) sink: f32,
    pub(crate) orientation_epsilon_degrees: f32,
    pub(crate) relocation_step: f32,
    pub(crate) relocation_steps: u16,
    pub(crate) include_grass_ids: Vec<String>,
    pub(crate) exclude_grass_ids: Vec<String>,
    pub(crate) include_occluder_ids: Vec<String>,
    pub(crate) exclude_occluder_ids: Vec<String>,
    pub(crate) road_texture_paths: Vec<String>,
}

impl PolicySummary {
    pub(crate) fn from_policy(policy: &UnclipPolicy) -> Self {
        Self {
            actions: policy.actions.enabled_names(),
            float_tolerance: policy.float_tolerance,
            max_sink: policy.max_sink,
            sink: policy.sink,
            orientation_epsilon_degrees: policy.orientation_epsilon_degrees,
            relocation_step: policy.relocation.step,
            relocation_steps: policy.relocation.steps,
            include_grass_ids: policy.target_filter.include_ids().to_vec(),
            exclude_grass_ids: policy.target_filter.exclude_ids().to_vec(),
            include_occluder_ids: policy.occluder_filter.include_ids().to_vec(),
            exclude_occluder_ids: policy.occluder_filter.exclude_ids().to_vec(),
            road_texture_paths: policy.road_texture_filter.paths().to_vec(),
        }
    }
}

#[derive(Default, Serialize)]
pub(crate) struct CellSummary {
    pub(crate) target_exterior_refs: usize,
    pub(crate) target_cells: usize,
    pub(crate) active_cells: usize,
    pub(crate) terrain_cells_loaded: usize,
    pub(crate) terrain_cells_missing: Vec<[i32; 2]>,
}

/// Writes the report as pretty JSON.
///
/// # Errors
///
/// Returns output stream errors.
pub(crate) fn write_json(out: &mut dyn Write, report: &Report) -> io::Result<()> {
    serde_json::to_writer_pretty(&mut *out, report).map_err(io::Error::other)?;
    writeln!(out)
}

/// Writes the report as text.
///
/// # Errors
///
/// Returns output stream errors.
pub(crate) fn write_text(out: &mut dyn Write, report: &Report) -> io::Result<()> {
    write_header(out, report)?;
    write_world(out, report)?;
    write_counts(out, report)?;
    write_outcome(out, report)
}

fn write_header(out: &mut dyn Write, report: &Report) -> io::Result<()> {
    writeln!(out, "Unclip report for {}", report.target.display())?;
    match &report.mode {
        OutputSummary::DryRun {
            would_write,
            in_place,
        } => writeln!(
            out,
            "Mode: dry run (pass --write to {} {})",
            if *in_place { "rewrite" } else { "write" },
            would_write.display()
        )?,
        OutputSummary::Patch { path } => writeln!(out, "Mode: patch plugin {}", path.display())?,
        OutputSummary::InPlace { path } => writeln!(out, "Mode: in place {}", path.display())?,
    }
    writeln!(out, "Actions: {}", join(&report.policy.actions))?;
    writeln!(
        out,
        "Tolerances: float {} / max sink {} / sink to {} / tilt {} deg",
        report.policy.float_tolerance,
        report.policy.max_sink,
        report.policy.sink,
        report.policy.orientation_epsilon_degrees
    )?;
    writeln!(out)
}

fn write_world(out: &mut dyn Write, report: &Report) -> io::Result<()> {
    writeln!(
        out,
        "Cells: {} target, {} active, {} with terrain, {} missing terrain",
        report.cells.target_cells,
        report.cells.active_cells,
        report.cells.terrain_cells_loaded,
        report.cells.terrain_cells_missing.len()
    )?;
    if !report.cells.terrain_cells_missing.is_empty() {
        let shown = report
            .cells
            .terrain_cells_missing
            .iter()
            .take(12)
            .map(|cell| format!("({}, {})", cell[0], cell[1]))
            .collect::<Vec<_>>();
        writeln!(
            out,
            "  missing terrain: {}{}",
            shown.join(" "),
            if report.cells.terrain_cells_missing.len() > 12 {
                " ..."
            } else {
                ""
            }
        )?;
    }
    let occluders = &report.occluders;
    writeln!(
        out,
        "Occluders: {} statics ({} collision meshes, {} visual fallbacks, {} without collision), {} refs excluded as grass, {} excluded by filter, {} unresolved",
        occluders.resolved_bounds,
        occluders.collision_source,
        occluders.visible_fallback_source,
        occluders.no_collision,
        occluders.target_refs_excluded,
        occluders.regex_excluded,
        occluders.unresolved_static
    )?;
    if !occluders.missing_meshes.is_empty() {
        writeln!(
            out,
            "  {} occluder meshes could not be loaded (ignored)",
            occluders.missing_meshes.len()
        )?;
    }
    writeln!(out)
}

fn write_counts(out: &mut dyn Write, report: &Report) -> io::Result<()> {
    let counts = &report.counts;
    writeln!(
        out,
        "Refs: {} matched, {} fine, {} to fix, {} to delete, {} skipped",
        counts.total, counts.keep, counts.fix, counts.delete, counts.skip
    )?;
    for (label, count) in &counts.by_label {
        if *label != "keep" {
            writeln!(out, "  {label:<32} {count}")?;
        }
    }
    writeln!(out)
}

fn write_outcome(out: &mut dyn Write, report: &Report) -> io::Result<()> {
    let counts = &report.counts;
    match &report.write {
        Some(write) => {
            writeln!(
                out,
                "Wrote {} ({} fixed, {} deleted, {} cells, masters: {}){}",
                write.path.display(),
                write.refs_fixed,
                write.refs_deleted,
                write.cells,
                join(&write.masters.iter().map(String::as_str).collect::<Vec<_>>()),
                if write.verified {
                    ", verified"
                } else {
                    ", NOT verified"
                }
            )?;
            for backup in &write.backups {
                writeln!(out, "  backup: {}", backup.display())?;
            }
        }
        None if counts.fix + counts.delete == 0 => writeln!(out, "Nothing to write.")?,
        None => writeln!(out, "Nothing written (dry run).")?,
    }
    Ok(())
}

/// Writes one line per reference, for the log.
///
/// # Errors
///
/// Returns output stream errors.
pub(crate) fn write_ref_lines(out: &mut dyn Write, refs: &[RefVerdict]) -> io::Result<()> {
    writeln!(out, "cell\tkey\tid\tverdict\tdetail")?;
    for entry in refs {
        writeln!(
            out,
            "({}, {})\t{}:{}\t{}\t{}\t{}",
            entry.cell[0],
            entry.cell[1],
            entry.key[0],
            entry.key[1],
            entry.id,
            entry.verdict.label(),
            detail(entry)
        )?;
    }
    Ok(())
}

fn detail(entry: &RefVerdict) -> String {
    let measured = entry.measured.map(|measured| {
        format!(
            "terrain_z={:.1} gap={:.2} min_gap={:.2} tilt_delta={:.1}deg",
            measured.terrain_z,
            measured.contact.gap,
            measured.contact.min_gap,
            measured.tilt_delta_degrees
        )
    });
    let verdict = match &entry.verdict {
        Verdict::Keep { reason } => serde_json::to_string(reason).unwrap_or_default(),
        Verdict::Fix(fix) => format!(
            "to=({:.1}, {:.1}, {:.1}) rot=({:.3}, {:.3}, {:.3}) gap {:.2} -> {:.2}{}",
            fix.translation[0],
            fix.translation[1],
            fix.translation[2],
            fix.rotation[0],
            fix.rotation[1],
            fix.rotation[2],
            fix.gap_before,
            fix.gap_after,
            fix.occluder
                .as_deref()
                .map(|occluder| format!(" away from {occluder}"))
                .unwrap_or_default()
        ),
        Verdict::Delete { reason } => serde_json::to_string(reason).unwrap_or_default(),
        Verdict::Skip { reason } => serde_json::to_string(reason).unwrap_or_default(),
    };
    match measured {
        Some(measured) => format!("{verdict} [{measured}]"),
        None => verdict,
    }
}

fn join(items: &[&str]) -> String {
    if items.is_empty() {
        "none".to_owned()
    } else {
        items.join(", ")
    }
}
