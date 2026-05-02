use std::{collections::BTreeSet, fmt::Write as FmtWrite, io, io::Write, path::PathBuf};

use serde::Serialize;
use tes3::esp::{Cell, Landscape, Plugin, Static};

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
    let target_plugin_path = resolve_target_plugin(&args.plugin, &vfs)?;
    let target_plugin = load_target_plugin(&target_plugin_path)?;
    let content_files = openmw::content_files(&openmw_config)?;
    let terrain_plugin_paths = resolve_content_plugin_paths(&content_files, &vfs)?;
    let terrain_plugins = load_terrain_plugins(&terrain_plugin_paths)?;
    let target_is_active = path_matches_any(&target_plugin_path, &terrain_plugin_paths);
    let static_index = build_static_index(
        &terrain_plugins,
        (!target_is_active).then_some(&target_plugin),
    );
    let terrain = TerrainIndex::from_landscapes(
        terrain_plugins
            .iter()
            .flat_map(tes3::esp::Plugin::objects_of_type::<Landscape>),
    );
    let target_cells = target_exterior_cells(&target_plugin);
    let active_cells = active_cells(&target_cells)?;
    let missing_active_terrain_cells = active_cells
        .iter()
        .copied()
        .filter(|cell| !terrain.has_cell(*cell))
        .collect::<Vec<_>>();
    let mut mesh_contacts = MeshContactCache::new(&vfs);
    let report = inspect_target_refs(
        &target_plugin,
        &terrain,
        &static_index,
        &mut mesh_contacts,
        args.verbose,
    );

    let output = UnclipReport::new(
        &target_plugin_path,
        target_cells.len(),
        active_cells.len(),
        terrain.len(),
        missing_active_terrain_cells,
        report,
    );
    write_output(stdout, &output, args)?;

    Ok(())
}

fn write_output(
    stdout: &mut dyn Write,
    report: &UnclipReport,
    args: &UnclipArgs,
) -> io::Result<()> {
    writeln!(
        stdout,
        "{}",
        vfstool_lib::serialize_value(report, args.format.serialize_type())?
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

fn resolve_target_plugin(plugin: &std::path::Path, vfs: &vfstool_lib::VFS) -> io::Result<PathBuf> {
    if plugin.is_file() {
        return Ok(plugin.to_path_buf());
    }

    let plugin_name = plugin.to_string_lossy();
    vfs.get_file(plugin_name.as_ref())
        .map(|file| file.path().to_path_buf())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "target plugin {} was not found as a file or VFS entry",
                    plugin.display()
                ),
            )
        })
}

fn load_target_plugin(path: &std::path::Path) -> io::Result<Plugin> {
    Plugin::from_path_filtered(path, |tag| &tag == Cell::TAG || &tag == Static::TAG).map_err(
        |error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to load target plugin {}: {error}", path.display()),
            )
        },
    )
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
struct UnclipReport {
    kind: &'static str,
    target_plugin: String,
    summary: UnclipSummary,
    missing_active_terrain_cells: Vec<[i32; 2]>,
    refs: Vec<ReferenceInspection>,
    #[serde(skip_serializing_if = "String::is_empty")]
    details: String,
}

impl UnclipReport {
    fn new(
        target_plugin_path: &std::path::Path,
        target_exterior_cells: usize,
        active_cells: usize,
        loaded_terrain_cells_total: usize,
        missing_active_terrain_cells: Vec<CellCoord>,
        inspection: TerrainInspectionReport,
    ) -> Self {
        let active_terrain_cells_missing = missing_active_terrain_cells.len();
        Self {
            kind: "greenmote_unclip_terrain_inspection",
            target_plugin: target_plugin_path.display().to_string(),
            summary: UnclipSummary {
                target_exterior_cells,
                active_cells,
                loaded_terrain_cells_total,
                active_terrain_cells_loaded: active_cells - active_terrain_cells_missing,
                active_terrain_cells_missing,
                refs: inspection.refs,
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
            },
            missing_active_terrain_cells: missing_active_terrain_cells
                .into_iter()
                .map(|(x, y)| [x, y])
                .collect(),
            refs: inspection.reference_inspections,
            details: inspection.details,
        }
    }
}

#[derive(Serialize)]
struct UnclipSummary {
    target_exterior_cells: usize,
    active_cells: usize,
    loaded_terrain_cells_total: usize,
    active_terrain_cells_loaded: usize,
    active_terrain_cells_missing: usize,
    refs: usize,
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
    origin: OriginInspection,
    #[serde(skip_serializing_if = "Option::is_none")]
    static_mesh: Option<StaticMeshInspection>,
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
    reference_inspections: Vec<ReferenceInspection>,
    details: String,
}

fn inspect_target_refs(
    plugin: &Plugin,
    terrain: &TerrainIndex,
    static_index: &StaticMeshIndex,
    mesh_contacts: &mut MeshContactCache<'_>,
    include_details: bool,
) -> TerrainInspectionReport {
    let mut report = TerrainInspectionReport::default();

    for cell in plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
    {
        for (key, reference) in &cell.references {
            report.refs += 1;
            let [x, y, z] = reference.translation;
            let mesh_contact = resolve_ref_mesh_contact(
                &mut report,
                cell.data.grid,
                *key,
                reference,
                static_index,
                mesh_contacts,
                include_details,
            );
            let contact_details = mesh_contact.as_ref().map(|(static_mesh, contact)| {
                classify_contact(&mut report, terrain, reference, static_mesh, contact)
            });
            let Some(terrain_z) = terrain.height_at(x, y) else {
                report.refs_missing_terrain += 1;
                report.reference_inspections.push(reference_inspection(
                    cell.data.grid,
                    *key,
                    reference,
                    None,
                    None,
                    "origin_missing_terrain",
                    contact_details.as_ref(),
                ));
                if include_details {
                    write_missing_origin_terrain_detail(
                        &mut report.details,
                        cell.data.grid,
                        *key,
                        reference,
                        contact_details.as_ref(),
                    );
                }
                continue;
            };

            report.refs_with_terrain += 1;
            let delta = z - terrain_z;
            let classification = classify_origin_delta(delta);
            match classification {
                OriginTerrainClassification::Above => {
                    report.refs_above_terrain += 1;
                }
                OriginTerrainClassification::Below => {
                    report.refs_below_terrain += 1;
                }
                OriginTerrainClassification::OnTerrain => {}
            }
            report.reference_inspections.push(reference_inspection(
                cell.data.grid,
                *key,
                reference,
                Some(terrain_z),
                Some(delta),
                classification.label(),
                contact_details.as_ref(),
            ));

            if include_details {
                if let Some(contact_details) = contact_details {
                    let [contact_x, contact_y, contact_z] = contact_details.position;
                    let _ = writeln!(
                        report.details,
                        "CELL {:?} REF {:?} {} static={} mesh={:?} origin_z={z:.3} origin_terrain_z={terrain_z:.3} origin_delta={delta:.3} origin_epsilon={ORIGIN_TERRAIN_EPSILON:.3} origin_classification={} contact=({:.3}, {:.3}, {:.3}) contact_terrain_z={} contact_delta={} contact_epsilon={CONTACT_TERRAIN_EPSILON:.3} contact_classification={}",
                        cell.data.grid,
                        key,
                        reference.id,
                        contact_details.static_mesh.static_id,
                        contact_details.static_mesh.mesh_path,
                        classification.label(),
                        contact_x,
                        contact_y,
                        contact_z,
                        optional_f32(contact_details.terrain_z),
                        optional_f32(contact_details.delta),
                        contact_details.classification.map_or(
                            "mesh_contact_missing_terrain",
                            ContactTerrainClassification::label
                        )
                    );
                } else {
                    let _ = writeln!(
                        report.details,
                        "CELL {:?} REF {:?} {} origin_z={z:.3} terrain_z={terrain_z:.3} origin_delta={delta:.3} origin_epsilon={ORIGIN_TERRAIN_EPSILON:.3} origin_classification={} contact_classification=unresolved",
                        cell.data.grid,
                        key,
                        reference.id,
                        classification.label()
                    );
                }
            }
        }
    }

    report
}

fn reference_inspection(
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    origin_terrain_z: Option<f32>,
    origin_delta: Option<f32>,
    origin_classification: &'static str,
    contact_details: Option<&ContactDetails<'_>>,
) -> ReferenceInspection {
    ReferenceInspection {
        cell: [cell.0, cell.1],
        reference_key: [key.0, key.1],
        id: reference.id.clone(),
        origin: OriginInspection {
            position: reference.translation,
            terrain_z: origin_terrain_z,
            delta: origin_delta,
            classification: origin_classification,
        },
        static_mesh: contact_details.map(|contact| StaticMeshInspection {
            id: contact.static_mesh.static_id.clone(),
            mesh: contact.static_mesh.mesh_path.clone(),
        }),
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

struct ContactDetails<'a> {
    static_mesh: &'a super::mesh::StaticMesh,
    position: [f32; 3],
    terrain_z: Option<f32>,
    delta: Option<f32>,
    classification: Option<ContactTerrainClassification>,
}

fn classify_contact<'a>(
    report: &mut TerrainInspectionReport,
    terrain: &TerrainIndex,
    reference: &tes3::esp::Reference,
    static_mesh: &'a super::mesh::StaticMesh,
    contact: &super::mesh::MeshContact,
) -> ContactDetails<'a> {
    let position =
        contact.world_position(reference.translation, reference.rotation, reference.scale);
    let terrain_z = terrain.height_at(position[0], position[1]);
    let delta = terrain_z.map(|terrain_z| position[2] - terrain_z);
    let classification = delta.map(|delta| classify_counted_contact_delta(report, delta));
    if terrain_z.is_none() {
        report.refs_contact_missing_terrain += 1;
    }

    ContactDetails {
        static_mesh,
        position,
        terrain_z,
        delta,
        classification,
    }
}

fn write_missing_origin_terrain_detail(
    details: &mut String,
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    contact_details: Option<&ContactDetails<'_>>,
) {
    if let Some(contact_details) = contact_details {
        let [contact_x, contact_y, contact_z] = contact_details.position;
        let _ = writeln!(
            details,
            "CELL {cell:?} REF {key:?} {} missing origin terrain at ({:.2}, {:.2}) static={} mesh={:?} contact=({contact_x:.3}, {contact_y:.3}, {contact_z:.3}) contact_terrain_z={} contact_delta={} contact_classification={}",
            reference.id,
            reference.translation[0],
            reference.translation[1],
            contact_details.static_mesh.static_id,
            contact_details.static_mesh.mesh_path,
            optional_f32(contact_details.terrain_z),
            optional_f32(contact_details.delta),
            contact_details.classification.map_or(
                "mesh_contact_missing_terrain",
                ContactTerrainClassification::label
            )
        );
    } else {
        let _ = writeln!(
            details,
            "CELL {cell:?} REF {key:?} {} missing origin terrain at ({:.2}, {:.2}) contact_classification=unresolved",
            reference.id, reference.translation[0], reference.translation[1]
        );
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

fn resolve_ref_mesh_contact<'a>(
    report: &mut TerrainInspectionReport,
    cell: CellCoord,
    key: (u32, u32),
    reference: &tes3::esp::Reference,
    static_index: &'a StaticMeshIndex,
    mesh_contacts: &'a mut MeshContactCache<'_>,
    include_details: bool,
) -> Option<(&'a super::mesh::StaticMesh, &'a super::mesh::MeshContact)> {
    let Some(static_mesh) = static_index.get(&reference.id) else {
        report.refs_without_resolved_static += 1;
        if include_details {
            let _ = writeln!(
                report.details,
                "CELL {cell:?} REF {key:?} {} has no resolved STAT mesh definition",
                reference.id
            );
        }
        return None;
    };

    match mesh_contacts.contact(&static_mesh.mesh_path) {
        Ok(contact) => {
            report.refs_with_mesh_contact += 1;
            Some((static_mesh, contact))
        }
        Err(error) => {
            report.refs_missing_mesh_contact += 1;
            if include_details {
                let _ = writeln!(
                    report.details,
                    "CELL {cell:?} REF {key:?} {} static={} mesh={:?} missing mesh contact: {error}",
                    reference.id, static_mesh.static_id, static_mesh.mesh_path
                );
            }
            None
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
