// SPDX-License-Identifier: MIT OR Apache-2.0

//! End-to-end `greenmote unclip` flow: load, measure, decide, write, report.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
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
    report::{self, CellSummary, OutputSummary, PolicySummary, Report},
    setup::{
        ContextPlugin, active_cells, build_static_index, load_context_plugins, load_target_plugin,
        path_matches_any, resolve_content_plugin_paths, resolve_target_plugin,
    },
    static_occluders::{NamedPlugin, StaticOccluderBuildReport, build_static_occluders},
    target::TargetRefIndex,
    terrain::{TerrainIndex, TerrainTextureIndex},
    write::{OutputMode, WriteOutcome, apply_changes, verify_written, write_plugin},
};

/// Log file written next to `openmw.cfg` on every run.
pub const UNCLIP_LOG_NAME: &str = "greenmote-unclip.log";

/// Set this environment variable to print phase timings to stderr.
pub const PROFILE_ENV: &str = "GREENMOTE_PROFILE";

struct Profiler {
    enabled: bool,
    last: std::time::Instant,
    start: std::time::Instant,
}

impl Profiler {
    fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            enabled: std::env::var_os(PROFILE_ENV).is_some(),
            last: now,
            start: now,
        }
    }

    fn phase(&mut self, name: &str) {
        if self.enabled {
            let now = std::time::Instant::now();
            eprintln!(
                "[profile] {name:<28} {:8.3}s  (total {:7.3}s)",
                (now - self.last).as_secs_f64(),
                (now - self.start).as_secs_f64()
            );
            self.last = now;
        }
    }
}

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
    let mut profiler = Profiler::new();
    check()?;
    let policy = config
        .policy()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let openmw_config = openmw::load_config_from_path(config.openmw_cfg.as_deref())?;
    let plugin_directories = unlisted_plugin_directories(&config.plugin, &openmw_config);
    let vfs = openmw::build_vfs_with_extra_directories(&openmw_config, &plugin_directories);
    let source_path = resolve_target_plugin(&config.plugin, &vfs)?;
    let mut source = load_target_plugin(&source_path)?;
    profiler.phase("config, vfs, target plugin");
    check()?;

    let target_refs = TargetRefIndex::build(&source, &policy, cancellation)?;
    let mode = output_mode(config);
    let mut report = Report {
        target: source_path.clone(),
        added_data_directories: plugin_directories,
        mode: describe_mode(config.write, &mode, &source_path),
        policy: PolicySummary::from_policy(&policy),
        cells: CellSummary {
            target_exterior_refs: target_refs.exterior_ref_count,
            target_cells: target_refs.target_cells.len(),
            ..CellSummary::default()
        },
        occluders: StaticOccluderBuildReport::default(),
        counts: VerdictCounts::default(),
        mesh_errors: BTreeMap::new(),
        write: None,
        refs: None,
    };

    let verdicts = if target_refs.target_cells.is_empty() {
        Vec::new()
    } else {
        let world = load_world(
            &mut profiler,
            &WorldInput {
                openmw_config: &openmw_config,
                vfs: &vfs,
                source_path: &source_path,
                source: &source,
                target_refs: &target_refs,
                policy: &policy,
                cancellation,
            },
        )?;
        report.cells.active_cells = world.active_cells.len();
        report.cells.terrain_cells_loaded = world.terrain.len();
        report.cells.terrain_cells_missing = world
            .active_cells
            .iter()
            .filter(|cell| !world.terrain.has_cell(**cell))
            .map(|cell| [cell.0, cell.1])
            .collect();
        report.occluders = world.occluder_report.clone();
        check()?;

        let verdicts = decide_all(&world, &policy, &source, &target_refs);
        profiler.phase("decide");
        check()?;
        verdicts
    };
    report.counts = VerdictCounts::from_verdicts(&verdicts);
    report.mesh_errors = mesh_errors(&verdicts);

    // In place, an unchanged plugin is left untouched. With --output-plugin the caller asked
    // for a file, so an unchanged copy is still written rather than silently nothing.
    let has_changes = report.counts.fix + report.counts.delete > 0;
    if config.write && (has_changes || matches!(mode, OutputMode::Copy { .. })) {
        report.write = Some(
            write_changes(&mut source, &source_path, &mode, &verdicts)
                .map_err(super::write::WriteFailure::wrap)?,
        );
        profiler.phase("write and verify");
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

/// Decides every target reference in parallel; the verdict order follows the target index.
fn decide_all(
    world: &World,
    policy: &UnclipPolicy,
    source: &Plugin,
    target_refs: &TargetRefIndex,
) -> Vec<RefVerdict> {
    let surveyor = Surveyor {
        terrain: &world.terrain,
        textures: &world.textures,
        occluders: &world.occluders,
        policy,
    };
    let inputs = target_refs
        .iter_ref_entries(source)
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
    inputs
        .par_iter()
        .map(|input| decide(input, &surveyor))
        .collect()
}

struct WorldInput<'a> {
    openmw_config: &'a openmw_config::OpenMWConfiguration,
    vfs: &'a vfstool_lib::VFS,
    source_path: &'a Path,
    source: &'a Plugin,
    target_refs: &'a TargetRefIndex,
    policy: &'a UnclipPolicy,
    cancellation: &'a CancellationToken,
}

fn load_world(profiler: &mut Profiler, input: &WorldInput<'_>) -> io::Result<World> {
    let WorldInput {
        openmw_config,
        vfs,
        source_path,
        source,
        target_refs,
        policy,
        cancellation,
    } = *input;
    let context_paths = resolve_content_plugin_paths(&openmw::content_files(openmw_config)?, vfs)?;
    let context_plugins = load_context_plugins(&context_paths, source_path, source, cancellation)?;
    profiler.phase("context plugins");
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
    let mut textures = TerrainTextureIndex::from_plugins_in_cells(
        context_plugins.iter().map(ContextPlugin::as_plugin),
        &active_cells,
    );
    textures.precompute_roads(&policy.road_texture_filter);
    profiler.phase("statics, terrain, textures");
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
    profiler.phase("grass meshes");
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
    profiler.phase("occluders");

    Ok(World {
        active_cells,
        terrain,
        textures,
        occluders,
        occluder_report,
        geometry,
    })
}

fn mesh_errors(verdicts: &[RefVerdict]) -> BTreeMap<String, usize> {
    let mut errors = BTreeMap::new();
    for entry in verdicts {
        if let super::decide::Verdict::Skip {
            reason: super::decide::SkipReason::MeshError { error },
        } = &entry.verdict
        {
            *errors.entry(error.clone()).or_default() += 1;
        }
    }
    errors
}

/// When `--plugin` is a file outside every configured data directory, the folders that hold its
/// mod's `meshes/` are added to the VFS so the plugin's grass resolves as it would once the mod is
/// enabled.
///
/// Starting at the plugin's directory and walking up to two levels, the first level that offers
/// a folder with a `meshes` directory (the level's folder itself or one of its immediate
/// sub-folders) wins. That covers mods that keep plugins beside `meshes/` (Fantasia), below it
/// (Aesthesia), or in a sibling option folder (Remiros' `00 Core OpenMW` next to `03 TR Plugins`).
/// If any of those folders is already a configured data directory the mod counts as enabled and
/// nothing is added. Folders whose name mentions `OpenMW` are added last so they win over other
/// engine variants of the same mod.
fn unlisted_plugin_directories(
    plugin: &Path,
    openmw_config: &openmw_config::OpenMWConfiguration,
) -> Vec<PathBuf> {
    if !plugin.is_file() {
        return Vec::new();
    }
    let Some(parent) = plugin
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Vec::new();
    };
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let listed = openmw_config
        .data_directories_iter()
        .map(|setting| canonical(setting.parsed()))
        .collect::<BTreeSet<_>>();

    for ancestor in parent.ancestors().take(3) {
        let mut candidates = std::fs::read_dir(ancestor)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                    .map(|entry| entry.path())
                    .filter(|path| has_meshes_directory(path))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if has_meshes_directory(ancestor) {
            candidates.push(ancestor.to_path_buf());
        }
        if candidates.is_empty() {
            continue;
        }
        if candidates
            .iter()
            .any(|candidate| listed.contains(&canonical(candidate)))
        {
            return Vec::new();
        }
        candidates.sort_by_key(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            (name.contains("openmw"), name)
        });
        return candidates;
    }
    Vec::new()
}

fn has_meshes_directory(directory: &Path) -> bool {
    std::fs::read_dir(directory).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry.file_name().eq_ignore_ascii_case("meshes")
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
        })
    })
}

fn output_mode(config: &UnclipConfig) -> OutputMode {
    match &config.output_plugin {
        Some(path) => OutputMode::Copy { path: path.clone() },
        None => OutputMode::InPlace,
    }
}

fn describe_mode(write: bool, mode: &OutputMode, source_path: &Path) -> OutputSummary {
    let path = match mode {
        OutputMode::InPlace => source_path.to_path_buf(),
        OutputMode::Copy { path } => path.clone(),
    };
    if write {
        OutputSummary::Write { path }
    } else {
        OutputSummary::DryRun { would_write: path }
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

    apply_changes(source, verdicts);
    let (path, replaced_source) = match mode {
        OutputMode::InPlace => (source_path.to_path_buf(), true),
        OutputMode::Copy { path } => (path.clone(), false),
    };
    let backups = write_plugin(source, &path, replaced_source)?;
    verify_written(&path, verdicts)?;

    Ok(WriteOutcome {
        path,
        replaced_source,
        backups,
        refs_fixed,
        refs_deleted,
        cells,
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
