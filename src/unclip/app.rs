use std::{io, io::Write};

use tes3::esp::{Landscape, Plugin};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    args::UnclipPolicy,
    cells::CellCoord,
    mesh::{MeshBoundsCache, MeshContactCache, StaticMeshIndex},
    model::{
        MeshContactInspection, OriginInspection, ReferenceInspection,
        StaticBoundsOcclusionInspection, StaticMeshInspection, TerrainInspectionReport,
        UnclipReportContext, UnclipReportContextInput,
    },
    occlusion::{StaticBoundsAction, StaticOccluderIndex, decide_static_bounds_action},
    report,
    setup::{
        active_cells, build_static_index, load_context_plugins, load_target_plugin,
        path_matches_any, resolve_content_plugin_paths, resolve_target_plugin,
    },
    static_occluders::build_static_occluders,
    target::{
        sorted_exterior_cells, target_exterior_cells, target_exterior_ref_count,
        target_reference_static_ids,
    },
    terrain::TerrainIndex,
    write_plan::{WritePlan, WriteReport, WriteStatusIndex},
    write_policy::{
        RefTransform, apply_unclip_write_plan, find_valid_relocation_transform,
        plan_unclip_adjustments,
    },
    write_status::{
        MeshResolutionStatus, WriteStatusEvidence, WriteStatusInput, write_plan_evidence,
        write_status_label,
    },
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

fn inspect_target_refs(
    plugin: &Plugin,
    context: &mut ReferenceInspectionContext<'_, '_>,
    write: Option<&WriteStatusIndex>,
    mut reference_sink: impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<TerrainInspectionReport> {
    let mut report = TerrainInspectionReport::default();

    for cell in sorted_exterior_cells(plugin) {
        let mut reference_keys = cell.references.keys().copied().collect::<Vec<_>>();
        reference_keys.sort_unstable();
        for key in reference_keys {
            let Some(reference) = cell.references.get(&key) else {
                continue;
            };
            if !context.policy.target_filter.includes(&reference.id) {
                continue;
            }
            inspect_reference(
                &mut report,
                context,
                cell.data.grid,
                key,
                reference,
                write,
                &mut reference_sink,
            )?;
        }
    }

    Ok(report)
}

fn count_target_refs(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    policy: &UnclipPolicy,
) -> TerrainInspectionReport {
    let mut report = TerrainInspectionReport::default();
    let mut context = ReferenceInspectionContext {
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        policy,
    };

    for cell in sorted_exterior_cells(plugin) {
        let mut reference_keys = cell.references.keys().copied().collect::<Vec<_>>();
        reference_keys.sort_unstable();
        for key in reference_keys {
            let Some(reference) = cell.references.get(&key) else {
                continue;
            };
            if !policy.target_filter.includes(&reference.id) {
                continue;
            }
            count_reference(&mut report, &mut context, cell.data.grid, reference);
        }
    }

    report
}

struct ReferenceInspectionContext<'a, 'b> {
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'b>,
    static_occluders: &'a StaticOccluderIndex,
    policy: &'a UnclipPolicy,
}

struct OriginDetails {
    terrain_z: Option<f32>,
    delta: Option<f32>,
    classification: &'static str,
}

fn inspect_reference(
    report: &mut TerrainInspectionReport,
    context: &mut ReferenceInspectionContext<'_, '_>,
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    write: Option<&WriteStatusIndex>,
    reference_sink: &mut impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<()> {
    report.refs += 1;
    if reference.deleted == Some(true) {
        report.refs_deleted += 1;
        let inspection = deleted_reference_inspection(
            cell,
            key,
            reference,
            if write.is_some_and(|write| write.is_deleted(cell, key)) {
                "deleted_static_bounds_occluded"
            } else {
                "skipped_deleted_ref"
            },
        );
        return reference_sink(&inspection);
    }

    let mesh_contact = resolve_ref_mesh_contact(
        report,
        reference,
        context.static_index,
        context.mesh_contacts,
    );
    let contact_details = match &mesh_contact {
        MeshContactResolution::Resolved { contact, .. } => Some(classify_contact(
            report,
            context.terrain,
            reference,
            contact,
            context.policy.contact_epsilon,
        )),
        MeshContactResolution::UnresolvedStatic | MeshContactResolution::MissingContact { .. } => {
            None
        }
    };
    let static_bounds_occlusion = classify_static_bounds_occlusion(
        report,
        cell,
        context.terrain,
        reference,
        &mesh_contact,
        context.static_occluders,
        context.policy,
    );
    let origin = classify_origin(
        report,
        context.terrain,
        reference,
        context.policy.origin_epsilon,
    );
    let inspection = reference_inspection(&ReferenceInspectionInput {
        cell,
        key,
        reference,
        origin: &origin,
        mesh_resolution: &mesh_contact,
        contact_details: contact_details.as_ref(),
        static_bounds_occlusion: static_bounds_occlusion.as_ref(),
        write: WriteStatusEvidence {
            plan: write_plan_evidence(write, cell, key),
            actions: context.policy.write_actions,
        },
        contact_epsilon: context.policy.contact_epsilon,
    });
    reference_sink(&inspection)
}

fn count_reference(
    report: &mut TerrainInspectionReport,
    context: &mut ReferenceInspectionContext<'_, '_>,
    cell: CellCoord,
    reference: &tes3::esp::Reference,
) {
    report.refs += 1;
    if reference.deleted == Some(true) {
        report.refs_deleted += 1;
        return;
    }

    let mesh_contact = resolve_ref_mesh_contact(
        report,
        reference,
        context.static_index,
        context.mesh_contacts,
    );
    if let MeshContactResolution::Resolved { contact, .. } = &mesh_contact {
        let _ = classify_contact(
            report,
            context.terrain,
            reference,
            contact,
            context.policy.contact_epsilon,
        );
    }
    let _ = classify_static_bounds_occlusion(
        report,
        cell,
        context.terrain,
        reference,
        &mesh_contact,
        context.static_occluders,
        context.policy,
    );
    let _ = classify_origin(
        report,
        context.terrain,
        reference,
        context.policy.origin_epsilon,
    );
}

fn classify_origin(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    epsilon: f32,
) -> OriginDetails {
    let [x, y, z] = reference.translation;
    let Some(terrain_z) = terrain.height_at(x, y) else {
        report.refs_missing_terrain += 1;
        return OriginDetails {
            terrain_z: None,
            delta: None,
            classification: "origin_missing_terrain",
        };
    };

    report.refs_with_terrain += 1;
    let delta = z - terrain_z;
    let classification = classify_origin_delta(delta, epsilon);
    match classification {
        OriginTerrainClassification::Above => report.refs_above_terrain += 1,
        OriginTerrainClassification::Below => report.refs_below_terrain += 1,
        OriginTerrainClassification::OnTerrain => {}
    }

    OriginDetails {
        terrain_z: Some(terrain_z),
        delta: Some(delta),
        classification: classification.label(),
    }
}

struct ReferenceInspectionInput<'a, 'b> {
    cell: CellCoord,
    key: (u32, u32),
    reference: &'a tes3::esp::Reference,
    origin: &'a OriginDetails,
    mesh_resolution: &'a MeshContactResolution<'b>,
    contact_details: Option<&'a ContactDetails>,
    static_bounds_occlusion: Option<&'a StaticBoundsOcclusionDetails>,
    write: WriteStatusEvidence,
    contact_epsilon: f32,
}

fn reference_inspection(input: &ReferenceInspectionInput<'_, '_>) -> ReferenceInspection {
    ReferenceInspection {
        cell: [input.cell.0, input.cell.1],
        reference_key: [input.key.0, input.key.1],
        id: input.reference.id.clone(),
        origin: OriginInspection {
            position: input.reference.translation,
            terrain_z: input.origin.terrain_z,
            delta: input.origin.delta,
            classification: input.origin.classification,
        },
        static_resolution: input.mesh_resolution.static_resolution_label(),
        mesh_contact_status: input
            .mesh_resolution
            .mesh_contact_status_label(input.contact_details),
        deleted: input.reference.deleted == Some(true),
        write_status: write_status_label(&write_status_input(input)),
        static_mesh: input
            .mesh_resolution
            .static_mesh()
            .map(static_mesh_inspection),
        mesh_contact_error: input
            .mesh_resolution
            .mesh_contact_error()
            .map(str::to_owned),
        mesh_contact: input.contact_details.map(|contact| MeshContactInspection {
            position: contact.position,
            terrain_z: contact.terrain_z,
            delta: contact.delta,
            classification: contact.classification.map_or(
                "mesh_contact_missing_terrain",
                ContactTerrainClassification::label,
            ),
        }),
        static_bounds_occlusion: input.static_bounds_occlusion.map(|occlusion| {
            StaticBoundsOcclusionInspection {
                status: occlusion.status,
                ratio: occlusion.ratio,
            }
        }),
    }
}

fn write_status_input(input: &ReferenceInspectionInput<'_, '_>) -> WriteStatusInput {
    WriteStatusInput {
        reference_deleted: input.reference.deleted == Some(true),
        mesh_status: input.mesh_resolution.write_status_mesh_status(),
        contact_delta: input.contact_details.and_then(|contact| contact.delta),
        static_bounds_status: input
            .static_bounds_occlusion
            .map(|occlusion| occlusion.status),
        write: input.write,
        contact_epsilon: input.contact_epsilon,
    }
}

fn deleted_reference_inspection(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    write_status: &'static str,
) -> ReferenceInspection {
    ReferenceInspection {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        static_resolution: "skipped_deleted_ref",
        mesh_contact_status: "skipped_deleted_ref",
        deleted: true,
        write_status,
        origin: OriginInspection {
            position: reference.translation,
            terrain_z: None,
            delta: None,
            classification: "skipped_deleted_ref",
        },
        static_mesh: None,
        mesh_contact_error: None,
        mesh_contact: None,
        static_bounds_occlusion: None,
    }
}

fn static_mesh_inspection(static_mesh: &super::mesh::StaticMesh) -> StaticMeshInspection {
    StaticMeshInspection {
        id: static_mesh.static_id.clone(),
        mesh: static_mesh.mesh_path.clone(),
    }
}

struct ContactDetails {
    position: [f32; 3],
    terrain_z: Option<f32>,
    delta: Option<f32>,
    classification: Option<ContactTerrainClassification>,
}

struct StaticBoundsOcclusionDetails {
    status: &'static str,
    ratio: f32,
}

fn classify_static_bounds_occlusion(
    report: &mut TerrainInspectionReport,
    cell: CellCoord,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    mesh_resolution: &MeshContactResolution<'_>,
    static_occluders: &StaticOccluderIndex,
    policy: &UnclipPolicy,
) -> Option<StaticBoundsOcclusionDetails> {
    let MeshContactResolution::Resolved {
        contact, bounds, ..
    } = mesh_resolution
    else {
        return None;
    };
    let mut corrected_translation = reference.translation;
    let contact_position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    if let Some(terrain_z) = terrain.height_at(contact_position[0], contact_position[1]) {
        corrected_translation[2] -= contact_position[2] - terrain_z;
    }
    let corrected_bounds =
        bounds.world_aabb(corrected_translation, reference.rotation, reference.scale);
    let action = decide_static_bounds_action(corrected_bounds, static_occluders);
    let (status, ratio) = match action {
        StaticBoundsAction::None => ("static_bounds_clear", 0.0),
        StaticBoundsAction::Delete { ratio, .. } => {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_fully_occluded += 1;
            ("static_bounds_fully_occluded", ratio)
        }
        StaticBoundsAction::Move { ratio, .. }
            if find_valid_relocation_transform(
                cell,
                RefTransform {
                    translation: corrected_translation,
                    rotation: reference.rotation,
                    scale: reference.scale,
                },
                contact,
                *bounds,
                terrain,
                static_occluders,
                policy.relocation,
            )
            .is_some() =>
        {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_relocatable += 1;
            ("static_bounds_relocatable", ratio)
        }
        StaticBoundsAction::Move { ratio, .. } => {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_blocked += 1;
            ("static_bounds_blocked", ratio)
        }
    };

    Some(StaticBoundsOcclusionDetails { status, ratio })
}

fn classify_contact(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    contact: &super::mesh::MeshContact,
    epsilon: f32,
) -> ContactDetails {
    let position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(position[0], position[1]);
    let delta = terrain_z.map(|terrain_z| position[2] - terrain_z);
    let classification = delta.map(|delta| classify_counted_contact_delta(report, delta, epsilon));
    if terrain_z.is_none() {
        report.refs_contact_missing_terrain += 1;
    }

    ContactDetails {
        position,
        terrain_z,
        delta,
        classification,
    }
}

fn classify_counted_contact_delta(
    report: &mut TerrainInspectionReport,
    delta: f32,
    epsilon: f32,
) -> ContactTerrainClassification {
    let classification = classify_contact_delta(delta, epsilon);
    match classification {
        ContactTerrainClassification::Above => {
            report.refs_contact_above_terrain += 1;
        }
        ContactTerrainClassification::Below => {
            report.refs_contact_below_terrain += 1;
        }
        ContactTerrainClassification::OnTerrain => {}
    }
    classification
}

enum MeshContactResolution<'a> {
    Resolved {
        static_mesh: &'a super::mesh::StaticMesh,
        contact: &'a super::mesh::MeshContact,
        bounds: super::mesh::MeshAabb,
    },
    MissingContact {
        static_mesh: &'a super::mesh::StaticMesh,
        error: String,
    },
    UnresolvedStatic,
}

impl<'a> MeshContactResolution<'a> {
    const fn static_resolution_label(&self) -> &'static str {
        match self {
            Self::Resolved { .. } | Self::MissingContact { .. } => "resolved",
            Self::UnresolvedStatic => "unresolved",
        }
    }

    fn mesh_contact_status_label(&self, contact_details: Option<&ContactDetails>) -> &'static str {
        match self {
            Self::Resolved { .. } => contact_details.map_or("missing_terrain", |details| {
                if details.terrain_z.is_some() {
                    "resolved"
                } else {
                    "missing_terrain"
                }
            }),
            Self::MissingContact { .. } => "missing_contact",
            Self::UnresolvedStatic => "unresolved_static",
        }
    }

    const fn static_mesh(&self) -> Option<&'a super::mesh::StaticMesh> {
        match self {
            Self::Resolved { static_mesh, .. } | Self::MissingContact { static_mesh, .. } => {
                Some(*static_mesh)
            }
            Self::UnresolvedStatic => None,
        }
    }

    fn mesh_contact_error(&self) -> Option<&str> {
        match self {
            Self::MissingContact { error, .. } => Some(error),
            Self::Resolved { .. } | Self::UnresolvedStatic => None,
        }
    }

    const fn write_status_mesh_status(&self) -> MeshResolutionStatus {
        match self {
            Self::Resolved { .. } => MeshResolutionStatus::Resolved,
            Self::MissingContact { .. } => MeshResolutionStatus::MissingContact,
            Self::UnresolvedStatic => MeshResolutionStatus::UnresolvedStatic,
        }
    }
}

fn resolve_ref_mesh_contact<'a>(
    report: &mut TerrainInspectionReport,
    reference: &tes3::esp::Reference,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'_>,
) -> MeshContactResolution<'a> {
    let Some(static_mesh) = static_index.get(&reference.id) else {
        report.refs_without_resolved_static += 1;
        return MeshContactResolution::UnresolvedStatic;
    };

    match mesh_contacts.geometry(static_mesh) {
        Ok(geometry) => {
            report.refs_with_mesh_contact += 1;
            MeshContactResolution::Resolved {
                static_mesh,
                contact: &geometry.contact,
                bounds: geometry.bounds,
            }
        }
        Err(error) => {
            report.refs_missing_mesh_contact += 1;
            let error = error.to_string();
            MeshContactResolution::MissingContact { static_mesh, error }
        }
    }
}

#[derive(Clone, Copy)]
enum OriginTerrainClassification {
    Above,
    Below,
    OnTerrain,
}

impl OriginTerrainClassification {
    const fn label(self) -> &'static str {
        match self {
            Self::Above => "origin_above_terrain",
            Self::Below => "origin_below_terrain",
            Self::OnTerrain => "origin_on_terrain",
        }
    }
}

fn classify_origin_delta(delta: f32, epsilon: f32) -> OriginTerrainClassification {
    if delta > epsilon {
        OriginTerrainClassification::Above
    } else if delta < -epsilon {
        OriginTerrainClassification::Below
    } else {
        OriginTerrainClassification::OnTerrain
    }
}

#[derive(Clone, Copy)]
enum ContactTerrainClassification {
    Above,
    Below,
    OnTerrain,
}

impl ContactTerrainClassification {
    const fn label(self) -> &'static str {
        match self {
            Self::Above => "mesh_contact_above_terrain",
            Self::Below => "mesh_contact_below_terrain",
            Self::OnTerrain => "mesh_contact_on_terrain",
        }
    }
}

fn classify_contact_delta(delta: f32, epsilon: f32) -> ContactTerrainClassification {
    if delta > epsilon {
        ContactTerrainClassification::Above
    } else if delta < -epsilon {
        ContactTerrainClassification::Below
    } else {
        ContactTerrainClassification::OnTerrain
    }
}

#[cfg(test)]
mod tests {
    use tes3::esp::Reference;

    use crate::unclip::{
        UnclipArgs,
        args::WriteActionArg,
        model::{TerrainInspectionReport, UnclipReportContext},
        write_plan::{WriteAdjustment, WritePlan, WriteReport},
    };

    use super::{deleted_reference_inspection, write_output_footer};

    #[test]
    fn deleted_reference_inspection_skips_actionable_details() {
        let mut reference = reference_at_z(10.0);
        reference.deleted = Some(true);

        let inspection =
            deleted_reference_inspection((1, 2), (3, 4), &reference, "skipped_deleted_ref");

        assert!(inspection.deleted);
        assert_eq!(inspection.static_resolution, "skipped_deleted_ref");
        assert_eq!(inspection.mesh_contact_status, "skipped_deleted_ref");
        assert_eq!(inspection.write_status, "skipped_deleted_ref");
        assert!(inspection.static_mesh.is_none());
        assert!(inspection.mesh_contact.is_none());
    }

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

    fn reference_at_z(z: f32) -> Reference {
        Reference {
            id: "grass".to_owned(),
            translation: [0.0, 0.0, z],
            ..Reference::default()
        }
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
