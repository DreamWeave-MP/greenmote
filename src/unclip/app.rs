use std::{collections::BTreeMap, collections::BTreeSet, io, io::Write, path::PathBuf};

use tes3::esp::{Cell, Landscape, Plugin, Static};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    args::UnclipPolicy,
    cells::{CellCoord, active_grid},
    mesh::{MeshBoundsCache, MeshContactCache, StaticMeshIndex},
    model::{
        MeshContactInspection, OriginInspection, ReferenceInspection,
        StaticBoundsOcclusionInspection, StaticMeshInspection, TerrainInspectionReport,
        UnclipReportContext,
    },
    occlusion::{
        StaticBoundsAction, StaticOccluder, StaticOccluderIndex, decide_static_bounds_action,
    },
    report,
    terrain::TerrainIndex,
    write_plan::{WritePlan, WriteReport, WriteStatusIndex},
    write_policy::{
        RefTransform, apply_unclip_write_plan, find_valid_relocation_transform,
        plan_unclip_adjustments,
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
    let target_cells = target_exterior_cells(&target_plugin_data);
    let active_cells = active_cells(&target_cells)?;
    let terrain = TerrainIndex::from_landscapes_in_cells(
        context_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Landscape>),
        &active_cells,
    );
    let target_static_ids = target_reference_static_ids(&target_plugin_data);
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
        &target_plugin.source_path,
        target_cells.len(),
        active_cells.len(),
        terrain.len(),
        missing_active_terrain_cells,
        policy.origin_epsilon,
        policy.contact_epsilon,
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
    )?;
    write_output_footer(stdout, args, &report_context, &inspection)?;

    Ok(())
}

fn save_write_plan(
    plugin: &mut Plugin,
    source_path: &std::path::Path,
    destination_path: &std::path::Path,
    write_plan: Option<WritePlan>,
) -> io::Result<Option<WriteReport>> {
    let Some(write_plan) = write_plan else {
        return Ok(None);
    };
    if write_plan.changed_refs() == 0 {
        return Ok(Some(WriteReport::not_written(destination_path, write_plan)));
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

fn resolve_content_plugin_paths(
    content_files: &[String],
    vfs: &vfstool_lib::VFS,
) -> io::Result<Vec<PathBuf>> {
    content_files
        .iter()
        .map(|plugin| {
            vfs.get_file(plugin.as_str())
                .map(|file| file.path().to_path_buf())
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("active content file {plugin} was not found in the VFS"),
                    )
                })
        })
        .collect()
}

struct TargetPluginPath {
    source_path: PathBuf,
    destination_path: PathBuf,
}

fn resolve_target_plugin(
    plugin: &std::path::Path,
    openmw_config: &openmw_config::OpenMWConfiguration,
    vfs: &vfstool_lib::VFS,
) -> io::Result<TargetPluginPath> {
    if plugin.is_file() {
        let path = plugin.to_path_buf();
        return Ok(TargetPluginPath {
            source_path: path.clone(),
            destination_path: path,
        });
    }

    let plugin_name = plugin.to_string_lossy();
    let source_path = vfs
        .get_file(plugin_name.as_ref())
        .map(|file| file.path().to_path_buf())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "target plugin {} was not found as a file or VFS entry",
                    plugin.display()
                ),
            )
        })?;
    let destination_path = vfs_target_destination(plugin, &source_path, openmw_config)?;

    Ok(TargetPluginPath {
        source_path,
        destination_path,
    })
}

fn vfs_target_destination(
    plugin: &std::path::Path,
    source_path: &std::path::Path,
    openmw_config: &openmw_config::OpenMWConfiguration,
) -> io::Result<PathBuf> {
    let file_name = plugin.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("target plugin {} has no filename", plugin.display()),
        )
    })?;
    let directory = openmw_config.data_local().map_or_else(
        || {
            source_path.parent().map_or_else(
                || {
                    Err(io::Error::other(format!(
                        "target plugin {} has no parent directory",
                        source_path.display()
                    )))
                },
                |parent| Ok(parent.to_path_buf()),
            )
        },
        |data_local| Ok(data_local.parsed().to_path_buf()),
    )?;

    Ok(directory.join(file_name))
}

fn load_target_plugin(path: &std::path::Path) -> io::Result<Plugin> {
    Plugin::from_path(path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to load target plugin {}: {error}", path.display()),
        )
    })
}

fn load_context_plugins(paths: &[PathBuf]) -> io::Result<Vec<Plugin>> {
    paths
        .iter()
        .map(|path| {
            Plugin::from_path_filtered(path, |tag| {
                &tag == Landscape::TAG || &tag == Static::TAG || &tag == Cell::TAG
            })
            .map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "failed to load unclip context from {}: {error}",
                        path.display()
                    ),
                )
            })
        })
        .collect()
}

fn build_static_index(
    active_plugins: &[Plugin],
    extra_target_plugin: Option<&Plugin>,
) -> StaticMeshIndex {
    StaticMeshIndex::from_statics(
        active_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Static>)
            .chain(
                extra_target_plugin
                    .into_iter()
                    .flat_map(tes3::esp::Plugin::objects_of_type::<Static>),
            ),
    )
}

fn path_matches_any(path: &std::path::Path, candidates: &[PathBuf]) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    candidates
        .iter()
        .map(|candidate| {
            candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone())
        })
        .any(|candidate| candidate == path)
}

fn target_exterior_cells(plugin: &Plugin) -> BTreeSet<CellCoord> {
    plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .map(|cell| cell.data.grid)
        .collect()
}

fn active_cells(target_cells: &BTreeSet<CellCoord>) -> io::Result<BTreeSet<CellCoord>> {
    let mut cells = BTreeSet::new();

    for cell in target_cells {
        cells.extend(active_grid(*cell)?);
    }

    Ok(cells)
}

fn target_reference_static_ids(plugin: &Plugin) -> BTreeSet<String> {
    plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .flat_map(|cell| cell.references.values())
        .filter(|reference| reference.deleted != Some(true))
        .map(|reference| reference.id.to_lowercase())
        .collect()
}

fn build_static_occluders(
    active_plugins: &[Plugin],
    active_cells: &BTreeSet<CellCoord>,
    static_index: &StaticMeshIndex,
    mesh_bounds: &mut MeshBoundsCache<'_>,
    target_static_ids: &BTreeSet<String>,
) -> StaticOccluderIndex {
    let effective_refs = effective_active_refs(active_plugins, active_cells);
    let mut occluders = Vec::new();

    for (key, reference) in effective_refs {
        let reference_id_key = reference.id.to_lowercase();
        if target_static_ids.contains(&reference_id_key) {
            continue;
        }

        let Some(static_mesh) = static_index.get_normalized_key(&reference_id_key) else {
            continue;
        };
        let Ok(bounds) = mesh_bounds.bounds(static_mesh) else {
            continue;
        };

        occluders.push(StaticOccluder {
            id: reference.id.clone(),
            cell: [key.cell.0, key.cell.1],
            reference_key: [key.reference.0, key.reference.1],
            bounds: bounds.world_aabb(reference.translation, reference.rotation, reference.scale),
        });
    }

    StaticOccluderIndex::new(occluders)
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct EffectiveRefKey {
    cell: CellCoord,
    reference: (u32, u32),
}

fn effective_active_refs<'a>(
    active_plugins: &'a [Plugin],
    active_cells: &BTreeSet<CellCoord>,
) -> BTreeMap<EffectiveRefKey, &'a tes3::esp::Reference> {
    let mut refs = BTreeMap::new();

    for plugin in active_plugins {
        for cell in plugin.objects_of_type::<Cell>() {
            if !cell.is_exterior() || !active_cells.contains(&cell.data.grid) {
                continue;
            }

            for (reference_key, reference) in &cell.references {
                let key = EffectiveRefKey {
                    cell: reference.moved_cell.unwrap_or(cell.data.grid),
                    reference: *reference_key,
                };
                if reference.deleted == Some(true) {
                    refs.remove(&key);
                } else {
                    refs.insert(key, reference);
                }
            }
        }
    }

    refs
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

fn sorted_exterior_cells(plugin: &Plugin) -> Vec<&Cell> {
    let mut cells = plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.data.grid);
    cells
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
            adjusted: write.is_some_and(|write| write.is_adjusted(cell, key)),
            deleted: write.is_some_and(|write| write.is_deleted(cell, key)),
            moved: write.is_some_and(|write| write.is_moved(cell, key)),
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

struct WriteStatusEvidence {
    adjusted: bool,
    deleted: bool,
    moved: bool,
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
        write_status: write_status_label(
            input.reference,
            input.mesh_resolution,
            input.contact_details,
            input.write.adjusted,
            input.write.deleted,
            input.write.moved,
            input.contact_epsilon,
        ),
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

fn write_status_label(
    reference: &tes3::esp::Reference,
    mesh_resolution: &MeshContactResolution<'_>,
    contact_details: Option<&ContactDetails>,
    was_adjusted: bool,
    was_deleted: bool,
    was_moved: bool,
    contact_epsilon: f32,
) -> &'static str {
    if was_deleted {
        return "deleted_static_bounds_occluded";
    }
    if was_moved {
        return "moved_static_bounds_occluded";
    }
    if was_adjusted {
        return "adjusted";
    }
    if reference.deleted == Some(true) {
        return "skipped_deleted_ref";
    }
    match mesh_resolution {
        MeshContactResolution::UnresolvedStatic => "skipped_unresolved_static",
        MeshContactResolution::MissingContact { .. } => "skipped_missing_mesh_contact",
        MeshContactResolution::Resolved { .. } => {
            contact_details.map_or("skipped_missing_contact_terrain", |details| {
                match details.delta {
                    Some(delta) if delta.abs() > contact_epsilon => "adjusted_or_adjustable",
                    Some(_) => "skipped_within_epsilon",
                    None => "skipped_missing_contact_terrain",
                }
            })
        }
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
    use std::collections::BTreeSet;

    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use crate::unclip::{
        UnclipArgs,
        args::WriteActionArg,
        model::{TerrainInspectionReport, UnclipReportContext},
        write_plan::{WriteAdjustment, WritePlan, WriteReport},
    };

    use super::{
        MeshContactResolution, deleted_reference_inspection, effective_active_refs,
        write_output_footer, write_status_label,
    };

    #[test]
    fn write_status_prefers_adjusted_plan_evidence() {
        let reference = reference_at_z(10.0);

        assert_eq!(
            write_status_label(
                &reference,
                &MeshContactResolution::UnresolvedStatic,
                None,
                true,
                false,
                false,
                0.5,
            ),
            "adjusted"
        );
    }

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
    fn effective_active_refs_apply_later_deletions() {
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([(
                (1, 2),
                reference_at_z(0.0),
            )]))],
        };
        let deleted = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([((1, 2), deleted_ref())]))],
        };

        let plugins = [first, deleted];
        let refs = effective_active_refs(&plugins, &BTreeSet::from([(0, 0)]));

        assert!(refs.is_empty());
    }

    #[test]
    fn effective_active_refs_key_moved_refs_by_original_cell() {
        let mut moved = reference_at_z(0.0);
        moved.moved_cell = Some((0, 0));
        let mut deleted = deleted_ref();
        deleted.moved_cell = Some((0, 0));
        let first = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell_at(
                (1, 0),
                [((1, 2), moved)],
            ))],
        };
        let second = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([((1, 2), deleted)]))],
        };

        let plugins = [first, second];
        let refs = effective_active_refs(&plugins, &BTreeSet::from([(0, 0), (1, 0)]));

        assert!(refs.is_empty());
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

    fn exterior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        exterior_cell_at((0, 0), refs)
    }

    fn exterior_cell_at(
        grid: (i32, i32),
        refs: impl IntoIterator<Item = ((u32, u32), Reference)>,
    ) -> Cell {
        let mut cell = Cell {
            data: CellData {
                grid,
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }

    fn deleted_ref() -> Reference {
        Reference {
            deleted: Some(true),
            id: "rock".to_owned(),
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
