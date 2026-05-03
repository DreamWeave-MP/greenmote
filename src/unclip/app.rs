use std::{io, io::Write};

use tes3::esp::{Landscape, Plugin};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    args::UnclipPolicy,
    inspection::{ReferenceInspectionContext, count_target_refs, inspect_target_refs},
    mesh::{MeshBoundsCache, MeshContactCache, StaticMeshIndex},
    model::{TerrainInspectionReport, UnclipReportContext, UnclipReportContextInput},
    occlusion::StaticOccluderIndex,
    report,
    setup::{
        active_cells, build_static_index, load_context_plugins, load_target_plugin,
        path_matches_any, resolve_content_plugin_paths, resolve_target_plugin,
    },
    static_occluders::build_static_occluders,
    target::{target_exterior_cells, target_exterior_ref_count, target_reference_static_ids},
    terrain::TerrainIndex,
    write_plan::{WritePlan, WriteReport, WriteStatusIndex},
    write_policy::{apply_unclip_write_plan, plan_unclip_adjustments},
    writer::save_plugin_with_backup,
};

pub fn run(args: &UnclipArgs, stdout: &mut dyn Write) -> io::Result<()> {
    let policy = args
        .policy()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let openmw_config = openmw::load_config_from_path(args.openmw_cfg.as_deref())?;
    let vfs = openmw::build_vfs(&openmw_config);
    let target_plugin = resolve_target_plugin(&args.plugin, &openmw_config, &vfs)?;
    let mut target_plugin_data = load_target_plugin(&target_plugin.source_path)?;
    let content_files = openmw::content_files(&openmw_config)?;
    let context_plugin_paths = resolve_content_plugin_paths(&content_files, &vfs)?;
    let context_plugins = load_context_plugins(&context_plugin_paths)?;
    let target_is_active = path_matches_any(&target_plugin.source_path, &context_plugin_paths);
    let active_static_index = build_static_index(&context_plugins, None);
    let target_static_index = build_static_index(
        &context_plugins,
        (!target_is_active).then_some(&target_plugin_data),
    );
    let target_cells = target_exterior_cells(&target_plugin_data, &policy);
    let active_cells = active_cells(&target_cells)?;
    let terrain = TerrainIndex::from_landscapes_in_cells(
        context_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Landscape>),
        &active_cells,
    );
    let target_static_ids = target_reference_static_ids(&target_plugin_data, &policy);
    let mut context_meshes = MeshBoundsCache::new(&vfs);
    let static_occluders = build_static_occluders(
        &context_plugins,
        &active_cells,
        &active_static_index,
        &mut context_meshes,
        &target_static_ids,
    );
    let missing_active_terrain_cells = active_cells
        .iter()
        .copied()
        .filter(|cell| !terrain.has_cell(*cell))
        .collect::<Vec<_>>();
    let mut report_context = UnclipReportContext::new(
        UnclipReportContextInput {
            target_plugin_path: &target_plugin.source_path,
            target_exterior_cells: target_cells.len(),
            target_refs_total: target_exterior_ref_count(&target_plugin_data),
            active_cells: active_cells.len(),
            loaded_terrain_cells_total: terrain.len(),
            missing_active_terrain_cells,
            write_requested: args.write,
        },
        &policy,
    );
    let mut target_meshes = MeshContactCache::new(&vfs);
    let write_plan = if args.write {
        if policy.write_actions.any_enabled() {
            Some(plan_unclip_adjustments(
                &target_plugin_data,
                &terrain,
                &target_static_index,
                &mut target_meshes,
                &static_occluders,
                &policy,
            ))
        } else {
            Some(WritePlan::default())
        }
    } else {
        None
    };
    let write_status = if args.instances {
        write_plan.as_ref().map(WriteStatusIndex::from_plan)
    } else {
        None
    };

    let mut output = OutputContext {
        plugin: &target_plugin_data,
        terrain: &terrain,
        static_index: &target_static_index,
        mesh_contacts: &mut target_meshes,
        static_occluders: &static_occluders,
        report: &report_context,
        policy: &policy,
    };
    let inspection = write_output(stdout, args, &mut output, write_status.as_ref())?;
    report_context.write = save_write_plan(
        &mut target_plugin_data,
        &target_plugin.source_path,
        &target_plugin.destination_path,
        write_plan,
        (!policy.write_actions.any_enabled()).then_some("all_write_actions_disabled"),
    )?;
    write_output_footer(stdout, args, &report_context, &inspection)?;

    Ok(())
}

fn save_write_plan(
    plugin: &mut Plugin,
    source_path: &std::path::Path,
    destination_path: &std::path::Path,
    write_plan: Option<WritePlan>,
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
    apply_unclip_write_plan(plugin, &write_plan);
    save_plugin_with_backup(plugin, source_path, destination_path, write_plan).map(Some)
}

struct OutputContext<'a, 'b> {
    plugin: &'a Plugin,
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'b>,
    static_occluders: &'a StaticOccluderIndex,
    report: &'a UnclipReportContext,
    policy: &'a UnclipPolicy,
}

fn write_output(
    stdout: &mut dyn Write,
    args: &UnclipArgs,
    output: &mut OutputContext<'_, '_>,
    write_status: Option<&WriteStatusIndex>,
) -> io::Result<TerrainInspectionReport> {
    match (args.structured, args.instances) {
        (false, false) => Ok(write_text_summary(
            output.plugin,
            output.terrain,
            output.static_index,
            output.mesh_contacts,
            output.static_occluders,
            output.policy,
        )),
        (false, true) => write_instance_text(stdout, output, write_status),
        (true, false) => Ok(write_structured_summary(
            output.plugin,
            output.terrain,
            output.static_index,
            output.mesh_contacts,
            output.static_occluders,
            output.policy,
        )),
        (true, true) => write_structured_instances(stdout, output, write_status),
    }
}

fn write_output_footer(
    stdout: &mut dyn Write,
    args: &UnclipArgs,
    context: &UnclipReportContext,
    inspection: &TerrainInspectionReport,
) -> io::Result<()> {
    match (args.structured, args.instances) {
        (false, false) => report::write_summary_text(stdout, context, inspection, true),
        (false, true) => {
            writeln!(stdout)?;
            report::write_summary_text(stdout, context, inspection, true)
        }
        (true, false) => report::write_structured_summary(stdout, context, inspection),
        (true, true) => {
            report::write_structured_write_records(stdout, context.write.as_ref())?;
            report::write_structured_summary_record(stdout, context, inspection)
        }
    }
}

fn write_text_summary(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    policy: &UnclipPolicy,
) -> TerrainInspectionReport {
    count_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        policy,
    )
}

fn write_instance_text(
    stdout: &mut dyn Write,
    output: &mut OutputContext<'_, '_>,
    write_status: Option<&WriteStatusIndex>,
) -> io::Result<TerrainInspectionReport> {
    report::write_instance_header(stdout, output.report)?;
    let mut context = ReferenceInspectionContext {
        terrain: output.terrain,
        static_index: output.static_index,
        mesh_contacts: output.mesh_contacts,
        static_occluders: output.static_occluders,
        policy: output.policy,
    };
    let inspection = inspect_target_refs(output.plugin, &mut context, write_status, |reference| {
        report::write_reference_text(stdout, reference)
    })?;
    Ok(inspection)
}

fn write_structured_summary(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    policy: &UnclipPolicy,
) -> TerrainInspectionReport {
    count_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        policy,
    )
}

fn write_structured_instances(
    stdout: &mut dyn Write,
    output: &mut OutputContext<'_, '_>,
    write_status: Option<&WriteStatusIndex>,
) -> io::Result<TerrainInspectionReport> {
    report::write_structured_header(stdout, output.report)?;

    let mut context = ReferenceInspectionContext {
        terrain: output.terrain,
        static_index: output.static_index,
        mesh_contacts: output.mesh_contacts,
        static_occluders: output.static_occluders,
        policy: output.policy,
    };
    let inspection = inspect_target_refs(output.plugin, &mut context, write_status, |reference| {
        report::write_structured_reference_record(stdout, reference)
    })?;
    Ok(inspection)
}

#[cfg(test)]
mod tests {
    use crate::unclip::{
        UnclipArgs,
        args::WriteActionArg,
        model::{TerrainInspectionReport, UnclipReportContext},
        write_plan::{WriteAdjustment, WritePlan, WriteReport},
    };

    use super::write_output_footer;

    #[test]
    fn structured_instance_footer_writes_changes_before_summary() {
        let args = UnclipArgs {
            openmw_cfg: None,
            plugin: "plugin.omwaddon".into(),
            instances: true,
            structured: true,
            write: true,
            write_actions: vec![
                WriteActionArg::TerrainZ,
                WriteActionArg::StaticDelete,
                WriteActionArg::StaticMove,
            ],
            contact_epsilon: 0.5,
            origin_epsilon: 0.5,
            relocation_step: 32.0,
            relocation_steps: 8,
            include_ids: Vec::new(),
            exclude_ids: Vec::new(),
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
            &args,
            &context,
            &TerrainInspectionReport::default(),
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        let write_record = output.find("\"type\":\"write_adjustment\"").unwrap();
        let summary_record = output.find("\"type\":\"summary\"").unwrap();
        assert!(write_record < summary_record);
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
