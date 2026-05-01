use std::{collections::HashSet, fs::File, io, io::Write, path::Path};

use crate::groundcover::{
    GroundcoverArgs, GroundcoverConfig, LOG_NAME, auto_enable, load, openmw, output,
    plan::{build_static_conversion_plan, scan_cells_parallel},
    progress::{self, ConversionPhase, EventSink},
};

pub fn run(
    args: GroundcoverArgs,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    events: &EventSink<'_>,
) -> io::Result<()> {
    let mut openmw_config = openmw::load_config(&args)?;
    let greenmote_config_path = openmw::greenmote_config_path(&args, &openmw_config);
    let default_output_directory = openmw::default_output_directory(&openmw_config);
    let config = GroundcoverConfig::get(
        args,
        &openmw_config.user_config_path(),
        default_output_directory,
    )?;

    if config.validate_config {
        writeln!(
            stdout,
            "Validated {} successfully",
            greenmote_config_path.display()
        )?;
        return Ok(());
    }

    if config.auto_enable {
        auto_enable::validate_output_directory(&openmw_config, &config)?;
    }

    let content_files = openmw::content_files(&openmw_config)?;
    let vfs = openmw::build_vfs(&openmw_config);

    let sources = load::resolve_source_plugins(&content_files, &config, &vfs);
    progress::emit_phase(events, ConversionPhase::LoadingStaticPlugins);
    let static_load = load::load_plugins_for_static_planning(sources.clone(), &|current, total| {
        progress::emit_progress(
            events,
            ConversionPhase::LoadingStaticPlugins,
            current,
            total,
        );
    });
    write_load_warnings(stderr, &static_load.warnings)?;
    let skipped_generated_plugins = static_load
        .skipped_generated
        .iter()
        .map(|plugin| format!("{} at {}", plugin.plugin_name, plugin.plugin_path.display()))
        .collect::<Vec<_>>();
    let static_plugins = static_load.plugins;
    progress::emit_phase(events, ConversionPhase::PlanningStatics);
    let static_plan = build_static_conversion_plan(&static_plugins, &config);
    let (loaded_plugins, cell_plans) = if static_plan.matched_static_ids.is_empty() {
        (static_plugins.len(), Vec::new())
    } else {
        progress::emit_phase(events, ConversionPhase::LoadingCellPlugins);
        let cell_load = load::load_plugins_for_cell_scanning(sources, &|current, total| {
            progress::emit_progress(events, ConversionPhase::LoadingCellPlugins, current, total);
        });
        write_load_warnings(stderr, &cell_load.warnings)?;
        let cell_plugins = cell_load.plugins;
        ensure_static_sources_loaded_for_cell_scanning(&static_plan, &cell_plugins)?;
        progress::emit_phase(events, ConversionPhase::ScanningCells);
        let cell_plans = scan_cells_parallel(
            &cell_plugins,
            &static_plan.matched_static_ids,
            &|current, total| {
                progress::emit_progress(events, ConversionPhase::ScanningCells, current, total);
            },
        );
        (cell_plugins.len(), cell_plans)
    };
    let plan = static_plan.with_cell_plans(cell_plans);
    progress::emit_phase(events, ConversionPhase::ResolvingMeshes);
    let mesh_paths = plan.used_mesh_paths()?;
    let summary = build_run_summary(
        content_files.len(),
        loaded_plugins,
        skipped_generated_plugins,
        &plan,
        mesh_paths.len(),
    );

    if config.debug {
        output::write_summary(&mut *stderr, &summary, &plan, &config)?;
    }

    if config.dry_run {
        output::write_summary(&mut *stdout, &summary, &plan, &config)?;
        return Ok(());
    }

    let mesh_jobs = output::resolve_mesh_copy_jobs(&vfs, &mesh_paths, &config.output_directory)?;
    progress::emit_phase(events, ConversionPhase::WritingPlugins);
    let built = output::build_plugins(&plan)?;
    output::save_plugins(built, &config)?;
    progress::emit_phase(events, ConversionPhase::CopyingMeshes);
    output::copy_meshes(&mesh_jobs, &|current, total| {
        progress::emit_progress(events, ConversionPhase::CopyingMeshes, current, total);
    })?;

    if config.auto_enable {
        progress::emit_phase(events, ConversionPhase::AutoEnabling);
        let backup = auto_enable::outputs(&mut openmw_config, &config)?;
        writeln!(
            stdout,
            "Updated OpenMW config; backup saved at {}",
            backup.display()
        )?;
    }

    progress::emit_phase(events, ConversionPhase::WritingLog);
    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    let mut log = File::create(&log_path)?;
    output::write_summary(&mut log, &summary, &plan, &config)?;

    print_success(stdout, &config, &log_path, mesh_jobs.len())?;

    Ok(())
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
        matched_statics: plan.static_plans.len(),
        used_statics: plan.used_static_ids.len(),
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
    cell_plugins: &[crate::groundcover::plan::LoadedPlugin],
) -> io::Result<()> {
    let cell_load_indices = cell_plugins
        .iter()
        .map(|plugin| plugin.load_index)
        .collect::<HashSet<_>>();
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
                "plugins contributed matched statics but could not be loaded for cell scanning: {}",
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
) -> io::Result<()> {
    writeln!(
        writer,
        "Generated {} and {} in {}",
        config.groundcover_output,
        config.deleted_output,
        config.output_directory.display()
    )?;
    writeln!(writer, "Copied {copied_meshes} meshes under Meshes/grass")?;
    writeln!(writer, "Wrote log to {}", log_path.display())?;
    if !config.auto_enable {
        writeln!(
            writer,
            "Add {} as groundcover= and {} as content= in openmw.cfg.",
            config.groundcover_output, config.deleted_output
        )?;
    }

    Ok(())
}
