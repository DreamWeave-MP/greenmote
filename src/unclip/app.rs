use std::{collections::BTreeMap, collections::BTreeSet, io, io::Write, path::PathBuf};

use serde::Serialize;
use tes3::esp::{Cell, Landscape, Plugin, Static, TES3Object};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    cells::{CellCoord, active_grid},
    mesh::{MeshContactCache, StaticMeshIndex},
    occlusion::{
        StaticBoundsAction, StaticOccluder, StaticOccluderIndex, decide_static_bounds_action,
        static_bounds_occlusion_ratio, translate_bounds_xy,
    },
    report,
    terrain::TerrainIndex,
    write_plan::{
        WriteAdjustment, WritePlan, WriteReport, WriteStaticBoundsDeletion, WriteStaticBoundsMove,
    },
    writer::save_plugin_with_backup,
};

const ORIGIN_TERRAIN_EPSILON: f32 = 0.5;
const CONTACT_TERRAIN_EPSILON: f32 = 0.5;
const RELOCATION_STEP: f32 = 32.0;
const RELOCATION_STEPS: u16 = 8;
const CELL_SIZE: f32 = 8192.0;
const RELOCATION_DIRECTIONS: &[[f32; 2]] = &[
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
    [0.707_106_77, 0.707_106_77],
    [0.707_106_77, -0.707_106_77],
    [-0.707_106_77, 0.707_106_77],
    [-0.707_106_77, -0.707_106_77],
];

pub fn run(args: &UnclipArgs, stdout: &mut dyn Write) -> io::Result<()> {
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
    let terrain = TerrainIndex::from_landscapes(
        context_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Landscape>),
    );
    let target_cells = target_exterior_cells(&target_plugin_data);
    let active_cells = active_cells(&target_cells)?;
    let target_static_ids = target_reference_static_ids(&target_plugin_data);
    let mut context_meshes = MeshContactCache::new(&vfs);
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
    );
    if args.write {
        let mut mesh_contacts = MeshContactCache::new(&vfs);
        let write_plan = apply_unclip_adjustments(
            &mut target_plugin_data,
            &terrain,
            &target_static_index,
            &mut mesh_contacts,
            &static_occluders,
        );
        let write_report = if write_plan.changed_refs() == 0 {
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
    let mut output = OutputContext {
        plugin: &target_plugin_data,
        terrain: &terrain,
        static_index: &target_static_index,
        mesh_contacts: &mut mesh_contacts,
        static_occluders: &static_occluders,
        report: &report_context,
    };
    write_output(stdout, args, &mut output)?;

    Ok(())
}

struct OutputContext<'a, 'b> {
    plugin: &'a Plugin,
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'b>,
    static_occluders: &'a StaticOccluderIndex,
    report: &'a UnclipReportContext,
}

fn write_output(
    stdout: &mut dyn Write,
    args: &UnclipArgs,
    output: &mut OutputContext<'_, '_>,
) -> io::Result<()> {
    match (args.structured, args.instances) {
        (false, false) => write_text_summary(
            stdout,
            output.plugin,
            output.terrain,
            output.static_index,
            output.mesh_contacts,
            output.static_occluders,
            output.report,
        ),
        (false, true) => write_instance_text(
            stdout,
            output.plugin,
            output.terrain,
            output.static_index,
            output.mesh_contacts,
            output.static_occluders,
            output.report,
        ),
        (true, false) => write_structured_summary(
            stdout,
            output.plugin,
            output.terrain,
            output.static_index,
            output.mesh_contacts,
            output.static_occluders,
            output.report,
        ),
        (true, true) => write_structured_instances(
            stdout,
            output.plugin,
            output.terrain,
            output.static_index,
            output.mesh_contacts,
            output.static_occluders,
            output.report,
        ),
    }
}

fn write_text_summary(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    context: &UnclipReportContext,
) -> io::Result<()> {
    let inspection = count_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
    );
    report::write_summary_text(stdout, context, &inspection, true)
}

fn write_instance_text(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    context: &UnclipReportContext,
) -> io::Result<()> {
    report::write_instance_header(stdout, context)?;
    let inspection = inspect_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        context.write.as_ref(),
        |reference| report::write_reference_text(stdout, reference),
    )?;
    writeln!(stdout)?;
    report::write_summary_text(stdout, context, &inspection, false)
}

fn write_structured_summary(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    context: &UnclipReportContext,
) -> io::Result<()> {
    let inspection = count_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
    );
    report::write_structured_summary(stdout, context, &inspection)
}

fn write_structured_instances(
    stdout: &mut dyn Write,
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    context: &UnclipReportContext,
) -> io::Result<()> {
    report::write_structured_header(stdout, context)?;
    report::write_structured_write_records(stdout, context.write.as_ref())?;

    let inspection = inspect_target_refs(
        plugin,
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
        context.write.as_ref(),
        |reference| report::write_structured_reference_record(stdout, reference),
    )?;
    report::write_structured_summary_record(stdout, context, &inspection)
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
    static_occluders: &StaticOccluderIndex,
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
            let change = adjust_reference_for_terrain_and_static_bounds(
                cell.data.grid,
                *key,
                reference,
                terrain,
                static_index,
                mesh_contacts,
                static_occluders,
            );
            match change {
                WriteReferenceChange::None => {}
                WriteReferenceChange::AdjustZ(adjustment) => {
                    plan.adjusted_refs += 1;
                    plan.adjustments.push(adjustment);
                }
                WriteReferenceChange::Delete(deletion) => {
                    plan.deleted_refs += 1;
                    plan.deletions.push(deletion);
                }
                WriteReferenceChange::Move(move_) => {
                    plan.moved_refs += 1;
                    plan.moves.push(move_);
                }
            }
        }
    }

    plan
}

enum WriteReferenceChange {
    None,
    AdjustZ(WriteAdjustment),
    Delete(WriteStaticBoundsDeletion),
    Move(WriteStaticBoundsMove),
}

#[derive(Clone, Copy)]
struct StaticBoundsMoveCause<'a> {
    ratio: f32,
    occluder: &'a StaticOccluder,
}

fn adjust_reference_for_terrain_and_static_bounds(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
) -> WriteReferenceChange {
    if reference.deleted == Some(true) {
        return WriteReferenceChange::None;
    }
    let Some(static_mesh) = static_index.get(&reference.id) else {
        return WriteReferenceChange::None;
    };
    let Ok(geometry) = mesh_contacts.geometry(&static_mesh.mesh_path) else {
        return WriteReferenceChange::None;
    };

    let contact_position =
        geometry
            .contact
            .world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(contact_position[0], contact_position[1]);
    let mut corrected_translation = reference.translation;
    if let Some(terrain_z) = terrain_z {
        corrected_translation[2] -= contact_position[2] - terrain_z;
    }
    let mut corrected_reference = reference.clone();
    corrected_reference.translation = corrected_translation;

    match decide_static_bounds_action(
        geometry
            .bounds
            .world_aabb(corrected_translation, reference.rotation, reference.scale),
        static_occluders,
    ) {
        StaticBoundsAction::None => {}
        StaticBoundsAction::Delete { ratio, occluder } => {
            reference.deleted = Some(true);
            return WriteReferenceChange::Delete(WriteStaticBoundsDeletion {
                cell: [cell.0, cell.1],
                reference_key: [key.0, key.1],
                id: reference.id.clone(),
                occlusion_ratio: ratio,
                occluder_id: occluder.id.clone(),
                occluder_cell: occluder.cell,
                occluder_reference_key: occluder.reference_key,
            });
        }
        StaticBoundsAction::Move {
            ratio, occluder, ..
        } => {
            let Some(move_) = apply_static_bounds_move(
                cell,
                key,
                &mut corrected_reference,
                geometry,
                terrain,
                static_occluders,
                StaticBoundsMoveCause { ratio, occluder },
            ) else {
                let Some(terrain_z) = terrain_z else {
                    return WriteReferenceChange::None;
                };
                return apply_contact_adjustment(cell, key, reference, contact_position, terrain_z)
                    .map_or(WriteReferenceChange::None, WriteReferenceChange::AdjustZ);
            };
            reference.translation = corrected_reference.translation;
            return WriteReferenceChange::Move(move_);
        }
    }

    let Some(terrain_z) = terrain_z else {
        return WriteReferenceChange::None;
    };
    apply_contact_adjustment(cell, key, reference, contact_position, terrain_z)
        .map_or(WriteReferenceChange::None, WriteReferenceChange::AdjustZ)
}

fn apply_static_bounds_move(
    cell: CellCoord,
    key: (u32, u32),
    reference: &mut tes3::esp::Reference,
    geometry: &super::mesh::MeshGeometry,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
    cause: StaticBoundsMoveCause<'_>,
) -> Option<WriteStaticBoundsMove> {
    let old_position = reference.translation;
    let new_position = find_valid_relocation_position(
        cell,
        reference,
        &geometry.contact,
        geometry.bounds,
        terrain,
        static_occluders,
    )?;
    reference.translation = new_position;

    Some(WriteStaticBoundsMove {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        occlusion_ratio: cause.ratio,
        old_position,
        new_position,
        occluder_id: cause.occluder.id.clone(),
        occluder_cell: cause.occluder.cell,
        occluder_reference_key: cause.occluder.reference_key,
    })
}

fn find_valid_relocation_position(
    cell: CellCoord,
    reference: &tes3::esp::Reference,
    contact: &super::mesh::MeshContact,
    bounds: super::mesh::MeshAabb,
    terrain: &TerrainIndex,
    static_occluders: &StaticOccluderIndex,
) -> Option<[f32; 3]> {
    let original = [reference.translation[0], reference.translation[1]];
    let grass_bounds =
        bounds.world_aabb(reference.translation, reference.rotation, reference.scale);

    for step in 1..=RELOCATION_STEPS {
        let radius = f32::from(step) * RELOCATION_STEP;
        for direction in RELOCATION_DIRECTIONS {
            let candidate_xy = [
                original[0] + direction[0] * radius,
                original[1] + direction[1] * radius,
            ];
            if !cell_contains_xy(cell, candidate_xy) {
                continue;
            }

            let moved_bounds = translate_bounds_xy(
                grass_bounds,
                candidate_xy[0] - original[0],
                candidate_xy[1] - original[1],
            );
            if static_bounds_occlusion_ratio(
                moved_bounds,
                &static_occluders.bounds_for(moved_bounds),
            ) > f32::EPSILON
            {
                continue;
            }

            let mut candidate_translation = reference.translation;
            candidate_translation[0] = candidate_xy[0];
            candidate_translation[1] = candidate_xy[1];
            let contact_position =
                contact.world_position(candidate_translation, reference.rotation, reference.scale);
            let terrain_z = terrain.height_at(contact_position[0], contact_position[1])?;
            candidate_translation[2] -= contact_position[2] - terrain_z;
            let final_bounds =
                bounds.world_aabb(candidate_translation, reference.rotation, reference.scale);
            if static_bounds_occlusion_ratio(
                final_bounds,
                &static_occluders.bounds_for(final_bounds),
            ) <= f32::EPSILON
            {
                return Some(candidate_translation);
            }
        }
    }

    None
}

#[allow(clippy::cast_precision_loss)]
fn cell_contains_xy(cell: CellCoord, position: [f32; 2]) -> bool {
    let min_x = cell.0 as f32 * CELL_SIZE;
    let min_y = cell.1 as f32 * CELL_SIZE;
    position[0] >= min_x
        && position[0] < min_x + CELL_SIZE
        && position[1] >= min_y
        && position[1] < min_y + CELL_SIZE
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
    mesh_contacts: &mut MeshContactCache<'_>,
    target_static_ids: &BTreeSet<String>,
) -> StaticOccluderIndex {
    let effective_refs = effective_active_refs(active_plugins, active_cells);
    let mut occluders = Vec::new();

    for (key, reference) in effective_refs {
        if target_static_ids.contains(&reference.id.to_lowercase()) {
            continue;
        }

        let Some(static_mesh) = static_index.get(&reference.id) else {
            continue;
        };
        let Ok(geometry) = mesh_contacts.geometry(&static_mesh.mesh_path) else {
            continue;
        };

        occluders.push(StaticOccluder {
            id: reference.id.clone(),
            cell: [key.cell.0, key.cell.1],
            reference_key: [key.reference.0, key.reference.1],
            bounds: geometry.bounds.world_aabb(
                reference.translation,
                reference.rotation,
                reference.scale,
            ),
        });
    }

    StaticOccluderIndex::new(occluders)
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
struct EffectiveRefKey {
    cell: CellCoord,
    reference: (u32, u32),
}

fn effective_active_refs(
    active_plugins: &[Plugin],
    active_cells: &BTreeSet<CellCoord>,
) -> BTreeMap<EffectiveRefKey, tes3::esp::Reference> {
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
                    refs.insert(key, reference.clone());
                }
            }
        }
    }

    refs
}

pub(crate) struct UnclipReportContext {
    pub(crate) target_plugin: String,
    target_exterior_cells: usize,
    active_cells: usize,
    loaded_terrain_cells_total: usize,
    missing_active_terrain_cells: Vec<CellCoord>,
    pub(crate) write: Option<WriteReport>,
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

    pub(crate) fn missing_active_terrain_cells(&self) -> Vec<[i32; 2]> {
        self.missing_active_terrain_cells
            .iter()
            .map(|&(x, y)| [x, y])
            .collect()
    }

    pub(crate) fn summary(&self, inspection: &TerrainInspectionReport) -> UnclipSummary {
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
            refs_static_bounds_occluded: inspection.refs_static_bounds_occluded,
            refs_static_bounds_fully_occluded: inspection.refs_static_bounds_fully_occluded,
            refs_static_bounds_lightly_occluded: inspection.refs_static_bounds_lightly_occluded,
            refs_static_bounds_blocked: inspection.refs_static_bounds_blocked,
            origin_terrain_epsilon: ORIGIN_TERRAIN_EPSILON,
            mesh_contact_terrain_epsilon: CONTACT_TERRAIN_EPSILON,
        }
    }
}

#[cfg(test)]
impl UnclipReportContext {
    pub(crate) fn new_for_test(target_plugin: &str) -> Self {
        Self {
            target_plugin: target_plugin.to_owned(),
            target_exterior_cells: 0,
            active_cells: 0,
            loaded_terrain_cells_total: 0,
            missing_active_terrain_cells: Vec::new(),
            write: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct UnclipSummary {
    pub(crate) target_exterior_cells: usize,
    pub(crate) active_cells: usize,
    pub(crate) loaded_terrain_cells_total: usize,
    pub(crate) active_terrain_cells_loaded: usize,
    pub(crate) active_terrain_cells_missing: usize,
    pub(crate) refs: usize,
    pub(crate) refs_actionable: usize,
    pub(crate) refs_deleted: usize,
    pub(crate) refs_with_mesh_contact: usize,
    pub(crate) refs_without_resolved_static: usize,
    pub(crate) refs_missing_mesh_contact: usize,
    pub(crate) refs_with_terrain: usize,
    pub(crate) refs_missing_terrain: usize,
    pub(crate) refs_origin_above_terrain: usize,
    pub(crate) refs_origin_below_terrain: usize,
    pub(crate) refs_mesh_contact_above_terrain: usize,
    pub(crate) refs_mesh_contact_below_terrain: usize,
    pub(crate) refs_mesh_contact_missing_terrain: usize,
    pub(crate) refs_static_bounds_occluded: usize,
    pub(crate) refs_static_bounds_fully_occluded: usize,
    pub(crate) refs_static_bounds_lightly_occluded: usize,
    pub(crate) refs_static_bounds_blocked: usize,
    pub(crate) origin_terrain_epsilon: f32,
    pub(crate) mesh_contact_terrain_epsilon: f32,
}

#[derive(Serialize)]
pub(crate) struct ReferenceInspection {
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) id: String,
    pub(crate) static_resolution: &'static str,
    pub(crate) mesh_contact_status: &'static str,
    pub(crate) deleted: bool,
    pub(crate) write_status: &'static str,
    pub(crate) origin: OriginInspection,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) static_mesh: Option<StaticMeshInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mesh_contact_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mesh_contact: Option<MeshContactInspection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) static_bounds_occlusion: Option<StaticBoundsOcclusionInspection>,
}

#[derive(Serialize)]
pub(crate) struct OriginInspection {
    pub(crate) position: [f32; 3],
    pub(crate) terrain_z: Option<f32>,
    pub(crate) delta: Option<f32>,
    pub(crate) classification: &'static str,
}

#[derive(Serialize)]
pub(crate) struct StaticMeshInspection {
    pub(crate) id: String,
    pub(crate) mesh: String,
}

#[derive(Serialize)]
pub(crate) struct MeshContactInspection {
    pub(crate) position: [f32; 3],
    pub(crate) terrain_z: Option<f32>,
    pub(crate) delta: Option<f32>,
    pub(crate) classification: &'static str,
}

#[derive(Serialize)]
pub(crate) struct StaticBoundsOcclusionInspection {
    pub(crate) status: &'static str,
    pub(crate) ratio: f32,
}

#[derive(Default)]
pub(crate) struct TerrainInspectionReport {
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
    refs_static_bounds_occluded: usize,
    refs_static_bounds_fully_occluded: usize,
    refs_static_bounds_lightly_occluded: usize,
    refs_static_bounds_blocked: usize,
}

fn inspect_target_refs(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
    write: Option<&WriteReport>,
    mut reference_sink: impl FnMut(&ReferenceInspection) -> io::Result<()>,
) -> io::Result<TerrainInspectionReport> {
    let mut report = TerrainInspectionReport::default();
    let mut context = ReferenceInspectionContext {
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
    };

    for target_ref in sorted_exterior_refs(plugin) {
        inspect_reference(
            &mut report,
            &mut context,
            target_ref.cell,
            target_ref.key,
            target_ref.reference,
            write,
            &mut reference_sink,
        )?;
    }

    Ok(report)
}

fn count_target_refs(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    static_occluders: &StaticOccluderIndex,
) -> TerrainInspectionReport {
    let mut report = TerrainInspectionReport::default();
    let mut context = ReferenceInspectionContext {
        terrain,
        static_index,
        mesh_contacts,
        static_occluders,
    };

    for target_ref in sorted_exterior_refs(plugin) {
        count_reference(
            &mut report,
            &mut context,
            target_ref.cell,
            target_ref.reference,
        );
    }

    report
}

struct SortedReference<'a> {
    cell: CellCoord,
    key: (u32, u32),
    reference: &'a tes3::esp::Reference,
}

fn sorted_exterior_refs(plugin: &Plugin) -> Vec<SortedReference<'_>> {
    let mut cells = plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.data.grid);

    let mut sorted = Vec::new();
    for cell in cells {
        let mut references = cell.references.iter().collect::<Vec<_>>();
        references.sort_by_key(|(key, _)| **key);
        sorted.extend(
            references
                .into_iter()
                .map(|(key, reference)| SortedReference {
                    cell: cell.data.grid,
                    key: *key,
                    reference,
                }),
        );
    }

    sorted
}

struct ReferenceInspectionContext<'a, 'b> {
    terrain: &'a TerrainIndex,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'b>,
    static_occluders: &'a StaticOccluderIndex,
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
    );
    let origin = classify_origin(report, context.terrain, reference);
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
        let _ = classify_contact(report, context.terrain, reference, contact);
    }
    let _ = classify_static_bounds_occlusion(
        report,
        cell,
        context.terrain,
        reference,
        &mesh_contact,
        context.static_occluders,
    );
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

struct ReferenceInspectionInput<'a, 'b> {
    cell: CellCoord,
    key: (u32, u32),
    reference: &'a tes3::esp::Reference,
    origin: &'a OriginDetails,
    mesh_resolution: &'a MeshContactResolution<'b>,
    contact_details: Option<&'a ContactDetails>,
    static_bounds_occlusion: Option<&'a StaticBoundsOcclusionDetails>,
    write: WriteStatusEvidence,
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
) -> Option<StaticBoundsOcclusionDetails> {
    let MeshContactResolution::Resolved {
        contact, bounds, ..
    } = mesh_resolution
    else {
        return None;
    };
    let mut corrected_reference = reference.clone();
    let contact_position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    if let Some(terrain_z) = terrain.height_at(contact_position[0], contact_position[1]) {
        corrected_reference.translation[2] -= contact_position[2] - terrain_z;
    }
    let corrected_bounds = bounds.world_aabb(
        corrected_reference.translation,
        corrected_reference.rotation,
        corrected_reference.scale,
    );
    let action = decide_static_bounds_action(corrected_bounds, static_occluders);
    let (status, ratio) = match action {
        StaticBoundsAction::None => ("static_bounds_clear", 0.0),
        StaticBoundsAction::Delete { ratio, .. } => {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_fully_occluded += 1;
            ("static_bounds_fully_occluded", ratio)
        }
        StaticBoundsAction::Move { ratio, .. }
            if find_valid_relocation_position(
                cell,
                &corrected_reference,
                contact,
                *bounds,
                terrain,
                static_occluders,
            )
            .is_some() =>
        {
            report.refs_static_bounds_occluded += 1;
            report.refs_static_bounds_lightly_occluded += 1;
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

    match mesh_contacts.geometry(&static_mesh.mesh_path) {
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
    use std::collections::BTreeSet;

    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use super::{
        MeshContactResolution, apply_contact_adjustment, deleted_reference_inspection,
        effective_active_refs, write_status_label,
    };

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
    fn write_status_prefers_adjusted_plan_evidence() {
        let reference = reference_at_z(10.0);

        assert_eq!(
            write_status_label(
                &reference,
                &MeshContactResolution::UnresolvedStatic,
                None,
                true,
                false,
                false
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

        let refs = effective_active_refs(&[first, deleted], &BTreeSet::from([(0, 0)]));

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

        let refs = effective_active_refs(&[first, second], &BTreeSet::from([(0, 0), (1, 0)]));

        assert!(refs.is_empty());
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

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
