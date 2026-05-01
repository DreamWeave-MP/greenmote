use std::{
    collections::HashSet,
    fs::{File, copy},
    io,
    path::{Path, PathBuf},
};

use openmw_config::OpenMWConfiguration;
use vfstool_lib::VFS;

use crate::groundcover::{
    GroundcoverArgs, GroundcoverConfig, LOG_NAME, load, output,
    plan::{build_static_conversion_plan, scan_cells_parallel},
};

pub fn run(args: GroundcoverArgs) -> io::Result<()> {
    let mut openmw_config = load_openmw_config(&args)?;
    let greenmote_config_path = greenmote_config_path(&args, &openmw_config);
    let default_output_directory = default_output_directory(&openmw_config);
    let config = GroundcoverConfig::get(
        args,
        &openmw_config.user_config_path(),
        default_output_directory,
    )?;

    if config.validate_config {
        println!("Validated {} successfully", greenmote_config_path.display());
        return Ok(());
    }

    if config.auto_enable {
        validate_auto_enable_output_directory(&openmw_config, &config)?;
    }

    let content_files = content_files(&openmw_config)?;
    let directories = openmw_config
        .data_directories_iter()
        .map(openmw_config::DirectorySetting::parsed)
        .collect::<Vec<_>>();
    let fallback_archives = openmw_config
        .fallback_archives_iter()
        .map(openmw_config::FileSetting::value)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let vfs = VFS::from_directories(directories, Some(fallback_archives));

    let sources = load::resolve_source_plugins(&content_files, &config, &vfs);
    let static_plugins = load::load_plugins_for_static_planning(sources.clone());
    let static_plan = build_static_conversion_plan(&static_plugins, &config)?;
    let (loaded_plugins, cell_plans) = if static_plan.matched_static_ids.is_empty() {
        (static_plugins.len(), Vec::new())
    } else {
        let cell_plugins = load::load_plugins_for_cell_scanning(sources);
        ensure_static_sources_loaded_for_cell_scanning(&static_plan, &cell_plugins)?;
        let cell_plans = scan_cells_parallel(&cell_plugins, &static_plan.matched_static_ids);
        (cell_plugins.len(), cell_plans)
    };
    let plan = static_plan.with_cell_plans(cell_plans);
    let summary = output::RunSummary {
        content_files: content_files.len(),
        loaded_plugins,
        matched_statics: plan.static_plans.len(),
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
        meshes_to_copy: plan.mesh_paths.len(),
    };

    if config.debug {
        output::write_summary(io::stderr(), &summary, &plan, &config)?;
    }

    if config.dry_run {
        output::write_summary(io::stdout(), &summary, &plan, &config)?;
        return Ok(());
    }

    let mesh_jobs =
        output::resolve_mesh_copy_jobs(&vfs, &plan.mesh_paths, &config.output_directory)?;
    let built = output::build_plugins(&plan)?;
    output::save_plugins(built, &config)?;
    output::copy_meshes(&mesh_jobs)?;

    if config.auto_enable {
        let backup = auto_enable_outputs(&mut openmw_config, &config)?;
        eprintln!(
            "Updated OpenMW config; backup saved at {}",
            backup.display()
        );
    }

    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    let mut log = File::create(&log_path)?;
    output::write_summary(&mut log, &summary, &plan, &config)?;

    print_success(&config, &log_path, mesh_jobs.len());

    Ok(())
}

fn get_config_path(args: &GroundcoverArgs) -> io::Result<PathBuf> {
    if let Some(path) = &args.openmw_cfg {
        let absolute_path = if path.is_relative() {
            path.canonicalize().unwrap_or_else(|_| path.to_owned())
        } else {
            path.to_owned()
        };

        if absolute_path.is_file()
            || (absolute_path.is_dir() && absolute_path.join("openmw.cfg").is_file())
        {
            return Ok(absolute_path);
        }

        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "explicit --openmw-cfg path {} is neither a file nor a directory containing openmw.cfg",
                path.display()
            ),
        ));
    }

    let cwd_cfg = std::env::current_dir()
        .expect("failed to get current directory")
        .join("openmw.cfg");
    if cwd_cfg.is_file() {
        return Ok(cwd_cfg);
    }

    Ok(openmw_config::default_config_path())
}

fn load_openmw_config(args: &GroundcoverArgs) -> io::Result<OpenMWConfiguration> {
    OpenMWConfiguration::new(Some(get_config_path(args)?)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read OpenMW configuration: {error}"),
        )
    })
}

fn greenmote_config_path(args: &GroundcoverArgs, config: &OpenMWConfiguration) -> PathBuf {
    args.config.clone().unwrap_or_else(|| {
        config
            .user_config_path()
            .join(crate::groundcover::DEFAULT_CONFIG_NAME)
    })
}

fn content_files(config: &OpenMWConfiguration) -> io::Result<Vec<String>> {
    let content_files = config
        .content_files_iter()
        .map(|plugin| plugin.value_str().to_owned())
        .collect::<Vec<_>>();

    if content_files.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "openmw.cfg has no content files",
        ))
    } else {
        Ok(content_files)
    }
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

fn default_output_directory(config: &OpenMWConfiguration) -> PathBuf {
    config
        .data_local()
        .map_or_else(openmw_config::default_data_local_path, |data_local| {
            data_local.parsed().to_owned()
        })
}

fn backup_openmw_cfg(openmw_cfg: &Path) -> io::Result<PathBuf> {
    let file_name = openmw_cfg.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "OpenMW user config path has no file name",
        )
    })?;
    let backup_name = format!("{}.greenmote.bak", file_name.to_string_lossy());
    let backup_path = openmw_cfg.with_file_name(backup_name);

    copy(openmw_cfg, &backup_path)?;

    Ok(backup_path)
}

fn validate_auto_enable_output_directory(
    config: &OpenMWConfiguration,
    groundcover_config: &GroundcoverConfig,
) -> io::Result<()> {
    if output_directory_is_visible(config, &groundcover_config.output_directory) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "refusing to auto-enable outputs in {} because it is not data-local or a configured data directory",
                groundcover_config.output_directory.display()
            ),
        ))
    }
}

fn auto_enable_outputs(
    config: &mut OpenMWConfiguration,
    groundcover_config: &GroundcoverConfig,
) -> io::Result<PathBuf> {
    let user_openmw_cfg = config.user_config_path().join("openmw.cfg");
    let backup = backup_openmw_cfg(&user_openmw_cfg)?;

    if !config.has_groundcover_file(&groundcover_config.groundcover_output) {
        config
            .add_groundcover_file(&groundcover_config.groundcover_output)
            .map_err(to_io_error)?;
    }

    if !config.has_content_file(&groundcover_config.deleted_output) {
        config
            .add_content_file(&groundcover_config.deleted_output)
            .map_err(to_io_error)?;
    }

    config.save_user().map_err(to_io_error)?;
    Ok(backup)
}

fn output_directory_is_visible(config: &OpenMWConfiguration, output_directory: &Path) -> bool {
    config
        .data_local()
        .is_some_and(|data_local| paths_equal(data_local.parsed(), output_directory))
        || config
            .data_directories_iter()
            .any(|data_dir| paths_equal(data_dir.parsed(), output_directory))
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn print_success(config: &GroundcoverConfig, log_path: &Path, copied_meshes: usize) {
    println!(
        "Generated {} and {} in {}",
        config.groundcover_output,
        config.deleted_output,
        config.output_directory.display()
    );
    println!("Copied {copied_meshes} meshes under Meshes/grass");
    println!("Wrote log to {}", log_path.display());
    if !config.auto_enable {
        println!(
            "Add {} as groundcover= and {} as content= in openmw.cfg.",
            config.groundcover_output, config.deleted_output
        );
    }
}

fn to_io_error<E: std::fmt::Display>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
