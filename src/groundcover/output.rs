use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{copy, create_dir_all},
    io::{self, Write},
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use tes3::esp::{FixedString, Header, ObjectFlags, Plugin, TES3Object, types::FileType};
use vfstool_lib::VFS;

use crate::groundcover::{
    GroundcoverConfig, mesh,
    plan::{ConversionPlan, MasterSpec, PluginCellPlan},
};

#[derive(Debug)]
pub struct BuiltPlugins {
    pub groundcover_plugin: Plugin,
    pub deleted_plugin: Plugin,
    pub groundcover_header: Header,
    pub deleted_header: Header,
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

const MAX_GENERATED_MASTERS: usize = 255;

pub fn build_plugins(plan: &ConversionPlan) -> io::Result<BuiltPlugins> {
    let mut groundcover_plugin = Plugin::new();
    let mut deleted_plugin = Plugin::new();
    let mut groundcover_header = groundcover_header();
    let mut deleted_header = deleted_header();
    let groundcover_master_indices = groundcover_master_indices(plan);
    let deleted_master_indices = deleted_master_indices(plan);
    validate_master_count("groundcover", &groundcover_master_indices)?;
    validate_master_count("deleted groundcover", &deleted_master_indices)?;

    for static_plan in &plan.static_plans {
        groundcover_plugin
            .objects
            .push(static_plan.output_static.clone().into());
    }

    for cell_plan in &plan.cell_plans {
        if !cell_plan.is_used() {
            continue;
        }

        for cell in &cell_plan.groundcover_cells {
            groundcover_plugin
                .objects
                .push(remap_cell(cell, cell_plan, &groundcover_master_indices)?.into());
        }
        for cell in &cell_plan.deleted_cells {
            deleted_plugin
                .objects
                .push(remap_cell(cell, cell_plan, &deleted_master_indices)?.into());
        }
    }

    groundcover_header.masters = masters_from_index_map(&groundcover_master_indices);
    deleted_header.masters = masters_from_index_map(&deleted_master_indices);

    groundcover_header.num_objects = groundcover_plugin
        .objects
        .len()
        .try_into()
        .unwrap_or(u32::MAX);
    deleted_header.num_objects = deleted_plugin.objects.len().try_into().unwrap_or(u32::MAX);

    Ok(BuiltPlugins {
        groundcover_plugin,
        deleted_plugin,
        groundcover_header,
        deleted_header,
    })
}

fn groundcover_master_indices(plan: &ConversionPlan) -> BTreeMap<MasterSpec, u32> {
    let mut masters = MasterIndexBuilder::default();

    for cell_plan in plan
        .cell_plans
        .iter()
        .filter(|cell_plan| cell_plan.is_used())
    {
        for cell in &cell_plan.groundcover_cells {
            masters.insert_cell_reference_masters(cell, cell_plan);
        }
    }

    masters.into_map()
}

fn deleted_master_indices(plan: &ConversionPlan) -> BTreeMap<MasterSpec, u32> {
    let mut masters = MasterIndexBuilder::default();

    for cell_plan in plan
        .cell_plans
        .iter()
        .filter(|cell_plan| cell_plan.is_used())
    {
        for cell in &cell_plan.deleted_cells {
            masters.insert_cell_reference_masters(cell, cell_plan);
        }
    }

    masters.into_map()
}

fn validate_master_count(
    output_name: &str,
    master_indices: &BTreeMap<MasterSpec, u32>,
) -> io::Result<()> {
    if master_indices.len() > MAX_GENERATED_MASTERS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{output_name} output needs {} masters, but TES3 reference indices only support {MAX_GENERATED_MASTERS}",
                master_indices.len()
            ),
        ));
    }

    Ok(())
}

#[derive(Default)]
struct MasterIndexBuilder {
    masters: Vec<MasterSpec>,
}

impl MasterIndexBuilder {
    fn insert(&mut self, master: MasterSpec) {
        if !self.masters.contains(&master) {
            self.masters.push(master);
        }
    }

    fn insert_cell_reference_masters(
        &mut self,
        cell: &tes3::esp::Cell,
        cell_plan: &PluginCellPlan,
    ) {
        for (key, reference) in &cell.references {
            if let Some(master) = cell_plan.master_for_source_index(key.0) {
                self.insert(master.clone());
            }
            if reference.mast_index != key.0
                && let Some(master) = cell_plan.master_for_source_index(reference.mast_index)
            {
                self.insert(master.clone());
            }
        }
    }

    fn into_map(self) -> BTreeMap<MasterSpec, u32> {
        self.masters
            .into_iter()
            .enumerate()
            .map(|(index, master)| (master, u32::try_from(index + 1).unwrap_or(u32::MAX)))
            .collect()
    }
}

fn masters_from_index_map(master_indices: &BTreeMap<MasterSpec, u32>) -> Vec<(String, u64)> {
    let mut indexed_masters = master_indices.iter().collect::<Vec<_>>();
    indexed_masters.sort_by_key(|(_, index)| *index);

    indexed_masters
        .into_iter()
        .map(|(master, _)| master.as_header_master())
        .collect()
}

fn remap_cell(
    cell: &tes3::esp::Cell,
    cell_plan: &PluginCellPlan,
    master_indices: &BTreeMap<MasterSpec, u32>,
) -> io::Result<tes3::esp::Cell> {
    let mut remapped = cell.clone();
    remapped.references.clear();

    for (key, reference) in &cell.references {
        let remapped_key_mast = remap_source_mast_index(key.0, cell_plan, master_indices)?;
        let mut remapped_reference = reference.clone();
        remapped_reference.mast_index =
            remap_source_mast_index(remapped_reference.mast_index, cell_plan, master_indices)?;
        remapped
            .references
            .insert((remapped_key_mast, key.1), remapped_reference);
    }

    Ok(remapped)
}

fn remap_source_mast_index(
    mast_index: u32,
    cell_plan: &PluginCellPlan,
    master_indices: &BTreeMap<MasterSpec, u32>,
) -> io::Result<u32> {
    let source_master = cell_plan.master_for_source_index(mast_index).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "cell ref in {} uses source master index {mast_index}, but that plugin header does not define it",
                cell_plan.plugin_name
            ),
        )
    })?;
    master_indices.get(source_master).copied().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "generated master list is missing {} while remapping {}",
                source_master.name, cell_plan.plugin_name
            ),
        )
    })
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
) -> io::Result<Vec<MeshCopyJob>> {
    let mut missing = Vec::new();
    let jobs = mesh_paths
        .iter()
        .map(|mesh_path| {
            let backslash_key = format!("Meshes\\{mesh_path}");
            let slash_key = format!("Meshes/{}", mesh_path.replace('\\', "/"));
            let source = vfs
                .get_file(&backslash_key)
                .or_else(|| vfs.get_file(&slash_key));

            if let Some(source) = source {
                Ok(Some(MeshCopyJob {
                    source_path: source.path().to_path_buf(),
                    target_path: mesh::mesh_output_path(output_directory, mesh_path)?,
                }))
            } else {
                missing.push(backslash_key);
                Ok(None)
            }
        })
        .collect::<io::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    if missing.is_empty() {
        Ok(jobs)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "missing required meshes in OpenMW VFS:\n{}",
                missing.join("\n")
            ),
        ))
    }
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
    config: &GroundcoverConfig,
) -> io::Result<()> {
    writeln!(writer, "# greenmote convert {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(
        writer,
        "# output directory: {}",
        config.output_directory.display()
    )?;
    writeln!(
        writer,
        "# groundcover output: {}",
        config
            .output_directory
            .join(&config.groundcover_output)
            .display()
    )?;
    writeln!(
        writer,
        "# deleted output: {}",
        config
            .output_directory
            .join(&config.deleted_output)
            .display()
    )?;
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

    for mesh_path in &plan.mesh_paths {
        writeln!(
            writer,
            "MESH {:?} -> {}",
            mesh_path,
            mesh::mesh_output_path(&config.output_directory, mesh_path)?.display()
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

    use tes3::esp::{Cell, Reference, Static};

    use crate::groundcover::plan::{PluginCellPlan, StaticPlan};

    use super::*;

    #[test]
    fn mesh_output_path_preserves_subdirectories_under_meshes_grass() {
        assert_eq!(
            mesh::mesh_output_path(Path::new("out"), "flora\\tree\\grass.nif").unwrap(),
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

        let built = build_plugins(&plan).unwrap();

        assert!(built.groundcover_plugin.objects.is_empty());
        assert!(built.deleted_plugin.objects.is_empty());
        assert!(built.groundcover_header.masters.is_empty());
        assert!(built.deleted_header.masters.is_empty());
    }

    #[test]
    fn copied_cell_refs_are_remapped_to_generated_master_indices() {
        let source_master = MasterSpec {
            name: "Source.esp".to_owned(),
            size: 42,
        };
        let mut cell = Cell::default();
        cell.references.insert(
            (0, 7),
            Reference {
                id: "flora_grass_01".to_owned(),
                mast_index: 0,
                ..Reference::default()
            },
        );
        let plan = ConversionPlan {
            static_plans: vec![StaticPlan {
                source_load_index: 0,
                source_plugin_name: "Source.esp".to_owned(),
                source_plugin_path: PathBuf::from("Source.esp"),
                source_master: source_master.clone(),
                output_static: Static::default(),
            }],
            cell_plans: vec![PluginCellPlan {
                load_index: 0,
                plugin_name: "Source.esp".to_owned(),
                plugin_path: PathBuf::from("Source.esp"),
                source_master,
                header_masters: Vec::new(),
                groundcover_cells: vec![cell.clone()],
                deleted_cells: vec![cell],
                touched_refs: 1,
            }],
            matched_static_ids: HashSet::from(["flora_grass_01".to_owned()]),
            mesh_paths: BTreeSet::new(),
        };

        let built = build_plugins(&plan).unwrap();
        let generated_cell = built
            .groundcover_plugin
            .objects_of_type::<Cell>()
            .next()
            .unwrap();

        assert_eq!(
            built.groundcover_header.masters,
            vec![("Source.esp".to_owned(), 42)]
        );
        assert!(generated_cell.references.contains_key(&(1, 7)));
        assert_eq!(generated_cell.references[&(1, 7)].mast_index, 1);
    }

    #[test]
    fn static_only_source_plugins_do_not_become_masters() {
        let plan = ConversionPlan {
            static_plans: vec![StaticPlan {
                source_load_index: 0,
                source_plugin_name: "StaticOnly.esp".to_owned(),
                source_plugin_path: PathBuf::from("StaticOnly.esp"),
                source_master: MasterSpec {
                    name: "StaticOnly.esp".to_owned(),
                    size: 5,
                },
                output_static: Static::default(),
            }],
            cell_plans: Vec::new(),
            matched_static_ids: HashSet::new(),
            mesh_paths: BTreeSet::new(),
        };

        let built = build_plugins(&plan).unwrap();

        assert_eq!(built.groundcover_plugin.objects.len(), 1);
        assert!(built.groundcover_header.masters.is_empty());
        assert!(built.deleted_header.masters.is_empty());
    }

    #[test]
    fn generated_master_count_over_esp_reference_limit_fails() {
        let cell_plans = (0..=MAX_GENERATED_MASTERS)
            .map(|index| {
                let mut cell = Cell::default();
                cell.references.insert(
                    (0, u32::try_from(index).unwrap()),
                    Reference {
                        id: format!("flora_grass_{index}"),
                        mast_index: 0,
                        ..Reference::default()
                    },
                );

                PluginCellPlan {
                    load_index: index,
                    plugin_name: format!("Source{index}.esp"),
                    plugin_path: PathBuf::from(format!("Source{index}.esp")),
                    source_master: MasterSpec {
                        name: format!("Source{index}.esp"),
                        size: u64::try_from(index).unwrap(),
                    },
                    header_masters: Vec::new(),
                    groundcover_cells: vec![cell.clone()],
                    deleted_cells: vec![cell],
                    touched_refs: 1,
                }
            })
            .collect();
        let plan = ConversionPlan {
            static_plans: Vec::new(),
            cell_plans,
            matched_static_ids: HashSet::new(),
            mesh_paths: BTreeSet::new(),
        };

        let error = build_plugins(&plan).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("only support 255"));
    }
}
