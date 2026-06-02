// SPDX-License-Identifier: GPL-3.0-only

use std::{collections::HashSet, fs::File, io, io::Write, path::Path};

use openmw_config::OpenMWConfiguration;
use vfstool_lib::VFS;

use crate::groundcover::{
    DELETED_PLUGIN_NAME, GROUNDCOVER_PLUGIN_NAME, GroundcoverArgs, GroundcoverConfig, LOG_NAME,
    auto_enable, load, mesh, openmw, output,
    plan::{ConversionPlan, build_static_conversion_plan},
    progress::{self, CancellationToken, ConversionPhase, EventSink},
};

pub fn run(
    openmw_cfg: Option<&Path>,
    config_path_override: Option<&Path>,
    args: GroundcoverArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &EventSink<'_>,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    let mut stdin = io::stdin().lock();
    let openmw_config = openmw::load_config_with_prompt(openmw_cfg, "convert", &mut stdin, stderr)?;
    let greenmote_config_path = openmw::greenmote_config_path(config_path_override, &openmw_config);
    let persisted_openmw_cfg = openmw::persisted_config_path(&openmw_config);
    let output_directory = openmw::resolve_convert_output_directory(&openmw_config)?;
    let config = GroundcoverConfig::get(
        args,
        &greenmote_config_path,
        output_directory,
        Some(persisted_openmw_cfg),
    )?;

    if config.validate_config {
        writeln!(
            stdout,
            "Validated {} successfully",
            greenmote_config_path.display()
        )?;
        return Ok(());
    }

    run_loaded_config(openmw_config, &config, stdout, stderr, events, cancellation)
}

pub fn run_with_config(
    openmw_cfg: Option<&Path>,
    config: &GroundcoverConfig,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &EventSink<'_>,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    if config.validate_config {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pre-resolved conversion config must not request validate-config",
        ));
    }

    let openmw_config = openmw::load_config_from_path(openmw_cfg)?;
    run_loaded_config(openmw_config, config, stdout, stderr, events, cancellation)
}

fn run_loaded_config(
    mut openmw_config: OpenMWConfiguration,
    config: &GroundcoverConfig,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &EventSink<'_>,
    cancellation: &CancellationToken,
) -> io::Result<()> {
    check_cancelled(cancellation)?;
    let initial_enablement = auto_enable::status(&openmw_config);

    validate_auto_enable(&openmw_config, config, initial_enablement)?;
    check_cancelled(cancellation)?;

    let content_files = openmw::content_files(&openmw_config)?;
    let vfs = openmw::build_vfs(&openmw_config);
    check_cancelled(cancellation)?;

    let sources = load::resolve_source_plugins(&content_files, config, &vfs);
    progress::emit_phase(events, ConversionPhase::LoadingStaticPlugins);
    let static_load = load::load_plugins_for_static_planning(
        sources.clone(),
        &|current, total| {
            progress::emit_progress(
                events,
                ConversionPhase::LoadingStaticPlugins,
                current,
                total,
            );
        },
        cancellation,
    )?;
    write_load_warnings(stderr, &static_load.warnings)?;
    check_cancelled(cancellation)?;
    let skipped_generated_plugins = static_load
        .skipped_generated
        .iter()
        .map(|plugin| format!("{} at {}", plugin.plugin_name, plugin.plugin_path.display()))
        .collect::<Vec<_>>();
    let static_plugins = static_load.plugins;
    progress::emit_phase(events, ConversionPhase::PlanningStatics);
    let static_plan = build_static_conversion_plan(&static_plugins, config);
    check_cancelled(cancellation)?;
    let (loaded_plugins, cell_plans) = if static_plan.matched_source_ids.is_empty() {
        (static_plugins.len(), Vec::new())
    } else {
        progress::emit_phase(events, ConversionPhase::LoadingCellPlugins);
        progress::emit_phase(events, ConversionPhase::ScanningCells);
        let cell_scan = load::load_and_scan_plugins_for_cell_planning(
            sources,
            &static_plan.matched_source_ids,
            &|current, total| {
                progress::emit_progress(events, ConversionPhase::ScanningCells, current, total);
            },
            cancellation,
        )?;
        write_load_warnings(stderr, &cell_scan.warnings)?;
        check_cancelled(cancellation)?;
        ensure_static_sources_loaded_for_cell_scanning(
            &static_plan,
            &cell_scan.loaded_load_indices,
        )?;
        check_cancelled(cancellation)?;
        (cell_scan.loaded_plugins, cell_scan.cell_plans)
    };
    let plan = static_plan.with_cell_plans(cell_plans);
    check_cancelled(cancellation)?;
    progress::emit_phase(events, ConversionPhase::ResolvingMeshes);
    let mesh_paths = plan.used_mesh_paths()?;
    check_cancelled(cancellation)?;
    let summary = build_run_summary(
        content_files.len(),
        loaded_plugins,
        skipped_generated_plugins,
        &plan,
        mesh_paths.len(),
    );

    if config.debug {
        output::write_summary(&mut *stderr, &summary, &plan, config)?;
    }
    check_cancelled(cancellation)?;

    if config.dry_run {
        output::write_summary(&mut *stdout, &summary, &plan, config)?;
        check_cancelled(cancellation)?;
        return Ok(());
    }

    let (log_path, copied_meshes) = write_conversion_outputs(OutputWriteContext {
        stdout,
        openmw_config: &mut openmw_config,
        vfs: &vfs,
        config,
        plan: &plan,
        mesh_paths: &mesh_paths,
        summary: &summary,
        events,
        cancellation,
    })?;

    let output_directory_visible =
        auto_enable::output_directory_is_visible(&openmw_config, &config.output_directory);
    print_success(
        stdout,
        config,
        &log_path,
        copied_meshes,
        initial_enablement,
        output_directory_visible,
    )?;

    Ok(())
}

struct OutputWriteContext<'a, 'b> {
    stdout: &'a mut dyn Write,
    openmw_config: &'a mut openmw_config::OpenMWConfiguration,
    vfs: &'a VFS,
    config: &'a GroundcoverConfig,
    plan: &'a ConversionPlan,
    mesh_paths: &'a std::collections::BTreeSet<mesh::MeshCopyPath>,
    summary: &'a output::RunSummary,
    events: &'a EventSink<'b>,
    cancellation: &'a CancellationToken,
}

fn write_conversion_outputs(
    ctx: OutputWriteContext<'_, '_>,
) -> io::Result<(std::path::PathBuf, usize)> {
    let OutputWriteContext {
        stdout,
        openmw_config,
        vfs,
        config,
        plan,
        mesh_paths,
        summary,
        events,
        cancellation,
    } = ctx;

    let mesh_jobs = output::resolve_mesh_copy_jobs(vfs, mesh_paths, &config.output_directory)?;
    check_cancelled(cancellation)?;
    progress::emit_phase(events, ConversionPhase::WritingPlugins);
    let built = output::build_plugins(plan)?;
    check_cancelled(cancellation)?;
    output::save_plugins(built, config, cancellation)?;
    progress::emit_phase(events, ConversionPhase::CopyingMeshes);
    if let Err(error) = output::copy_meshes(
        &mesh_jobs,
        &|current, total| {
            progress::emit_progress(events, ConversionPhase::CopyingMeshes, current, total);
        },
        cancellation,
    ) {
        if is_cancelled_error(&error, cancellation) {
            write_cancellation_log(openmw_config, config, summary, plan)?;
        }
        return Err(error);
    }

    check_cancelled_after_output_side_effects(cancellation, openmw_config, config, summary, plan)?;
    run_auto_enable(stdout, openmw_config, config, events)?;

    check_cancelled_after_output_side_effects(cancellation, openmw_config, config, summary, plan)?;
    progress::emit_phase(events, ConversionPhase::WritingLog);
    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    check_cancelled_after_output_side_effects(cancellation, openmw_config, config, summary, plan)?;
    let mut log = File::create(&log_path)?;
    output::write_summary(&mut log, summary, plan, config)?;
    if cancellation.is_cancelled() {
        write_cancellation_notice(&mut log)?;
        return Err(cancelled_error());
    }

    Ok((log_path, mesh_jobs.len()))
}

fn check_cancelled(cancellation: &CancellationToken) -> io::Result<()> {
    if cancellation.is_cancelled() {
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

fn check_cancelled_after_output_side_effects(
    cancellation: &CancellationToken,
    openmw_config: &openmw_config::OpenMWConfiguration,
    config: &GroundcoverConfig,
    summary: &output::RunSummary,
    plan: &ConversionPlan,
) -> io::Result<()> {
    if cancellation.is_cancelled() {
        write_cancellation_log(openmw_config, config, summary, plan)?;
        Err(cancelled_error())
    } else {
        Ok(())
    }
}

fn write_cancellation_log(
    openmw_config: &openmw_config::OpenMWConfiguration,
    config: &GroundcoverConfig,
    summary: &output::RunSummary,
    plan: &ConversionPlan,
) -> io::Result<()> {
    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    let mut log = File::create(log_path)?;
    output::write_summary(&mut log, summary, plan, config)?;
    write_cancellation_notice(&mut log)
}

fn write_cancellation_notice(writer: &mut dyn Write) -> io::Result<()> {
    writeln!(writer)?;
    writeln!(
        writer,
        "Conversion cancelled after output writing started. Generated files, copied meshes, and OpenMW config may have been updated depending on the phase reached."
    )
}

fn is_cancelled_error(error: &io::Error, cancellation: &CancellationToken) -> bool {
    error.kind() == io::ErrorKind::Interrupted && cancellation.is_cancelled()
}

fn cancelled_error() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "conversion cancelled")
}

fn validate_auto_enable(
    openmw_config: &openmw_config::OpenMWConfiguration,
    config: &GroundcoverConfig,
    _enablement: auto_enable::OutputEnablement,
) -> io::Result<()> {
    if config.auto_enable && !config.dry_run {
        auto_enable::validate_output_directory(openmw_config, config)?;
    }

    Ok(())
}

fn run_auto_enable(
    stdout: &mut dyn Write,
    openmw_config: &mut openmw_config::OpenMWConfiguration,
    config: &GroundcoverConfig,
    events: &EventSink<'_>,
) -> io::Result<()> {
    if !config.auto_enable {
        return Ok(());
    }

    progress::emit_phase(events, ConversionPhase::AutoEnabling);
    let result = auto_enable::outputs(openmw_config)?;
    print_auto_enable_result(stdout, &result)
}

fn build_run_summary(
    content_files: usize,
    loaded_plugins: usize,
    skipped_generated_plugins: Vec<String>,
    plan: &crate::groundcover::plan::ConversionPlan,
    meshes_to_copy: usize,
) -> output::RunSummary {
    output::RunSummary {
        content_files,
        loaded_plugins,
        skipped_generated_plugins,
        matched_records: plan.static_plans.len(),
        used_records: plan.used_source_ids.len(),
        changed_cells: plan
            .cell_plans
            .iter()
            .map(|cell_plan| cell_plan.groundcover_cells.len())
            .sum(),
        touched_refs: plan
            .cell_plans
            .iter()
            .map(|cell_plan| cell_plan.touched_refs)
            .sum(),
        meshes_to_copy,
    }
}

fn write_load_warnings(
    writer: &mut dyn Write,
    warnings: &[load::PluginLoadWarning],
) -> io::Result<()> {
    for warning in warnings {
        writeln!(
            writer,
            "[ WARNING ]: Plugin {} could not be loaded: {}. Continuing.",
            warning.plugin_path.display(),
            warning.error
        )?;
    }

    Ok(())
}

fn ensure_static_sources_loaded_for_cell_scanning(
    static_plan: &crate::groundcover::plan::StaticConversionPlan,
    cell_load_indices: &[usize],
) -> io::Result<()> {
    let cell_load_indices = cell_load_indices.iter().copied().collect::<HashSet<_>>();
    let missing = static_plan
        .static_plans
        .iter()
        .filter(|plan| !cell_load_indices.contains(&plan.source_load_index))
        .map(|plan| plan.source_plugin_name.as_str())
        .collect::<Vec<_>>();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "plugins contributed matched records but could not be loaded for cell scanning: {}",
                missing.join(", ")
            ),
        ))
    }
}

fn print_success(
    writer: &mut dyn Write,
    config: &GroundcoverConfig,
    log_path: &Path,
    copied_meshes: usize,
    enablement: auto_enable::OutputEnablement,
    output_directory_visible: bool,
) -> io::Result<()> {
    writeln!(
        writer,
        "Generated {} and {} in {}",
        GROUNDCOVER_PLUGIN_NAME,
        DELETED_PLUGIN_NAME,
        config.output_directory.display()
    )?;
    writeln!(writer, "Copied {copied_meshes} meshes under Meshes/grass")?;
    writeln!(writer, "Wrote log to {}", log_path.display())?;
    if !config.auto_enable {
        print_manual_enablement_guidance(writer, config, enablement, output_directory_visible)?;
    }

    Ok(())
}

fn print_auto_enable_result(
    writer: &mut dyn Write,
    result: &auto_enable::AutoEnableResult,
) -> io::Result<()> {
    let Some(backup) = &result.backup else {
        writeln!(
            writer,
            "OpenMW config already enables generated plugins; no update needed."
        )?;
        return Ok(());
    };

    match (result.added_groundcover, result.added_deleted) {
        (true, true) => writeln!(
            writer,
            "Updated OpenMW config with {} as groundcover= and {} as content=; backup saved at {}",
            GROUNDCOVER_PLUGIN_NAME,
            DELETED_PLUGIN_NAME,
            backup.display()
        ),
        (true, false) => writeln!(
            writer,
            "Updated OpenMW config with {} as groundcover=; backup saved at {}",
            GROUNDCOVER_PLUGIN_NAME,
            backup.display()
        ),
        (false, true) => writeln!(
            writer,
            "Updated OpenMW config with {} as content=; backup saved at {}",
            DELETED_PLUGIN_NAME,
            backup.display()
        ),
        (false, false) => unreachable!("auto-enable backup without added outputs is meaningless"),
    }
}

fn print_manual_enablement_guidance(
    writer: &mut dyn Write,
    config: &GroundcoverConfig,
    enablement: auto_enable::OutputEnablement,
    output_directory_visible: bool,
) -> io::Result<()> {
    if output_directory_visible {
        writeln!(
            writer,
            "Output directory is already visible to OpenMW; no data-local or data= change is needed."
        )?;
    } else {
        writeln!(
            writer,
            "First make {} visible to OpenMW by setting it as data-local or adding it as data= in openmw.cfg.",
            config.output_directory.display()
        )?;
    }

    match (enablement.groundcover_enabled, enablement.deleted_enabled) {
        (true, true) => writeln!(
            writer,
            "Generated plugin entries are already present in openmw.cfg."
        ),
        (false, false) => writeln!(
            writer,
            "Add {GROUNDCOVER_PLUGIN_NAME} as groundcover= and {DELETED_PLUGIN_NAME} as content= in openmw.cfg."
        ),
        (false, true) => writeln!(
            writer,
            "Add {GROUNDCOVER_PLUGIN_NAME} as groundcover= in openmw.cfg."
        ),
        (true, false) => writeln!(
            writer,
            "Add {DELETED_PLUGIN_NAME} as content= in openmw.cfg."
        ),
    }
}
