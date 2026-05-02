use std::{collections::BTreeSet, fmt::Write as FmtWrite, io, io::Write, path::PathBuf};

use tes3::esp::{Cell, Landscape, Plugin};

use crate::groundcover::openmw;

use super::{
    UnclipArgs,
    cells::{CellCoord, active_grid},
    terrain::TerrainIndex,
};

const ORIGIN_TERRAIN_EPSILON: f32 = 0.5;

pub fn run(args: &UnclipArgs, stdout: &mut dyn Write) -> io::Result<()> {
    let openmw_config = openmw::load_config_from_path(args.openmw_cfg.as_deref())?;
    let vfs = openmw::build_vfs(&openmw_config);
    let target_plugin_path = resolve_target_plugin(&args.plugin, &vfs)?;
    let target_plugin = load_target_plugin(&target_plugin_path)?;
    let content_files = openmw::content_files(&openmw_config)?;
    let terrain_plugin_paths = resolve_content_plugin_paths(&content_files, &vfs)?;
    let terrain_plugins = load_terrain_plugins(&terrain_plugin_paths)?;
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
    let report = inspect_target_refs(&target_plugin, &terrain, args.verbose);

    writeln!(stdout, "# greenmote unclip terrain inspection")?;
    writeln!(stdout, "# target plugin: {}", target_plugin_path.display())?;
    writeln!(stdout, "# target exterior cells: {}", target_cells.len())?;
    writeln!(stdout, "# active 3x3 cells: {}", active_cells.len())?;
    writeln!(stdout, "# loaded terrain cells total: {}", terrain.len())?;
    writeln!(
        stdout,
        "# active terrain cells loaded: {}",
        active_cells.len() - missing_active_terrain_cells.len()
    )?;
    writeln!(
        stdout,
        "# active terrain cells missing: {}",
        missing_active_terrain_cells.len()
    )?;
    writeln!(stdout, "# target refs: {}", report.refs)?;
    writeln!(stdout, "# refs with terrain: {}", report.refs_with_terrain)?;
    writeln!(
        stdout,
        "# refs missing terrain: {}",
        report.refs_missing_terrain
    )?;
    writeln!(
        stdout,
        "# refs above terrain: {}",
        report.refs_above_terrain
    )?;
    writeln!(
        stdout,
        "# refs below terrain: {}",
        report.refs_below_terrain
    )?;

    if args.verbose {
        for cell in &missing_active_terrain_cells {
            writeln!(stdout, "ACTIVE CELL {cell:?} missing terrain")?;
        }
        stdout.write_all(report.details.as_bytes())?;
    }

    Ok(())
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
    Plugin::from_path_filtered(path, |tag| &tag == Cell::TAG).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to load target plugin {}: {error}", path.display()),
        )
    })
}

fn load_terrain_plugins(paths: &[PathBuf]) -> io::Result<Vec<Plugin>> {
    paths
        .iter()
        .map(|path| {
            Plugin::from_path_filtered(path, |tag| &tag == Landscape::TAG).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("failed to load terrain from {}: {error}", path.display()),
                )
            })
        })
        .collect()
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

#[derive(Default)]
struct TerrainInspectionReport {
    refs: usize,
    refs_with_terrain: usize,
    refs_missing_terrain: usize,
    refs_above_terrain: usize,
    refs_below_terrain: usize,
    details: String,
}

fn inspect_target_refs(
    plugin: &Plugin,
    terrain: &TerrainIndex,
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
            let Some(terrain_z) = terrain.height_at(x, y) else {
                report.refs_missing_terrain += 1;
                if include_details {
                    let _ = writeln!(
                        report.details,
                        "CELL {:?} REF {:?} {} missing terrain at ({x:.2}, {y:.2})",
                        cell.data.grid, key, reference.id
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

            if include_details {
                let _ = writeln!(
                    report.details,
                    "CELL {:?} REF {:?} {} origin_z={z:.3} terrain_z={terrain_z:.3} origin_delta={delta:.3} classification={}",
                    cell.data.grid,
                    key,
                    reference.id,
                    classification.label()
                );
            }
        }
    }

    report
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
