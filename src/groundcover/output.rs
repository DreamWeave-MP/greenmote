use std::{
    collections::BTreeSet,
    fs::{copy, create_dir_all, metadata},
    io::{self, Write},
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use tes3::esp::{FixedString, Header, ObjectFlags, Plugin, TES3Object, types::FileType};
use vfstool_lib::VFS;

use crate::groundcover::{GroundcoverConfig, mesh, plan::ConversionPlan};

#[derive(Debug)]
pub struct BuiltPlugins {
    pub groundcover_plugin: Plugin,
    pub deleted_plugin: Plugin,
    pub groundcover_header: Header,
    pub deleted_header: Header,
    pub contributing_masters: BTreeSet<usize>,
}

#[derive(Debug)]
pub struct MeshCopyJob {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
}

#[derive(Debug)]
pub struct RunSummary {
    pub content_files: usize,
    pub loaded_plugins: usize,
    pub matched_statics: usize,
    pub changed_cells: usize,
    pub touched_refs: usize,
    pub meshes_to_copy: usize,
}

#[must_use]
pub fn build_plugins(plan: &ConversionPlan) -> BuiltPlugins {
    let mut groundcover_plugin = Plugin::new();
    let mut deleted_plugin = Plugin::new();
    let mut groundcover_header = groundcover_header();
    let mut deleted_header = deleted_header();
    let mut contributing_masters = BTreeSet::new();

    for static_plan in &plan.static_plans {
        groundcover_plugin
            .objects
            .push(static_plan.output_static.clone().into());
        contributing_masters.insert(static_plan.source_load_index);
    }

    for cell_plan in &plan.cell_plans {
        if !cell_plan.is_used() {
            continue;
        }

        contributing_masters.insert(cell_plan.load_index);
        groundcover_plugin.objects.extend(
            cell_plan
                .groundcover_cells
                .iter()
                .cloned()
                .map(TES3Object::from),
        );
        deleted_plugin.objects.extend(
            cell_plan
                .deleted_cells
                .iter()
                .cloned()
                .map(TES3Object::from),
        );
    }

    groundcover_header.num_objects = groundcover_plugin
        .objects
        .len()
        .try_into()
        .unwrap_or(u32::MAX);
    deleted_header.num_objects = deleted_plugin.objects.len().try_into().unwrap_or(u32::MAX);

    BuiltPlugins {
        groundcover_plugin,
        deleted_plugin,
        groundcover_header,
        deleted_header,
        contributing_masters,
    }
}

pub fn add_masters(built: &mut BuiltPlugins, plan: &ConversionPlan) -> io::Result<()> {
    let mut load_indices = built
        .contributing_masters
        .iter()
        .copied()
        .collect::<Vec<_>>();
    load_indices.sort_unstable();

    for load_index in load_indices {
        let path = source_path_for_load_index(plan, load_index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("missing source path for contributing load index {load_index}"),
            )
        })?;
        let master = plugin_master(path)?;
        built.groundcover_header.masters.push(master.clone());
        built.deleted_header.masters.push(master);
    }

    Ok(())
}

fn source_path_for_load_index(plan: &ConversionPlan, load_index: usize) -> Option<&Path> {
    plan.static_plans
        .iter()
        .find(|static_plan| static_plan.source_load_index == load_index)
        .map(|static_plan| static_plan.source_plugin_path.as_path())
        .or_else(|| {
            plan.cell_plans
                .iter()
                .find(|cell_plan| cell_plan.load_index == load_index)
                .map(|cell_plan| cell_plan.plugin_path.as_path())
        })
}

fn plugin_master(plugin_path: &Path) -> io::Result<(String, u64)> {
    let plugin_size = metadata(plugin_path)?.len();
    let name = plugin_path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("plugin path has no file name: {}", plugin_path.display()),
        )
    })?;

    Ok((name.to_string_lossy().to_string(), plugin_size))
}

pub fn save_plugins(mut built: BuiltPlugins, config: &GroundcoverConfig) -> io::Result<()> {
    create_dir_all(&config.output_directory)?;

    built
        .groundcover_plugin
        .objects
        .insert(0, TES3Object::Header(built.groundcover_header));
    built
        .deleted_plugin
        .objects
        .insert(0, TES3Object::Header(built.deleted_header));

    built.groundcover_plugin.sort_objects();
    built.deleted_plugin.sort_objects();

    built
        .groundcover_plugin
        .save_path(config.output_directory.join(&config.groundcover_output))?;
    built
        .deleted_plugin
        .save_path(config.output_directory.join(&config.deleted_output))?;

    Ok(())
}

pub fn resolve_mesh_copy_jobs(
    vfs: &VFS,
    mesh_paths: &BTreeSet<String>,
    output_directory: &Path,
) -> Vec<MeshCopyJob> {
    mesh_paths
        .iter()
        .filter_map(|mesh_path| {
            let backslash_key = format!("Meshes\\{mesh_path}");
            let slash_key = format!("Meshes/{}", mesh_path.replace('\\', "/"));
            let source = vfs
                .get_file(&backslash_key)
                .or_else(|| vfs.get_file(&slash_key));

            if let Some(source) = source {
                Some(MeshCopyJob {
                    source_path: source.path().to_path_buf(),
                    target_path: mesh::mesh_output_path(output_directory, mesh_path),
                })
            } else {
                eprintln!("[ WARNING ]: Mesh not found in VFS: {backslash_key}");
                None
            }
        })
        .collect()
}

pub fn copy_meshes(jobs: &[MeshCopyJob]) -> io::Result<()> {
    jobs.par_iter().try_for_each(|job| {
        if let Some(parent) = job.target_path.parent() {
            create_dir_all(parent)?;
        }
        copy(&job.source_path, &job.target_path)?;
        Ok(())
    })
}

pub fn write_summary(
    mut writer: impl Write,
    summary: &RunSummary,
    plan: &ConversionPlan,
) -> io::Result<()> {
    writeln!(writer, "# greenmote convert {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(writer, "# content files: {}", summary.content_files)?;
    writeln!(writer, "# loaded plugins: {}", summary.loaded_plugins)?;
    writeln!(writer, "# matched statics: {}", summary.matched_statics)?;
    writeln!(writer, "# changed cells: {}", summary.changed_cells)?;
    writeln!(writer, "# touched refs: {}", summary.touched_refs)?;
    writeln!(writer, "# meshes to copy: {}", summary.meshes_to_copy)?;

    for static_plan in &plan.static_plans {
        writeln!(
            writer,
            "STAT {:?} from {:?}: mesh -> {:?}",
            static_plan.output_static.id,
            static_plan.source_plugin_name,
            static_plan.output_static.mesh
        )?;
    }

    for cell_plan in plan.cell_plans.iter().filter(|plan| plan.is_used()) {
        writeln!(
            writer,
            "CELL refs from {:?}: {} refs in {} exterior cells",
            cell_plan.plugin_name,
            cell_plan.touched_refs,
            cell_plan.groundcover_cells.len()
        )?;
    }

    Ok(())
}

fn groundcover_header() -> Header {
    Header {
        version: 1.3,
        author: FixedString("greenmote".to_owned()),
        description: FixedString(
            "Generated groundcover plugin from vanilla-style static refs".to_owned(),
        ),
        file_type: FileType::Esp,
        flags: ObjectFlags::default(),
        num_objects: 0,
        masters: Vec::new(),
    }
}

fn deleted_header() -> Header {
    Header {
        version: 1.3,
        author: FixedString("greenmote".to_owned()),
        description: FixedString("Generated deleted groundcover plugin".to_owned()),
        file_type: FileType::Esp,
        flags: ObjectFlags::default(),
        num_objects: 0,
        masters: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeSet, HashSet},
        path::PathBuf,
    };

    use super::*;

    #[test]
    fn mesh_output_path_preserves_subdirectories_under_meshes_grass() {
        assert_eq!(
            mesh::mesh_output_path(Path::new("out"), "flora\\tree\\grass.nif"),
            PathBuf::from("out")
                .join("Meshes")
                .join("grass")
                .join("flora")
                .join("tree")
                .join("grass.nif")
        );
    }

    #[test]
    fn empty_plan_builds_empty_plugins() {
        let plan = ConversionPlan {
            static_plans: Vec::new(),
            cell_plans: Vec::new(),
            matched_static_ids: HashSet::new(),
            mesh_paths: BTreeSet::new(),
        };

        let built = build_plugins(&plan);

        assert!(built.groundcover_plugin.objects.is_empty());
        assert!(built.deleted_plugin.objects.is_empty());
        assert!(built.contributing_masters.is_empty());
    }
}
