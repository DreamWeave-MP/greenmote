// SPDX-License-Identifier: GPL-3.0-only

//! End-to-end `greenmote unclip` flow: load, measure, decide, write, report.

use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs::File,
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use rayon::prelude::*;
use tes3::esp::{Landscape, Plugin};

use crate::groundcover::{CancellationToken, openmw};

use super::{
    args::UnclipPolicy,
    config::UnclipConfig,
    decide::{GeometryTable, RefInput, RefVerdict, VerdictCounts, decide},
    measure::{RefTransform, Surveyor},
    mesh::{MeshCache, StaticMeshIndex},
    patch::{
        OutputMode, WriteOutcome, apply_in_place, build_patch, default_patch_path, verify_written,
        write_plugin,
    },
    report::{self, CellSummary, OutputSummary, PolicySummary, Report},
    setup::{
        ContextPlugin, active_cells, build_static_index, load_context_plugins, load_target_plugin,
        path_matches_any, resolve_content_plugin_paths, resolve_target_plugin,
    },
    static_occluders::{NamedPlugin, StaticOccluderBuildReport, build_static_occluders},
    target::TargetRefIndex,
    terrain::{TerrainIndex, TerrainTextureIndex},
};

/// Log file written next to `openmw.cfg` on every run.
pub const UNCLIP_LOG_NAME: &str = "greenmote-unclip.log";

/// Runs unclip for one configuration.
///
/// # Errors
///
/// Returns configuration, plugin, mesh, and filesystem errors.
pub fn run(
    config: &UnclipConfig,
    stdout: &mut dyn Write,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    let check = || super::check_cancellation(cancellation);
    check()?;
    let policy = config
        .policy()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let openmw_config = openmw::load_config_from_path(config.openmw_cfg.as_deref())?;
    let vfs = openmw::build_vfs(&openmw_config);
    let source_path = resolve_target_plugin(&config.plugin, &vfs)?;
    let mut source = load_target_plugin(&source_path)?;
    check()?;

    let target_refs = TargetRefIndex::build(&source, &policy, cancellation)?;
    let mode = output_mode(config, &openmw_config, &source_path);
    let mut report = Report {
        target: source_path.clone(),
        mode: describe_mode(config.write, &mode, &source_path),
        policy: PolicySummary::from_policy(&policy),
        cells: CellSummary {
            target_exterior_refs: target_refs.exterior_ref_count,
            target_cells: target_refs.target_cells.len(),
            ..CellSummary::default()
        },
        occluders: StaticOccluderBuildReport::default(),
        counts: VerdictCounts::default(),
        write: None,
        refs: None,
    };

    let verdicts = if target_refs.target_cells.is_empty() {
        Vec::new()
    } else {
        let world = load_world(
            &openmw_config,
            &vfs,
            &source_path,
            &source,
            &target_refs,
            &policy,
            cancellation,
        )?;
        report.cells.active_cells = world.active_cells.len();
        report.cells.terrain_cells_loaded = world.terrain.len();
        report.cells.terrain_cells_missing = world
            .active_cells
            .iter()
            .filter(|cell| !world.terrain.has_cell(**cell))
            .map(|cell| [cell.0, cell.1])
            .collect();
        report.occluders = world.occluder_report;
        if !report.occluders.missing_meshes.is_empty() && !config.ignore_missing_meshes {
            return Err(missing_meshes_error(&report.occluders));
        }
        check()?;

        let surveyor = Surveyor {
            terrain: &world.terrain,
            textures: &world.textures,
            occluders: &world.occluders,
            policy: &policy,
        };
        let inputs = target_refs
            .iter_ref_entries(&source)
            .map(|entry| RefInput {
                cell: entry.cell,
                key: entry.key,
                id: &entry.reference.id,
                deleted: entry.reference.deleted == Some(true),
                transform: RefTransform {
                    translation: entry.reference.translation,
                    rotation: entry.reference.rotation,
                    scale: entry.reference.scale,
                },
                geometry: world.geometry.get(entry.normalized_id),
            })
            .collect::<Vec<_>>();
        let verdicts = inputs
            .par_iter()
            .map(|input| decide(input, &surveyor))
            .collect::<Vec<_>>();
        check()?;
        verdicts
    };
    report.counts = VerdictCounts::from_verdicts(&verdicts);

    if config.write && report.counts.fix + report.counts.delete > 0 {
        report.write = Some(write_changes(&mut source, &source_path, &mode, &verdicts)?);
    }

    write_log(&openmw_config, &report, &verdicts, config.verbose)?;
    if config.verbose && config.structured {
        report.refs = Some(verdicts);
    }
    if config.structured {
        report::write_json(stdout, &report)
    } else {
        report::write_text(stdout, &report)
    }
}

struct World {
    active_cells: BTreeSet<(i32, i32)>,
    terrain: TerrainIndex,
    textures: TerrainTextureIndex,
    occluders: super::occlusion::StaticOccluderIndex,
    occluder_report: StaticOccluderBuildReport,
    geometry: GeometryTable,
}

fn load_world(
    openmw_config: &openmw_config::OpenMWConfiguration,
    vfs: &vfstool_lib::VFS,
    source_path: &Path,
    source: &Plugin,
    target_refs: &TargetRefIndex,
    policy: &UnclipPolicy,
    cancellation: &CancellationToken,
) -> io::Result<World> {
    let context_paths = resolve_content_plugin_paths(&openmw::content_files(openmw_config)?, vfs)?;
    let context_plugins = load_context_plugins(&context_paths, source_path, source, cancellation)?;
    let target_is_active = path_matches_any(source_path, &context_paths);
    let active_static_index =
        build_static_index(context_plugins.iter().map(ContextPlugin::as_plugin), None);
    let target_static_index: Cow<'_, StaticMeshIndex> = if target_is_active {
        Cow::Borrowed(&active_static_index)
    } else {
        Cow::Owned(build_static_index(
            context_plugins.iter().map(ContextPlugin::as_plugin),
            Some(source),
        ))
    };
    let active_cells = active_cells(&target_refs.target_cells)?;
    let terrain = TerrainIndex::from_landscapes_in_cells(
        context_plugins
            .iter()
            .map(ContextPlugin::as_plugin)
            .flat_map(Plugin::objects_of_type::<Landscape>),
        &active_cells,
    );
    let textures = TerrainTextureIndex::from_plugins_in_cells(
        context_plugins.iter().map(ContextPlugin::as_plugin),
        &active_cells,
    );
    super::check_cancellation(cancellation)?;

    let mut mesh_cache = MeshCache::new(vfs);
    let mut geometry = GeometryTable::new();
    for id in &target_refs.target_static_ids {
        let Some(static_mesh) = target_static_index.get_normalized_key(id) else {
            continue;
        };
        let loaded = mesh_cache
            .geometry(static_mesh)
            .map(|mesh| Arc::new(mesh.clone()))
            .map_err(|error| error.to_string());
        geometry.insert(id.clone(), loaded);
    }
    super::check_cancellation(cancellation)?;

    let named = context_paths
        .iter()
        .zip(&context_plugins)
        .map(|(path, plugin)| NamedPlugin {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .unwrap_or_default(),
            plugin: plugin.as_plugin(),
        })
        .collect::<Vec<_>>();
    let (occluders, occluder_report) = build_static_occluders(
        &named,
        &active_cells,
        &active_static_index,
        &mut mesh_cache,
        &target_refs.target_static_ids,
        &policy.occluder_filter,
        cancellation,
    )?;

    Ok(World {
        active_cells,
        terrain,
        textures,
        occluders,
        occluder_report,
        geometry,
    })
}

fn output_mode(
    config: &UnclipConfig,
    openmw_config: &openmw_config::OpenMWConfiguration,
    source_path: &Path,
) -> OutputMode {
    if config.in_place {
        return OutputMode::InPlace;
    }
    if let Some(path) = &config.output_plugin {
        return OutputMode::Patch { path: path.clone() };
    }
    let directory = openmw_config
        .data_local()
        .map(|setting| setting.parsed().to_owned())
        .or_else(|| source_path.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    OutputMode::Patch {
        path: default_patch_path(&directory, source_path),
    }
}

fn describe_mode(write: bool, mode: &OutputMode, source_path: &Path) -> OutputSummary {
    match (write, mode) {
        (false, OutputMode::Patch { path }) => OutputSummary::DryRun {
            would_write: path.clone(),
            in_place: false,
        },
        (false, OutputMode::InPlace) => OutputSummary::DryRun {
            would_write: source_path.to_path_buf(),
            in_place: true,
        },
        (true, OutputMode::Patch { path }) => OutputSummary::Patch { path: path.clone() },
        (true, OutputMode::InPlace) => OutputSummary::InPlace {
            path: source_path.to_path_buf(),
        },
    }
}

fn write_changes(
    source: &mut Plugin,
    source_path: &Path,
    mode: &OutputMode,
    verdicts: &[RefVerdict],
) -> io::Result<WriteOutcome> {
    let refs_fixed = verdicts
        .iter()
        .filter(|entry| matches!(entry.verdict, super::decide::Verdict::Fix(_)))
        .count();
    let refs_deleted = verdicts
        .iter()
        .filter(|entry| matches!(entry.verdict, super::decide::Verdict::Delete { .. }))
        .count();
    let cells = verdicts
        .iter()
        .filter(|entry| entry.verdict.changes_plugin())
        .map(|entry| entry.cell)
        .collect::<BTreeSet<_>>()
        .len();

    let (path, in_place, backups, masters) = match mode {
        OutputMode::Patch { path } => {
            let source_file_name = source_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let source_size = std::fs::metadata(source_path).map(|meta| meta.len())?;
            let mut patch = build_patch(source, &source_file_name, source_size, verdicts)?;
            let masters = patch
                .header()
                .map(|header| {
                    header
                        .masters
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect()
                })
                .unwrap_or_default();
            let backups = write_plugin(&mut patch, path, false)?;
            (path.clone(), false, backups, masters)
        }
        OutputMode::InPlace => {
            apply_in_place(source, verdicts);
            let masters = source
                .header()
                .map(|header| {
                    header
                        .masters
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect()
                })
                .unwrap_or_default();
            let backups = write_plugin(source, source_path, true)?;
            (source_path.to_path_buf(), true, backups, masters)
        }
    };
    verify_written(&path, verdicts, in_place)?;

    Ok(WriteOutcome {
        path,
        in_place,
        backups,
        refs_fixed,
        refs_deleted,
        cells,
        masters,
        verified: true,
    })
}

fn write_log(
    openmw_config: &openmw_config::OpenMWConfiguration,
    report: &Report,
    verdicts: &[RefVerdict],
    verbose: bool,
) -> io::Result<()> {
    let log_path: PathBuf = openmw_config.user_config_path().join(UNCLIP_LOG_NAME);
    let mut log = BufWriter::new(File::create(log_path)?);
    report::write_text(&mut log, report)?;
    if verbose {
        writeln!(log)?;
        report::write_ref_lines(&mut log, verdicts)?;
    }
    log.flush()
}

fn missing_meshes_error(report: &StaticOccluderBuildReport) -> io::Error {
    let mut lines = report
        .missing_meshes
        .iter()
        .take(20)
        .map(|missing| {
            format!(
                "  {} ({}): {}",
                missing.mesh_path, missing.static_id, missing.error
            )
        })
        .collect::<Vec<_>>();
    if report.missing_meshes.len() > 20 {
        lines.push(format!(
            "  ... and {} more",
            report.missing_meshes.len() - 20
        ));
    }
    io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "{} static occluder meshes could not be loaded, so clipping into those statics cannot be detected. Fix the load order or pass --ignore-missing-meshes to continue without them:\n{}",
            report.missing_meshes.len(),
            lines.join("\n")
        ),
    )
}
