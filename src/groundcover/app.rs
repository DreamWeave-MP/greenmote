use std::{
    fs::{File, copy},
    io,
    path::{Path, PathBuf},
};

use openmw_config::OpenMWConfiguration;
use vfstool_lib::VFS;

use crate::groundcover::{
    GroundcoverArgs, GroundcoverConfig, LOG_NAME, handle_generated_output, load, output,
    plan::build_conversion_plan,
};

pub fn run(args: GroundcoverArgs) -> io::Result<()> {
    if handle_generated_output(&args, &mut io::stdout())? {
        return Ok(());
    }

    let selected_config_file = selected_config_file_path(&args);
    let mut openmw_config = load_openmw_config(&args)?;
    let default_output_directory = default_output_directory(&openmw_config);
    let config = GroundcoverConfig::get(
        args,
        &openmw_config.user_config_path(),
        default_output_directory,
    )?;

    if config.validate_config {
        println!(
            "Validated {} successfully",
            openmw_config
                .user_config_path()
                .join(crate::groundcover::DEFAULT_CONFIG_NAME)
                .display()
        );
        return Ok(());
    }

    let content_files = content_files(&openmw_config)?;
    let directories = openmw_config
        .data_directories_iter()
        .map(openmw_config::DirectorySetting::parsed)
        .collect::<Vec<_>>();
    let vfs = VFS::from_directories(directories, None);

    let sources = load::resolve_source_plugins(&content_files, &config, &vfs);
    let plugins = load::load_plugins(sources);
    let plan = build_conversion_plan(&plugins, &config);
    let summary = output::RunSummary {
        content_files: content_files.len(),
        loaded_plugins: plugins.len(),
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
        output::write_summary(io::stderr(), &summary, &plan)?;
    }

    if config.dry_run {
        output::write_summary(io::stdout(), &summary, &plan)?;
        return Ok(());
    }

    let mut built = output::build_plugins(&plan);
    output::add_masters(&mut built, &plan)?;
    output::save_plugins(built, &config)?;

    let mesh_jobs =
        output::resolve_mesh_copy_jobs(&vfs, &plan.mesh_paths, &config.output_directory);
    output::copy_meshes(&mesh_jobs)?;

    if config.auto_enable {
        auto_enable_outputs(&mut openmw_config, &config, &selected_config_file)?;
    }

    let log_path = openmw_config.user_config_path().join(LOG_NAME);
    let mut log = File::create(log_path)?;
    output::write_summary(&mut log, &summary, &plan)?;

    Ok(())
}

fn selected_config_file_path(args: &GroundcoverArgs) -> PathBuf {
    let config_path = get_config_path(args);

    if config_path.is_dir() {
        config_path.join("openmw.cfg")
    } else {
        config_path
    }
}

fn get_config_path(args: &GroundcoverArgs) -> PathBuf {
    if let Some(path) = &args.openmw_cfg {
        let absolute_path = if path.is_relative() {
            path.canonicalize().unwrap_or_else(|_| path.to_owned())
        } else {
            path.to_owned()
        };

        if absolute_path.is_file()
            || (absolute_path.is_dir() && absolute_path.join("openmw.cfg").is_file())
        {
            return absolute_path;
        }

        panic!(
            "explicit --openmw-cfg path is neither a file nor a directory containing openmw.cfg"
        );
    }

    let cwd_cfg = std::env::current_dir()
        .expect("failed to get current directory")
        .join("openmw.cfg");
    if cwd_cfg.is_file() {
        return cwd_cfg;
    }

    openmw_config::default_config_path()
}

fn load_openmw_config(args: &GroundcoverArgs) -> io::Result<OpenMWConfiguration> {
    OpenMWConfiguration::new(Some(get_config_path(args))).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to read OpenMW configuration: {error}"),
        )
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

fn default_output_directory(config: &OpenMWConfiguration) -> PathBuf {
    config
        .data_local()
        .map_or_else(openmw_config::default_data_local_path, |data_local| {
            data_local.parsed().to_owned()
        })
}

fn backup_openmw_cfg(selected_config_file: &Path) -> io::Result<PathBuf> {
    let file_name = selected_config_file.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "selected OpenMW config path has no file name",
        )
    })?;
    let backup_name = format!("{}.greenmote.bak", file_name.to_string_lossy());
    let backup_path = selected_config_file.with_file_name(backup_name);

    copy(selected_config_file, &backup_path)?;

    Ok(backup_path)
}

fn auto_enable_outputs(
    config: &mut OpenMWConfiguration,
    groundcover_config: &GroundcoverConfig,
    selected_config_file: &Path,
) -> io::Result<()> {
    let _backup = backup_openmw_cfg(selected_config_file)?;

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

    config.save_user().map_err(to_io_error)
}

fn to_io_error<E: std::fmt::Display>(error: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
