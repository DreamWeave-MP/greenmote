// SPDX-License-Identifier: GPL-3.0-only

use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufWriter, Write},
};

use tes3::esp::{Landscape, Plugin};

use crate::groundcover::{CancellationToken, LOG_NAME, openmw};

use super::{
    args::UnclipPolicy,
    config::UnclipConfig,
    contact_baseline::{ContactBaselineIndex, build_contact_baselines},
    generated_placement::GeneratedPlacementIndex,
    inspection::{ReferenceInspectionContext, count_target_refs, inspect_target_refs},
    mesh::{MeshCache, StaticMeshIndex},
    model::{TerrainInspectionReport, UnclipReportContext, UnclipReportContextInput},
    occlusion::StaticOccluderIndex,
    report,
    setup::{
        active_cells, build_static_index, load_context_plugins, load_target_plugin,
        path_matches_any, resolve_content_plugin_paths, resolve_target_plugin,
    },
    static_occluders::{StaticOccluderBuildReport, build_static_occluders},
    target::TargetRefIndex,
    terrain::TerrainIndex,
    write_plan::{WritePlan, WriteReport, WriteStatusIndex},
    write_policy::{UnclipWritePlanningInput, apply_unclip_write_plan, plan_unclip_adjustments},
    writer::save_plugin_with_backup,
};

#[allow(clippy::too_many_lines)]
pub fn run(
    config: &UnclipConfig,
    stdout: &mut dyn Write,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    super::check_cancellation(cancellation)?;
    let policy = config
        .policy()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    super::check_cancellation(cancellation)?;
    let openmw_config = openmw::load_config_from_path(config.openmw_cfg.as_deref())?;
    let vfs = openmw::build_vfs(&openmw_config);
    let target_plugin = resolve_target_plugin(&config.plugin, &openmw_config, &vfs)?;
    super::check_cancellation(cancellation)?;
    let mut target_plugin_data = load_target_plugin(&target_plugin.source_path)?;
    let target_refs = TargetRefIndex::build(&target_plugin_data, &policy, cancellation)?;
    if target_refs.target_cells.is_empty() {
        return write_no_target_report(
            stdout,
            NoTargetReportInput {
                config,
                openmw_config: &openmw_config,
                target_plugin: &target_plugin,
                target_plugin_data: &mut target_plugin_data,
                target_refs: &target_refs,
                policy: &policy,
                cancellation,
            },
        );
    }
    super::check_cancellation(cancellation)?;
    let context_plugin_paths = context_plugin_paths(&openmw_config, &vfs)?;
    super::check_cancellation(cancellation)?;
    let context_plugins = load_context_plugins(&context_plugin_paths, cancellation)?;
    let target_is_active = path_matches_any(&target_plugin.source_path, &context_plugin_paths);
    super::check_cancellation(cancellation)?;
    let active_static_index = build_static_index(&context_plugins, None);
    super::check_cancellation(cancellation)?;
    let target_static_index = target_static_index(
        &context_plugins,
        &active_static_index,
        target_is_active,
        &target_plugin_data,
    );
    let active_cells = active_cells(&target_refs.target_cells)?;
    super::check_cancellation(cancellation)?;
    let terrain = terrain_from_context_plugins(&context_plugins, &active_cells);
    super::check_cancellation(cancellation)?;
    let mut mesh_cache = MeshCache::new(&vfs);
    let generated_placements = load_generated_placements(
        config,
        &target_plugin_data,
        &target_refs,
        &terrain,
        &target_static_index,
        cancellation,
    )?;
    let contact_baselines = build_contact_baselines(
        &target_plugin_data,
        &target_refs,
        &terrain,
        &target_static_index,
        &mut mesh_cache,
        cancellation,
    )?;
    let (static_occluders, static_occluder_report) = build_static_occluders(
        &context_plugins,
        &active_cells,
        &active_static_index,
        &mut mesh_cache,
        &target_refs.target_static_ids,
        &policy.occluder_filter,
        cancellation,
    )?;
    let missing_active_terrain_cells = missing_active_terrain_cells(&active_cells, &terrain);
    let mut report_context = build_report_context(ReportContextBuildInput {
        target_plugin_path: &target_plugin.source_path,
        target_refs: &target_refs,
        active_cells: active_cells.len(),
        terrain: &terrain,
        missing_active_terrain_cells,
        static_occluder_report,
        write_requested: config.write,
        policy: &policy,
    });
    let write_actions_enabled = policy.write_actions.any_enabled();
    let write_plan = Some(plan_requested_unclip_adjustments(
        UnclipWritePlanningInput {
            plugin: &target_plugin_data,
            target_refs: &target_refs,
            terrain: &terrain,
            static_index: &target_static_index,
            mesh_contacts: &mut mesh_cache,
            static_occluders: &static_occluders,
            policy: &policy,
            generated_placements: &generated_placements,
            retain_static_bounds_details: config.verbose,
            cancellation,
        },
        write_actions_enabled,
    )?);
    super::check_cancellation(cancellation)?;
    let write_status = write_plan.as_ref().map(WriteStatusIndex::from_plan);
    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    let mut log = BufWriter::new(File::create(log_path)?);
    super::check_cancellation(cancellation)?;
    let mut output = OutputContext {
        plugin: &target_plugin_data,
        target_refs: &target_refs,
        terrain: &terrain,
        static_index: &target_static_index,
        mesh_contacts: &mut mesh_cache,
        static_occluders: &static_occluders,
        report: &report_context,
        policy: &policy,
        contact_baselines: &contact_baselines,
        generated_placements: &generated_placements,
    };
    let inspection = inspect_refs_and_write_optional_log(
        &mut log,
        config,
        &mut output,
        write_status.as_ref(),
        cancellation,
    )?;
    super::check_cancellation(cancellation)?;
    report_context.write = save_write_plan(
        &mut target_plugin_data,
        &target_plugin.source_path,
        &target_plugin.destination_path,
        write_plan,
        config.write,
        (!write_actions_enabled).then_some("all_write_actions_disabled"),
    )?;
    super::check_cancellation(cancellation)?;
    write_reports(
        stdout,
        config,
        &mut log,
        &report_context,
        &inspection,
        &contact_baselines,
    )?;
    Ok(())
}

struct NoTargetReportInput<'a> {
    config: &'a UnclipConfig,
    openmw_config: &'a openmw_config::OpenMWConfiguration,
    target_plugin: &'a super::setup::TargetPluginPath,
    target_plugin_data: &'a mut Plugin,
    target_refs: &'a TargetRefIndex,
    policy: &'a UnclipPolicy,
    cancellation: &'a CancellationToken,
}

fn write_no_target_report(
    stdout: &mut dyn Write,
    input: NoTargetReportInput<'_>,
) -> io::Result<()> {
    let NoTargetReportInput {
        config,
        openmw_config,
        target_plugin,
        target_plugin_data,
        target_refs,
        policy,
        cancellation,
    } = input;
    super::check_cancellation(cancellation)?;
    let terrain = TerrainIndex::from_landscapes(std::iter::empty());
    let mut report_context = build_report_context(ReportContextBuildInput {
        target_plugin_path: &target_plugin.source_path,
        target_refs,
        active_cells: 0,
        terrain: &terrain,
        missing_active_terrain_cells: Vec::new(),
        static_occluder_report: StaticOccluderBuildReport::default(),
        write_requested: config.write,
        policy,
    });
    let write_actions_enabled = policy.write_actions.any_enabled();
    report_context.write = save_write_plan(
        target_plugin_data,
        &target_plugin.source_path,
        &target_plugin.destination_path,
        Some(WritePlan::default()),
        config.write,
        (!write_actions_enabled).then_some("all_write_actions_disabled"),
    )?;
    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    let mut log = BufWriter::new(File::create(log_path)?);
    super::check_cancellation(cancellation)?;
    if config.verbose {
        report::write_instance_header(&mut log, &report_context)?;
    }
    write_reports(
        stdout,
        config,
        &mut log,
        &report_context,
        &TerrainInspectionReport::default(),
        &ContactBaselineIndex::default(),
    )
}

fn context_plugin_paths(
    openmw_config: &openmw_config::OpenMWConfiguration,
    vfs: &vfstool_lib::VFS,
) -> io::Result<Vec<std::path::PathBuf>> {
    resolve_content_plugin_paths(&openmw::content_files(openmw_config)?, vfs)
}

struct ReportContextBuildInput<'a> {
    target_plugin_path: &'a std::path::Path,
    target_refs: &'a TargetRefIndex,
    active_cells: usize,
    terrain: &'a TerrainIndex,
    missing_active_terrain_cells: Vec<(i32, i32)>,
    static_occluder_report: StaticOccluderBuildReport,
    write_requested: bool,
    policy: &'a UnclipPolicy,
}

fn build_report_context(input: ReportContextBuildInput<'_>) -> UnclipReportContext {
    UnclipReportContext::new(
        UnclipReportContextInput {
            target_plugin_path: input.target_plugin_path,
            target_exterior_cells: input.target_refs.target_cells.len(),
            target_refs_total: input.target_refs.exterior_ref_count,
            active_cells: input.active_cells,
            loaded_terrain_cells_total: input.terrain.len(),
            missing_active_terrain_cells: input.missing_active_terrain_cells,
            static_occluder_report: input.static_occluder_report,
            write_requested: input.write_requested,
        },
        input.policy,
    )
}

fn target_static_index(
    context_plugins: &[Plugin],
    active_static_index: &StaticMeshIndex,
    target_is_active: bool,
    target_plugin_data: &Plugin,
) -> StaticMeshIndex {
    if target_is_active {
        active_static_index.clone()
    } else {
        build_static_index(context_plugins, Some(target_plugin_data))
    }
}

fn write_reports(
    stdout: &mut dyn Write,
    config: &UnclipConfig,
    log: &mut dyn Write,
    report_context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
    contact_baselines: &ContactBaselineIndex,
) -> io::Result<()> {
    write_output_footer(stdout, config, report_context, inspection)?;
    write_log_footer(
        log,
        report_context,
        inspection,
        contact_baselines,
        config.verbose,
    )
}

fn plan_requested_unclip_adjustments(
    input: UnclipWritePlanningInput<'_, '_>,
    write_actions_enabled: bool,
) -> io::Result<WritePlan> {
    if write_actions_enabled {
        plan_unclip_adjustments(input)
    } else {
        super::check_cancellation(input.cancellation)?;
        Ok(WritePlan::default())
    }
}

fn missing_active_terrain_cells(
    active_cells: &BTreeSet<(i32, i32)>,
    terrain: &TerrainIndex,
) -> Vec<(i32, i32)> {
    active_cells
        .iter()
        .copied()
        .filter(|cell| !terrain.has_cell(*cell))
        .collect()
}

fn terrain_from_context_plugins(
    context_plugins: &[Plugin],
    active_cells: &BTreeSet<(i32, i32)>,
) -> TerrainIndex {
    TerrainIndex::from_landscapes_in_cells(
        context_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Landscape>),
        active_cells,
    )
}

fn save_write_plan(
    plugin: &mut Plugin,
    source_path: &std::path::Path,
    destination_path: &std::path::Path,
    write_plan: Option<WritePlan>,
    write_requested: bool,
    no_change_reason: Option<&'static str>,
) -> io::Result<Option<WriteReport>> {
    let Some(write_plan) = write_plan else {
        return Ok(None);
    };
    if write_plan.changed_refs() == 0 {
        return Ok(Some(WriteReport::not_written(
            destination_path,
            write_plan,
            no_change_reason.unwrap_or("no_refs_changed"),
        )));
    }
    if !write_requested {
        return Ok(Some(WriteReport::not_written(
            destination_path,
            write_plan,
            "inspect_only",
        )));
    }
    apply_unclip_write_plan(plugin, &write_plan);
    save_plugin_with_backup(plugin, source_path, destination_path, write_plan).map(Some)
}

struct OutputContext<'a, 'b> {
    plugin: &'a Plugin,
    target_refs: &'a TargetRefIndex,
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshCache<'b>,
    static_occluders: &'a StaticOccluderIndex,
    report: &'a UnclipReportContext,
    policy: &'a UnclipPolicy,
    contact_baselines: &'a ContactBaselineIndex,
    generated_placements: &'a GeneratedPlacementIndex,
}

fn inspect_refs_and_write_optional_log(
    log: &mut dyn Write,
    config: &UnclipConfig,
    output: &mut OutputContext<'_, '_>,
    write_status: Option<&WriteStatusIndex>,
    cancellation: &CancellationToken,
) -> io::Result<TerrainInspectionReport> {
    let mut context = ReferenceInspectionContext {
        terrain: output.terrain,
        static_index: output.static_index,
        mesh_contacts: output.mesh_contacts,
        static_occluders: output.static_occluders,
        policy: output.policy,
        write: write_status,
        contact_baselines: output.contact_baselines,
        generated_placements: output.generated_placements,
    };
    if config.verbose {
        report::write_instance_header(log, output.report)?;
        inspect_target_refs(
            output.plugin,
            output.target_refs,
            &mut context,
            cancellation,
            |reference| report::write_reference_text(log, reference),
        )
    } else {
        count_target_refs(
            output.plugin,
            output.target_refs,
            &mut context,
            cancellation,
        )
    }
}

fn load_generated_placements(
    config: &UnclipConfig,
    plugin: &Plugin,
    target_refs: &TargetRefIndex,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    cancellation: &CancellationToken,
) -> io::Result<GeneratedPlacementIndex> {
    GeneratedPlacementIndex::build(
        config.meshgenerator_ini.as_deref(),
        plugin,
        target_refs,
        terrain,
        static_index,
        cancellation,
    )
}

fn write_output_footer(
    stdout: &mut dyn Write,
    config: &UnclipConfig,
    context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
) -> io::Result<()> {
    if config.structured {
        report::write_structured_summary(stdout, context, inspection)
    } else {
        report::write_summary_text(stdout, context, inspection, false)
    }
}

fn write_log_footer(
    log: &mut dyn Write,
    context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
    contact_baselines: &ContactBaselineIndex,
    verbose: bool,
) -> io::Result<()> {
    if verbose {
        writeln!(log)?;
    }
    report::write_contact_baseline_diagnostics(log, contact_baselines, context.write.as_ref())?;
    writeln!(log)?;
    report::write_summary_text(log, context, inspection, verbose)
}

#[cfg(test)]
mod tests {
    use crate::unclip::{
        args::WriteActionArg,
        config::UnclipConfig,
        contact_baseline::ContactBaselineIndex,
        model::{TerrainInspectionReport, UnclipReportContext},
        setup::build_static_index,
        write_plan::{WriteAdjustment, WritePlan, WriteReport},
    };

    use tes3::esp::{Plugin, Static, TES3Object};

    use super::{target_static_index, write_log_footer, write_output_footer};

    #[test]
    fn active_target_static_index_reuses_active_index_without_overlay() {
        let context_plugins = vec![plugin_with_statics([static_record(
            "grass_shared",
            "meshes/active.nif",
        )])];
        let active_static_index = build_static_index(&context_plugins, None);
        let target_plugin = plugin_with_statics([
            static_record("grass_shared", "meshes/target.nif"),
            static_record("grass_target_only", "meshes/target_only.nif"),
        ]);

        let index =
            target_static_index(&context_plugins, &active_static_index, true, &target_plugin);

        assert_eq!(
            index
                .get("grass_shared")
                .map(|static_| static_.mesh_path.as_str()),
            Some("meshes/active.nif")
        );
        assert!(index.get("grass_target_only").is_none());
    }

    #[test]
    fn inactive_target_static_index_overlays_target_statics() {
        let context_plugins = vec![plugin_with_statics([
            static_record("grass_context", "meshes/context.nif"),
            static_record("grass_shared", "meshes/context_shared.nif"),
        ])];
        let active_static_index = build_static_index(&context_plugins, None);
        let target_plugin = plugin_with_statics([
            static_record("grass_shared", "meshes/target_shared.nif"),
            static_record("grass_target_only", "meshes/target_only.nif"),
        ]);

        let index = target_static_index(
            &context_plugins,
            &active_static_index,
            false,
            &target_plugin,
        );

        assert_eq!(
            index
                .get("grass_context")
                .map(|static_| static_.mesh_path.as_str()),
            Some("meshes/context.nif")
        );
        assert_eq!(
            index
                .get("grass_shared")
                .map(|static_| static_.mesh_path.as_str()),
            Some("meshes/target_shared.nif")
        );
        assert_eq!(
            index
                .get("grass_target_only")
                .map(|static_| static_.mesh_path.as_str()),
            Some("meshes/target_only.nif")
        );
    }

    #[test]
    fn structured_footer_omits_write_change_records() {
        let config = UnclipConfig {
            openmw_cfg: None,
            plugin: "plugin.omwaddon".into(),
            meshgenerator_ini: None,
            verbose: true,
            structured: true,
            write: true,
            write_actions: vec![
                WriteActionArg::TerrainZ,
                WriteActionArg::StaticDelete,
                WriteActionArg::StaticMove,
                WriteActionArg::Orient,
            ],
            origin_epsilon: 0.5,
            relocation_step: 32.0,
            relocation_steps: 8,
            orientation_epsilon: 1.0,
            include_grass_ids: Vec::new(),
            exclude_grass_ids: Vec::new(),
            include_occluder_ids: Vec::new(),
            exclude_occluder_ids: Vec::new(),
        };
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

        write_output_footer(
            &mut output,
            &config,
            &context,
            &TerrainInspectionReport::default(),
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("\"kind\":\"greenmote_unclip_terrain_inspection\""));
        assert!(!output.contains("\"type\":\"write_adjustment\""));
        assert!(!output.contains("\"old_z\""));
    }

    #[test]
    fn text_footer_omits_write_change_lines_by_default() {
        let config = UnclipConfig {
            openmw_cfg: None,
            plugin: "plugin.omwaddon".into(),
            meshgenerator_ini: None,
            verbose: false,
            structured: false,
            write: true,
            write_actions: vec![
                WriteActionArg::TerrainZ,
                WriteActionArg::StaticDelete,
                WriteActionArg::StaticMove,
                WriteActionArg::Orient,
            ],
            origin_epsilon: 0.5,
            relocation_step: 32.0,
            relocation_steps: 8,
            orientation_epsilon: 1.0,
            include_grass_ids: Vec::new(),
            exclude_grass_ids: Vec::new(),
            include_occluder_ids: Vec::new(),
            exclude_occluder_ids: Vec::new(),
        };
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

        write_output_footer(
            &mut output,
            &config,
            &context,
            &TerrainInspectionReport::default(),
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Adjusted refs: 1"));
        assert!(!output.contains("WRITE CELL"));
    }

    #[test]
    fn verbose_log_footer_keeps_write_change_lines() {
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

        write_log_footer(
            &mut output,
            &context,
            &TerrainInspectionReport::default(),
            &ContactBaselineIndex::default(),
            true,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Adjusted refs: 1"));
        assert!(output.contains("WRITE CELL"));
    }

    #[test]
    fn default_log_footer_omits_write_change_lines() {
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

        write_log_footer(
            &mut output,
            &context,
            &TerrainInspectionReport::default(),
            &ContactBaselineIndex::default(),
            false,
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Adjusted refs: 1"));
        assert!(!output.contains("WRITE CELL"));
    }

    fn write_adjustment() -> WriteAdjustment {
        WriteAdjustment {
            cell: [1, 2],
            reference_key: [3, 4],
            id: "grass".to_owned(),
            old_z: 10.0,
            new_z: 12.0,
            applied_delta: 2.0,
            sample_kind: "contact",
            contact_position: [0.0, 0.0, 7.0],
            terrain_z: 9.0,
        }
    }

    fn plugin_with_statics<const N: usize>(statics: [Static; N]) -> Plugin {
        Plugin {
            objects: statics.into_iter().map(TES3Object::from).collect(),
        }
    }

    fn static_record(id: &str, mesh: &str) -> Static {
        Static {
            id: id.to_owned(),
            mesh: mesh.to_owned(),
            ..Static::default()
        }
    }
}
