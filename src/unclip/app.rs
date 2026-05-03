use std::{collections::BTreeSet, fs, io, io::Write, path::PathBuf};

use serde::Serialize;
use tes3::esp::{Cell, Landscape, Plugin, Static, TES3Object};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    cells::{CellCoord, active_grid},
    mesh::{MeshContactCache, StaticMeshIndex},
    terrain::TerrainIndex,
};

const ORIGIN_TERRAIN_EPSILON: f32 = 0.5;
const CONTACT_TERRAIN_EPSILON: f32 = 0.5;

pub fn run(args: &UnclipArgs, stdout: &mut dyn Write) -> io::Result<()> {
    let openmw_config = openmw::load_config_from_path(args.openmw_cfg.as_deref())?;
    let vfs = openmw::build_vfs(&openmw_config);
    let target_plugin = resolve_target_plugin(&args.plugin, &openmw_config, &vfs)?;
    let mut target_plugin_data = load_target_plugin(&target_plugin.source_path)?;
    let content_files = openmw::content_files(&openmw_config)?;
    let terrain_plugin_paths = resolve_content_plugin_paths(&content_files, &vfs)?;
    let terrain_plugins = load_terrain_plugins(&terrain_plugin_paths)?;
    let target_is_active = path_matches_any(&target_plugin.source_path, &terrain_plugin_paths);
    let static_index = build_static_index(
        &terrain_plugins,
        (!target_is_active).then_some(&target_plugin_data),
    );
    let terrain = TerrainIndex::from_landscapes(
        terrain_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Landscape>),
    );
    let target_cells = target_exterior_cells(&target_plugin_data);
    let active_cells = active_cells(&target_cells)?;
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
    );
    if args.write {
        let mut mesh_contacts = MeshContactCache::new(&vfs);
        let write_plan = apply_unclip_adjustments(
            &mut target_plugin_data,
            &terrain,
            &static_index,
            &mut mesh_contacts,
        );
        let write_report = if write_plan.adjusted_refs == 0 {
            WriteReport::not_written(&target_plugin.destination_path, write_plan)
        } else {
            save_plugin_with_backup(
                &mut target_plugin_data,
                &target_plugin.source_path,
                &target_plugin.destination_path,
                write_plan,
            )?
        };
        report_context.write = Some(write_report);
    }

    let mut mesh_contacts = MeshContactCache::new(&vfs);
    write_output(
        stdout,
        &target_plugin_data,
        &terrain,
        &static_index,
        &mut mesh_contacts,
        &report_context,
        args,
    )?;

    Ok(())
}

fn write_output(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    context: &UnclipReportContext,
    args: &UnclipArgs,
) -> io::Result<()> {
    match (args.structured, args.instances) {
        (false, false) => write_text_summary(
            stdout,
            plugin,
            terrain,
            static_index,
            mesh_contacts,
            context,
        ),
        (false, true) => write_instance_text(
            stdout,
            plugin,
            terrain,
            static_index,
            mesh_contacts,
            context,
        ),
        (true, false) => write_structured_summary(
            stdout,
            plugin,
            terrain,
            static_index,
            mesh_contacts,
            context,
        ),
        (true, true) => write_structured_instances(
            stdout,
            plugin,
            terrain,
            static_index,
            mesh_contacts,
            context,
        ),
    }
}

fn write_text_summary(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    context: &UnclipReportContext,
) -> io::Result<()> {
    let inspection = count_target_refs(plugin, terrain, static_index, mesh_contacts);
    write_summary_text(stdout, context, &inspection, true)
}

fn write_instance_text(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    context: &UnclipReportContext,
) -> io::Result<()> {
    writeln!(stdout, "Unclip reference diagnostics")?;
    writeln!(stdout, "Target plugin: {}", context.target_plugin)?;
    if let Some(write) = &context.write
        && !write.adjustments.is_empty()
    {
        writeln!(stdout)?;
        writeln!(stdout, "Write adjustments")?;
        for adjustment in &write.adjustments {
            write_adjustment_text(stdout, adjustment)?;
        }
    }
    writeln!(stdout)?;
    let inspection = inspect_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        context.write.as_ref(),
        |reference| write_reference_text(stdout, reference),
    )?;
    writeln!(stdout)?;
    write_summary_text(stdout, context, &inspection, false)
}

fn write_structured_summary(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    context: &UnclipReportContext,
) -> io::Result<()> {
    let inspection = count_target_refs(plugin, terrain, static_index, mesh_contacts);
    let report = StructuredSummaryReport {
        kind: "greenmote_unclip_terrain_inspection",
        target_plugin: &context.target_plugin,
        write: context.write.as_ref(),
        missing_active_terrain_cells: context.missing_active_terrain_cells(),
        summary: context.summary(&inspection),
    };
    write_json(stdout, &report)?;
    writeln!(stdout)
}

fn write_structured_instances(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
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
    writeln!(stdout)?;

    if let Some(write) = &context.write {
        for adjustment in &write.adjustments {
            let record = StructuredWriteAdjustmentRecord {
                r#type: "write_adjustment",
                adjustment,
            };
            write_json(stdout, &record)?;
            writeln!(stdout)?;
        }
    }

    let inspection = inspect_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        context.write.as_ref(),
        |reference| {
            let record = StructuredReferenceRecord {
                r#type: "ref",
                reference,
            };
            write_json(stdout, &record)?;
            writeln!(stdout)
        },
    )?;

    let summary = StructuredSummaryRecord {
        r#type: "summary",
        summary: context.summary(&inspection),
    };
    write_json(stdout, &summary)?;
    writeln!(stdout)
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

fn write_summary_text(
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
    write_threshold_summary_text(stdout, &summary)
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
                "No plugin written: no refs were adjusted at {}",
                write.destination_plugin
            )?;
        }
        writeln!(stdout, "Adjusted refs: {}", write.adjusted_refs)?;
        if include_adjustments {
            for adjustment in &write.adjustments {
                write_adjustment_text(stdout, adjustment)?;
            }
        }
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

fn write_reference_text(stdout: &mut dyn Write, reference: &ReferenceInspection) -> io::Result<()> {
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

fn apply_unclip_adjustments(
    plugin: &mut Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
) -> WritePlan {
    let mut plan = WritePlan::default();

    for object in &mut plugin.objects {
        let TES3Object::Cell(cell) = object else {
            continue;
        };
        if !cell.is_exterior() {
            continue;
        }

        for (key, reference) in &mut cell.references {
            if let Some(adjustment) = adjust_reference_z(
                cell.data.grid,
                *key,
                reference,
                terrain,
                static_index,
                mesh_contacts,
            ) {
                plan.adjusted_refs += 1;
                plan.adjustments.push(adjustment);
            }
        }
    }

    plan
}

fn adjust_reference_z(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
) -> Option<WriteAdjustment> {
    if reference.deleted == Some(true) {
        return None;
    }
    let static_mesh = static_index.get(&reference.id)?;
    let Ok(contact) = mesh_contacts.contact(&static_mesh.mesh_path) else {
        return None;
    };
    let position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(position[0], position[1])?;
    apply_contact_adjustment(cell, key, reference, position, terrain_z)
}

fn apply_contact_adjustment(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
    contact_position: [f32; 3],
    terrain_z: f32,
) -> Option<WriteAdjustment> {
    let contact_delta = contact_position[2] - terrain_z;
    if contact_delta.abs() <= CONTACT_TERRAIN_EPSILON {
        return None;
    }
    let old_z = reference.translation[2];
    reference.translation[2] -= contact_delta;
    Some(WriteAdjustment {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        old_z,
        new_z: reference.translation[2],
        applied_delta: -contact_delta,
        contact_position,
        terrain_z,
    })
}

fn save_plugin_with_backup(
    plugin: &mut Plugin,
    source_path: &std::path::Path,
    destination_path: &std::path::Path,
    plan: WritePlan,
) -> io::Result<WriteReport> {
    if let Some(parent) = destination_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let temp_path = next_temp_plugin_path(destination_path);
    if let Err(error) = plugin.save_path(&temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "failed to write temporary plugin {}: {error}",
                temp_path.display()
            ),
        ));
    }

    let backup = match prepare_plugin_backup(source_path, destination_path) {
        Ok(backup) => backup,
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
    };

    let had_destination = path_entry_exists(destination_path);
    if let Err(error) = replace_with_temp(
        &temp_path,
        destination_path,
        backup.path().filter(|_| had_destination),
    ) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    Ok(WriteReport {
        written: true,
        destination_plugin: destination_path.display().to_string(),
        backup_plugin: backup.path().map(|path| path.display().to_string()),
        adjusted_refs: plan.adjusted_refs,
        adjusted_ref_keys: adjusted_ref_keys(&plan.adjustments),
        adjustments: plan.adjustments,
    })
}

fn replace_with_temp(
    temp_path: &std::path::Path,
    destination_path: &std::path::Path,
    backup_path: Option<&std::path::Path>,
) -> io::Result<()> {
    if path_entry_exists(destination_path) {
        fs::remove_file(destination_path).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "failed to remove {} before replacement: {error}",
                    destination_path.display()
                ),
            )
        })?;
    }

    if let Err(error) = rename_with_context(temp_path, destination_path) {
        if let Some(backup_path) = backup_path {
            restore_backup_to_destination(backup_path, destination_path).map_err(|restore_error| {
                io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; additionally failed to restore backup {} to {}: {restore_error}",
                        backup_path.display(),
                        destination_path.display()
                    ),
                )
            })?;
        }
        return Err(error);
    }

    Ok(())
}

fn restore_backup_to_destination(
    backup_path: &std::path::Path,
    destination_path: &std::path::Path,
) -> io::Result<()> {
    let restore_temp_path = next_restore_temp_plugin_path(destination_path);
    if let Err(error) = fs::copy(backup_path, &restore_temp_path) {
        let _ = fs::remove_file(&restore_temp_path);
        return Err(io::Error::new(
            error.kind(),
            format!(
                "failed to copy backup {} to restore temp {}: {error}",
                backup_path.display(),
                restore_temp_path.display()
            ),
        ));
    }
    if let Err(error) = rename_with_context(&restore_temp_path, destination_path) {
        let _ = fs::remove_file(&restore_temp_path);
        return Err(error);
    }
    Ok(())
}

enum BackupAction {
    None,
    Copied { path: PathBuf },
}

impl BackupAction {
    fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::None => None,
            Self::Copied { path } => Some(path),
        }
    }
}

fn prepare_plugin_backup(
    source_path: &std::path::Path,
    destination_path: &std::path::Path,
) -> io::Result<BackupAction> {
    let backup_path = next_numbered_backup_path(destination_path);
    if path_entry_exists(destination_path) {
        copy_backup(destination_path, &backup_path)?;
        return Ok(BackupAction::Copied { path: backup_path });
    }

    if !same_path(source_path, destination_path) && path_entry_exists(source_path) {
        copy_backup(source_path, &backup_path)?;
        return Ok(BackupAction::Copied { path: backup_path });
    }

    Ok(BackupAction::None)
}

fn copy_backup(source: &std::path::Path, backup_path: &std::path::Path) -> io::Result<()> {
    fs::copy(source, backup_path).map(|_| ()).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to copy backup {} to {}: {error}",
                source.display(),
                backup_path.display()
            ),
        )
    })
}

fn rename_with_context(source: &std::path::Path, destination: &std::path::Path) -> io::Result<()> {
    fs::rename(source, destination).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to move {} to {}: {error}",
                source.display(),
                destination.display()
            ),
        )
    })
}

fn next_temp_plugin_path(plugin_path: &std::path::Path) -> PathBuf {
    for index in 0.. {
        let candidate = append_path_suffix(plugin_path, &format!(".greenmote-tmp.{index}"));
        if !path_entry_exists(&candidate) {
            return candidate;
        }
    }

    unreachable!("unbounded temp suffix search should always find a candidate")
}

fn next_restore_temp_plugin_path(plugin_path: &std::path::Path) -> PathBuf {
    for index in 0.. {
        let candidate = append_path_suffix(plugin_path, &format!(".greenmote-restore-tmp.{index}"));
        if !path_entry_exists(&candidate) {
            return candidate;
        }
    }

    unreachable!("unbounded restore temp suffix search should always find a candidate")
}

fn next_numbered_backup_path(plugin_path: &std::path::Path) -> PathBuf {
    for index in 1.. {
        let candidate = append_path_suffix(plugin_path, &format!(".{index:03}"));
        if !path_entry_exists(&candidate) {
            return candidate;
        }
    }

    unreachable!("unbounded backup suffix search should always find a candidate")
}

fn append_path_suffix(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut file_name = path
        .file_name()
        .expect("plugin path should have filename")
        .to_os_string();
    file_name.push(suffix);
    path.with_file_name(file_name)
}

fn path_entry_exists(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn load_terrain_plugins(paths: &[PathBuf]) -> io::Result<Vec<Plugin>> {
    paths
        .iter()
        .map(|path| {
            Plugin::from_path_filtered(path, |tag| &tag == Landscape::TAG || &tag == Static::TAG)
                .map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("failed to load terrain from {}: {error}", path.display()),
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

struct UnclipReportContext {
    target_plugin: String,
    target_exterior_cells: usize,
    active_cells: usize,
    loaded_terrain_cells_total: usize,
    missing_active_terrain_cells: Vec<CellCoord>,
    write: Option<WriteReport>,
}

impl UnclipReportContext {
    fn new(
        target_plugin_path: &std::path::Path,
        target_exterior_cells: usize,
        active_cells: usize,
        loaded_terrain_cells_total: usize,
        missing_active_terrain_cells: Vec<CellCoord>,
    ) -> Self {
        Self {
            target_plugin: target_plugin_path.display().to_string(),
            target_exterior_cells,
            active_cells,
            loaded_terrain_cells_total,
            missing_active_terrain_cells,
            write: None,
        }
    }

    fn missing_active_terrain_cells(&self) -> Vec<[i32; 2]> {
        self.missing_active_terrain_cells
            .iter()
            .map(|&(x, y)| [x, y])
            .collect()
    }

    fn summary(&self, inspection: &TerrainInspectionReport) -> UnclipSummary {
        let active_terrain_cells_missing = self.missing_active_terrain_cells.len();
        UnclipSummary {
            target_exterior_cells: self.target_exterior_cells,
            active_cells: self.active_cells,
            loaded_terrain_cells_total: self.loaded_terrain_cells_total,
            active_terrain_cells_loaded: self.active_cells - active_terrain_cells_missing,
            active_terrain_cells_missing,
            refs: inspection.refs,
            refs_actionable: inspection.refs - inspection.refs_deleted,
            refs_deleted: inspection.refs_deleted,
            refs_with_mesh_contact: inspection.refs_with_mesh_contact,
            refs_without_resolved_static: inspection.refs_without_resolved_static,
            refs_missing_mesh_contact: inspection.refs_missing_mesh_contact,
            refs_with_terrain: inspection.refs_with_terrain,
            refs_missing_terrain: inspection.refs_missing_terrain,
            refs_origin_above_terrain: inspection.refs_above_terrain,
            refs_origin_below_terrain: inspection.refs_below_terrain,
            refs_mesh_contact_above_terrain: inspection.refs_contact_above_terrain,
            refs_mesh_contact_below_terrain: inspection.refs_contact_below_terrain,
            refs_mesh_contact_missing_terrain: inspection.refs_contact_missing_terrain,
            origin_terrain_epsilon: ORIGIN_TERRAIN_EPSILON,
            mesh_contact_terrain_epsilon: CONTACT_TERRAIN_EPSILON,
        }
    }
}

#[derive(Serialize)]
struct WriteReport {
    written: bool,
    destination_plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_plugin: Option<String>,
    adjusted_refs: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    adjustments: Vec<WriteAdjustment>,
    #[serde(skip)]
    adjusted_ref_keys: BTreeSet<AdjustedRefKey>,
}

impl WriteReport {
    fn not_written(destination_path: &std::path::Path, plan: WritePlan) -> Self {
        let adjusted_ref_keys = adjusted_ref_keys(&plan.adjustments);
        Self {
            written: false,
            destination_plugin: destination_path.display().to_string(),
            backup_plugin: None,
            adjusted_refs: plan.adjusted_refs,
            adjustments: plan.adjustments,
            adjusted_ref_keys,
        }
    }

    fn summary(&self) -> WriteSummary {
        WriteSummary {
            written: self.written,
            destination_plugin: self.destination_plugin.clone(),
            backup_plugin: self.backup_plugin.clone(),
            adjusted_refs: self.adjusted_refs,
        }
    }

    fn is_adjusted(&self, cell: CellCoord, key: (u32, u32)) -> bool {
        self.adjusted_ref_keys
            .contains(&AdjustedRefKey::new(cell, key))
    }
}

#[derive(Serialize)]
struct WriteSummary {
    written: bool,
    destination_plugin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    backup_plugin: Option<String>,
    adjusted_refs: usize,
}

#[derive(Default)]
struct WritePlan {
    adjusted_refs: usize,
    adjustments: Vec<WriteAdjustment>,
}

#[derive(Serialize)]
struct WriteAdjustment {
    cell: [i32; 2],
    reference_key: [u32; 2],
    id: String,
    old_z: f32,
    new_z: f32,
    applied_delta: f32,
    contact_position: [f32; 3],
    terrain_z: f32,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct AdjustedRefKey {
    cell: [i32; 2],
    reference_key: [u32; 2],
}

impl AdjustedRefKey {
    const fn new(cell: CellCoord, key: (u32, u32)) -> Self {
        Self {
            cell: [cell.0, cell.1],
            reference_key: [key.0, key.1],
        }
    }

    const fn from_adjustment(adjustment: &WriteAdjustment) -> Self {
        Self {
            cell: adjustment.cell,
            reference_key: adjustment.reference_key,
        }
    }
}

fn adjusted_ref_keys(adjustments: &[WriteAdjustment]) -> BTreeSet<AdjustedRefKey> {
    adjustments
        .iter()
        .map(AdjustedRefKey::from_adjustment)
        .collect()
}

#[derive(Serialize)]
struct UnclipSummary {
    target_exterior_cells: usize,
    active_cells: usize,
    loaded_terrain_cells_total: usize,
    active_terrain_cells_loaded: usize,
    active_terrain_cells_missing: usize,
    refs: usize,
    refs_actionable: usize,
    refs_deleted: usize,
    refs_with_mesh_contact: usize,
    refs_without_resolved_static: usize,
    refs_missing_mesh_contact: usize,
    refs_with_terrain: usize,
    refs_missing_terrain: usize,
    refs_origin_above_terrain: usize,
    refs_origin_below_terrain: usize,
    refs_mesh_contact_above_terrain: usize,
    refs_mesh_contact_below_terrain: usize,
    refs_mesh_contact_missing_terrain: usize,
    origin_terrain_epsilon: f32,
    mesh_contact_terrain_epsilon: f32,
}

#[derive(Serialize)]
struct ReferenceInspection {
    cell: [i32; 2],
    reference_key: [u32; 2],
    id: String,
    static_resolution: &'static str,
    mesh_contact_status: &'static str,
    deleted: bool,
    write_status: &'static str,
    origin: OriginInspection,
    #[serde(skip_serializing_if = "Option::is_none")]
    static_mesh: Option<StaticMeshInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_contact_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mesh_contact: Option<MeshContactInspection>,
}

#[derive(Serialize)]
struct OriginInspection {
    position: [f32; 3],
    terrain_z: Option<f32>,
    delta: Option<f32>,
    classification: &'static str,
}

#[derive(Serialize)]
struct StaticMeshInspection {
    id: String,
    mesh: String,
}

#[derive(Serialize)]
struct MeshContactInspection {
    position: [f32; 3],
    terrain_z: Option<f32>,
    delta: Option<f32>,
    classification: &'static str,
}

#[derive(Default)]
struct TerrainInspectionReport {
    refs: usize,
    refs_deleted: usize,
    refs_with_mesh_contact: usize,
    refs_without_resolved_static: usize,
    refs_missing_mesh_contact: usize,
    refs_with_terrain: usize,
    refs_missing_terrain: usize,
    refs_above_terrain: usize,
    refs_below_terrain: usize,
    refs_contact_above_terrain: usize,
    refs_contact_below_terrain: usize,
    refs_contact_missing_terrain: usize,
}

fn inspect_target_refs(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    write: Option<&WriteReport>,
    mut reference_sink: impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<TerrainInspectionReport> {
    let mut report = TerrainInspectionReport::default();
    let mut context = ReferenceInspectionContext {
        terrain,
        static_index,
        mesh_contacts,
    };

    let mut cells = plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.data.grid);

    for cell in cells {
        let mut references = cell.references.iter().collect::<Vec<_>>();
        references.sort_by_key(|(key, _)| **key);

        for (key, reference) in references {
            inspect_reference(
                &mut report,
                &mut context,
                cell.data.grid,
                *key,
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
) -> TerrainInspectionReport {
    let mut report = TerrainInspectionReport::default();
    let mut context = ReferenceInspectionContext {
        terrain,
        static_index,
        mesh_contacts,
    };

    let mut cells = plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.data.grid);

    for cell in cells {
        let mut references = cell.references.iter().collect::<Vec<_>>();
        references.sort_by_key(|(key, _)| **key);

        for (_, reference) in references {
            count_reference(&mut report, &mut context, reference);
        }
    }

    report
}

struct ReferenceInspectionContext<'a, 'b> {
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'b>,
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
    write: Option<&WriteReport>,
    reference_sink: &mut impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<()> {
    report.refs += 1;
    if reference.deleted == Some(true) {
        report.refs_deleted += 1;
        let inspection = deleted_reference_inspection(cell, key, reference);
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
        )),
        MeshContactResolution::UnresolvedStatic | MeshContactResolution::MissingContact { .. } => {
            None
        }
    };
    let origin = classify_origin(report, context.terrain, reference);
    let inspection = reference_inspection(
        cell,
        key,
        reference,
        &origin,
        &mesh_contact,
        contact_details.as_ref(),
        write.is_some_and(|write| write.is_adjusted(cell, key)),
    );
    reference_sink(&inspection)
}

fn count_reference(
    report: &mut TerrainInspectionReport,
    context: &mut ReferenceInspectionContext<'_, '_>,
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
        let _ = classify_contact(report, context.terrain, reference, contact);
    }
    let _ = classify_origin(report, context.terrain, reference);
}

fn classify_origin(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
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
    let classification = classify_origin_delta(delta);
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

fn reference_inspection(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    origin: &OriginDetails,
    mesh_resolution: &MeshContactResolution<'_>,
    contact_details: Option<&ContactDetails>,
    was_adjusted: bool,
) -> ReferenceInspection {
    ReferenceInspection {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        origin: OriginInspection {
            position: reference.translation,
            terrain_z: origin.terrain_z,
            delta: origin.delta,
            classification: origin.classification,
        },
        static_resolution: mesh_resolution.static_resolution_label(),
        mesh_contact_status: mesh_resolution.mesh_contact_status_label(contact_details),
        deleted: reference.deleted == Some(true),
        write_status: write_status_label(reference, mesh_resolution, contact_details, was_adjusted),
        static_mesh: mesh_resolution.static_mesh().map(static_mesh_inspection),
        mesh_contact_error: mesh_resolution.mesh_contact_error().map(str::to_owned),
        mesh_contact: contact_details.map(|contact| MeshContactInspection {
            position: contact.position,
            terrain_z: contact.terrain_z,
            delta: contact.delta,
            classification: contact.classification.map_or(
                "mesh_contact_missing_terrain",
                ContactTerrainClassification::label,
            ),
        }),
    }
}

fn deleted_reference_inspection(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
) -> ReferenceInspection {
    ReferenceInspection {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        static_resolution: "skipped_deleted_ref",
        mesh_contact_status: "skipped_deleted_ref",
        deleted: true,
        write_status: "skipped_deleted_ref",
        origin: OriginInspection {
            position: reference.translation,
            terrain_z: None,
            delta: None,
            classification: "skipped_deleted_ref",
        },
        static_mesh: None,
        mesh_contact_error: None,
        mesh_contact: None,
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
) -> &'static str {
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
                    Some(delta) if delta.abs() > CONTACT_TERRAIN_EPSILON => {
                        "adjusted_or_adjustable"
                    }
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

fn classify_contact(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    contact: &super::mesh::MeshContact,
) -> ContactDetails {
    let position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(position[0], position[1]);
    let delta = terrain_z.map(|terrain_z| position[2] - terrain_z);
    let classification = delta.map(|delta| classify_counted_contact_delta(report, delta));
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
) -> ContactTerrainClassification {
    let classification = classify_contact_delta(delta);
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

    match mesh_contacts.contact(&static_mesh.mesh_path) {
        Ok(contact) => {
            report.refs_with_mesh_contact += 1;
            MeshContactResolution::Resolved {
                static_mesh,
                contact,
            }
        }
        Err(error) => {
            report.refs_missing_mesh_contact += 1;
            let error = error.to_string();
            MeshContactResolution::MissingContact { static_mesh, error }
        }
    }
}

fn optional_f32(value: Option<f32>) -> String {
    value.map_or_else(|| "missing".to_owned(), |value| format!("{value:.3}"))
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

fn classify_origin_delta(delta: f32) -> OriginTerrainClassification {
    if delta > ORIGIN_TERRAIN_EPSILON {
        OriginTerrainClassification::Above
    } else if delta < -ORIGIN_TERRAIN_EPSILON {
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

fn classify_contact_delta(delta: f32) -> ContactTerrainClassification {
    if delta > CONTACT_TERRAIN_EPSILON {
        ContactTerrainClassification::Above
    } else if delta < -CONTACT_TERRAIN_EPSILON {
        ContactTerrainClassification::Below
    } else {
        ContactTerrainClassification::OnTerrain
    }
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use tes3::esp::Reference;

    use super::{
        MeshContactResolution, WriteAdjustment, WritePlan, WriteReport, adjusted_ref_keys,
        apply_contact_adjustment, deleted_reference_inspection, next_numbered_backup_path,
        prepare_plugin_backup, replace_with_temp, write_status_label, write_write_summary_text,
    };

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-unclip-app-test-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn numbered_plugin_backups_append_suffix_to_full_filename() {
        let temp = TempDir::new("numbered-backup");
        let plugin = temp.path().join("plugin.omwaddon");
        std::fs::write(&plugin, b"current").unwrap();
        std::fs::write(temp.path().join("plugin.omwaddon.001"), b"old").unwrap();

        assert_eq!(
            next_numbered_backup_path(&plugin),
            temp.path().join("plugin.omwaddon.002")
        );
    }

    #[test]
    fn vfs_destination_without_existing_file_gets_source_backup_copy() {
        let temp = TempDir::new("copy-source-backup");
        let source = temp.path().join("source").join("plugin.omwaddon");
        let destination = temp.path().join("data-local").join("plugin.omwaddon");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
        std::fs::write(&source, b"original").unwrap();

        let backup = prepare_plugin_backup(&source, &destination).unwrap();
        let backup_path = backup.path().unwrap();

        assert_eq!(
            backup_path,
            temp.path().join("data-local/plugin.omwaddon.001")
        );
        assert_eq!(std::fs::read(backup_path).unwrap(), b"original");
        assert!(!destination.exists());
    }

    #[test]
    fn existing_destination_is_copied_to_backup() {
        let temp = TempDir::new("copy-destination-backup");
        let plugin = temp.path().join("plugin.omwaddon");
        std::fs::write(&plugin, b"modified").unwrap();

        let backup = prepare_plugin_backup(&plugin, &plugin).unwrap();
        let backup_path = backup.path().unwrap();

        assert_eq!(backup_path, temp.path().join("plugin.omwaddon.001"));
        assert_eq!(std::fs::read(backup_path).unwrap(), b"modified");
        assert_eq!(std::fs::read(plugin).unwrap(), b"modified");
    }

    #[test]
    fn replacement_overwrites_existing_destination_after_backup() {
        let temp = TempDir::new("replace-existing");
        let plugin = temp.path().join("plugin.omwaddon");
        let temp_plugin = temp.path().join("plugin.omwaddon.greenmote-tmp.0");
        let backup = temp.path().join("plugin.omwaddon.001");
        std::fs::write(&plugin, b"old").unwrap();
        std::fs::write(&backup, b"old").unwrap();
        std::fs::write(&temp_plugin, b"new").unwrap();

        replace_with_temp(&temp_plugin, &plugin, Some(&backup)).unwrap();

        assert_eq!(std::fs::read(&plugin).unwrap(), b"new");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old");
        assert!(!temp_plugin.exists());
    }

    #[test]
    fn failed_replacement_without_prior_destination_does_not_restore_backup() {
        let temp = TempDir::new("replace-missing-destination-fails");
        let plugin = temp.path().join("missing").join("plugin.omwaddon");
        let temp_plugin = temp.path().join("plugin.omwaddon.greenmote-tmp.0");
        let backup = temp.path().join("plugin.omwaddon.001");
        std::fs::write(&temp_plugin, b"new").unwrap();
        std::fs::write(&backup, b"original").unwrap();

        let result = replace_with_temp(&temp_plugin, &plugin, None);

        assert!(result.is_err());
        assert!(!plugin.exists());
        assert_eq!(std::fs::read(&backup).unwrap(), b"original");
    }

    #[test]
    fn contact_adjustment_moves_buried_contact_up() {
        let mut reference = reference_at_z(10.0);

        let adjustment =
            apply_contact_adjustment((1, 2), (3, 4), &mut reference, [0.0, 0.0, 7.0], 9.0).unwrap();

        assert_close(reference.translation[2], 12.0);
        assert_close(adjustment.old_z, 10.0);
        assert_close(adjustment.new_z, 12.0);
        assert_close(adjustment.applied_delta, 2.0);
    }

    #[test]
    fn contact_adjustment_moves_floating_contact_down() {
        let mut reference = reference_at_z(10.0);

        let adjustment =
            apply_contact_adjustment((1, 2), (3, 4), &mut reference, [0.0, 0.0, 12.0], 9.0)
                .unwrap();

        assert_close(reference.translation[2], 7.0);
        assert_close(adjustment.applied_delta, -3.0);
    }

    #[test]
    fn contact_adjustment_skips_within_epsilon() {
        let mut reference = reference_at_z(10.0);

        let adjustment =
            apply_contact_adjustment((1, 2), (3, 4), &mut reference, [0.0, 0.0, 9.25], 9.0);

        assert!(adjustment.is_none());
        assert_close(reference.translation[2], 10.0);
    }

    #[test]
    fn write_summary_prints_adjustments_only_when_requested() {
        let adjustments = vec![write_adjustment()];
        let report = WriteReport {
            written: false,
            destination_plugin: "plugin.omwaddon".to_owned(),
            backup_plugin: None,
            adjusted_refs: 1,
            adjusted_ref_keys: adjusted_ref_keys(&adjustments),
            adjustments,
        };
        let mut with_adjustments = Vec::new();
        let mut without_adjustments = Vec::new();

        write_write_summary_text(&mut with_adjustments, Some(&report), true).unwrap();
        write_write_summary_text(&mut without_adjustments, Some(&report), false).unwrap();

        let with_adjustments = String::from_utf8(with_adjustments).unwrap();
        let without_adjustments = String::from_utf8(without_adjustments).unwrap();
        assert!(with_adjustments.contains("WRITE CELL"));
        assert!(!without_adjustments.contains("WRITE CELL"));
    }

    #[test]
    fn write_status_prefers_adjusted_plan_evidence() {
        let reference = reference_at_z(10.0);

        assert_eq!(
            write_status_label(
                &reference,
                &MeshContactResolution::UnresolvedStatic,
                None,
                true
            ),
            "adjusted"
        );
    }

    #[test]
    fn write_report_uses_adjusted_key_set() {
        let report = WriteReport::not_written(
            Path::new("plugin.omwaddon"),
            WritePlan {
                adjusted_refs: 1,
                adjustments: vec![write_adjustment()],
            },
        );

        assert!(report.is_adjusted((1, 2), (3, 4)));
        assert!(!report.is_adjusted((1, 2), (3, 5)));
    }

    #[test]
    fn deleted_reference_inspection_skips_actionable_details() {
        let mut reference = reference_at_z(10.0);
        reference.deleted = Some(true);

        let inspection = deleted_reference_inspection((1, 2), (3, 4), &reference);

        assert!(inspection.deleted);
        assert_eq!(inspection.static_resolution, "skipped_deleted_ref");
        assert_eq!(inspection.mesh_contact_status, "skipped_deleted_ref");
        assert_eq!(inspection.write_status, "skipped_deleted_ref");
        assert!(inspection.static_mesh.is_none());
        assert!(inspection.mesh_contact.is_none());
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

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
