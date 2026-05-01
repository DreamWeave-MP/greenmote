use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, create_dir_all},
    io::{self, Write},
    path::{Path, PathBuf},
};

use rayon::prelude::*;
use tes3::esp::{FixedString, Header, ObjectFlags, Plugin, TES3Object, types::FileType};
use vfstool_lib::{VFS, VfsFile};

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
    pub source: VfsFile,
    pub target_path: PathBuf,
}

#[derive(Debug)]
pub struct RunSummary {
    pub content_files: usize,
    pub loaded_plugins: usize,
    pub matched_statics: usize,
    pub used_statics: usize,
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
    let generated_static_ids = plan.generated_static_ids_by_source_id();
    validate_master_count("groundcover", &groundcover_master_indices)?;
    validate_master_count("deleted groundcover", &deleted_master_indices)?;

    for static_plan in plan.used_static_plans() {
        groundcover_plugin
            .objects
            .push(static_plan.output_static()?.into());
    }

    for cell_plan in &plan.cell_plans {
        if !cell_plan.is_used() {
            continue;
        }

        for cell in &cell_plan.groundcover_cells {
            groundcover_plugin.objects.push(
                remap_cell(
                    cell,
                    cell_plan,
                    &groundcover_master_indices,
                    RefIdMode::Generated,
                    &generated_static_ids,
                )?
                .into(),
            );
        }
        for cell in &cell_plan.deleted_cells {
            deleted_plugin.objects.push(
                remap_cell(
                    cell,
                    cell_plan,
                    &deleted_master_indices,
                    RefIdMode::Source,
                    &generated_static_ids,
                )?
                .into(),
            );
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
    let mut masters = MasterIndexBuilder::new(plan);

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
    let mut masters = MasterIndexBuilder::new(plan);

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

struct MasterIndexBuilder {
    source_load_indices: BTreeMap<MasterSpec, usize>,
    masters: Vec<(usize, MasterSpec)>,
}

impl MasterIndexBuilder {
    fn new(plan: &ConversionPlan) -> Self {
        let source_load_indices = plan
            .cell_plans
            .iter()
            .map(|cell_plan| (cell_plan.source_master.clone(), cell_plan.load_index))
            .collect();

        Self {
            source_load_indices,
            masters: Vec::new(),
        }
    }

    fn insert(&mut self, master: MasterSpec) {
        if !self.masters.iter().any(|(_, existing)| existing == &master) {
            let load_index = self
                .source_load_indices
                .get(&master)
                .copied()
                .unwrap_or(usize::MAX);
            self.masters.push((load_index, master));
        }
    }

    fn insert_cell_reference_masters(
        &mut self,
        cell: &tes3::esp::Cell,
        cell_plan: &PluginCellPlan,
    ) {
        for (key, reference) in &cell.references {
            self.insert_source_master(key.0, cell_plan);
            if reference.mast_index != key.0 {
                self.insert_source_master(reference.mast_index, cell_plan);
            }
        }
    }

    fn insert_source_master(&mut self, mast_index: u32, cell_plan: &PluginCellPlan) {
        if let Some(master) = cell_plan.master_for_source_index(mast_index) {
            self.insert(master.clone());
        }
    }

    fn into_map(self) -> BTreeMap<MasterSpec, u32> {
        let mut masters = self.masters;
        masters.sort_by(|(left_index, left_master), (right_index, right_master)| {
            left_index
                .cmp(right_index)
                .then_with(|| left_master.name.cmp(&right_master.name))
        });

        masters
            .into_iter()
            .enumerate()
            .map(|(index, (_, master))| (master, u32::try_from(index + 1).unwrap_or(u32::MAX)))
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
    ref_id_mode: RefIdMode,
    generated_static_ids: &BTreeMap<String, String>,
) -> io::Result<tes3::esp::Cell> {
    let mut remapped = cell.clone();
    remapped.references.clear();

    for (key, reference) in &cell.references {
        let remapped_key_mast = remap_source_mast_index(key.0, cell_plan, master_indices)?;
        let mut remapped_reference = reference.clone();
        if ref_id_mode == RefIdMode::Generated {
            let source_id = remapped_reference.id.to_ascii_lowercase();
            remapped_reference.id =
                generated_static_ids
                    .get(&source_id)
                    .cloned()
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "generated static id map is missing source static {source_id:?}"
                            ),
                        )
                    })?;
        }
        remapped_reference.mast_index =
            remap_source_mast_index(remapped_reference.mast_index, cell_plan, master_indices)?;
        remapped
            .references
            .insert((remapped_key_mast, key.1), remapped_reference);
    }

    Ok(remapped)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RefIdMode {
    Source,
    Generated,
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
    mesh_paths: &BTreeSet<mesh::MeshCopyPath>,
    output_directory: &Path,
) -> io::Result<Vec<MeshCopyJob>> {
    let mut missing = Vec::new();
    let jobs = mesh_paths
        .iter()
        .map(|mesh_path| {
            let backslash_key = format!("Meshes\\{}", mesh_path.source);
            let slash_key = format!("Meshes/{}", mesh_path.source.replace('\\', "/"));
            let source = vfs
                .get_file(&backslash_key)
                .or_else(|| vfs.get_file(&slash_key));

            if let Some(source) = source {
                Ok(Some(MeshCopyJob {
                    source: source.clone(),
                    target_path: mesh::mesh_output_path(output_directory, &mesh_path.target)?,
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
        let mut source = job.source.open()?;
        let mut target = File::create(&job.target_path)?;
        io::copy(&mut source, &mut target)?;
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
    writeln!(writer, "# used statics: {}", summary.used_statics)?;
    writeln!(writer, "# changed cells: {}", summary.changed_cells)?;
    writeln!(writer, "# touched refs: {}", summary.touched_refs)?;
    writeln!(writer, "# meshes to copy: {}", summary.meshes_to_copy)?;

    for static_plan in plan.used_static_plans() {
        let output_static = static_plan.output_static()?;
        writeln!(
            writer,
            "STAT {:?} from {:?}: mesh -> {:?}",
            output_static.id, static_plan.source_plugin_name, output_static.mesh
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

    for mesh_path in &plan.used_mesh_paths()? {
        writeln!(
            writer,
            "MESH {:?} -> {}",
            mesh_path.source,
            mesh::mesh_output_path(&config.output_directory, &mesh_path.target)?.display()
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
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
    };

    use tes3::esp::{Cell, Reference, Static};

    use crate::groundcover::{
        mesh::MeshCopyPath,
        plan::{PluginCellPlan, StaticPlan},
    };

    use super::*;

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "greenmote-output-test-{name}-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

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
    fn mesh_jobs_use_source_for_lookup_and_target_for_output() {
        let data_dir = TempDir::new("mesh-source-target-data");
        let output_dir = TempDir::new("mesh-source-target-output");
        let mesh_dir = data_dir.path().join("Meshes").join("grass");
        std::fs::create_dir_all(&mesh_dir).unwrap();
        std::fs::write(mesh_dir.join("sky_flora.nif"), b"mesh bytes").unwrap();
        let vfs = VFS::from_directories([data_dir.path()], None);
        let mesh_paths = BTreeSet::from([MeshCopyPath {
            source: "grass\\sky_flora.nif".to_owned(),
            target: "sky_flora.nif".to_owned(),
        }]);

        let jobs = resolve_mesh_copy_jobs(&vfs, &mesh_paths, output_dir.path()).unwrap();

        assert_eq!(jobs.len(), 1);
        assert_eq!(
            jobs[0].target_path,
            output_dir
                .path()
                .join("Meshes")
                .join("grass")
                .join("sky_flora.nif")
        );
    }

    #[test]
    fn missing_mesh_jobs_are_fatal() {
        let data_dir = TempDir::new("missing-mesh-data");
        let output_dir = TempDir::new("missing-mesh-output");
        let vfs = VFS::from_directories([data_dir.path()], None);
        let mesh_paths = BTreeSet::from([MeshCopyPath {
            source: "flora\\missing.nif".to_owned(),
            target: "flora\\missing.nif".to_owned(),
        }]);

        let error = resolve_mesh_copy_jobs(&vfs, &mesh_paths, output_dir.path()).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("Meshes\\flora\\missing.nif"));
    }

    #[test]
    fn empty_plan_builds_empty_plugins() {
        let plan = ConversionPlan {
            static_plans: Vec::new(),
            cell_plans: Vec::new(),
            matched_static_ids: HashSet::new(),
            used_static_ids: BTreeSet::new(),
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
                source_static: Static {
                    id: "flora_grass_01".to_owned(),
                    mesh: "flora\\grass.nif".to_owned(),
                    ..Static::default()
                },
                generated_id: "gm_test_flora_grass_01".to_owned(),
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
                used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
            }],
            matched_static_ids: HashSet::from(["flora_grass_01".to_owned()]),
            used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
        };

        let built = build_plugins(&plan).unwrap();
        let generated_static = built
            .groundcover_plugin
            .objects_of_type::<Static>()
            .next()
            .unwrap();
        let generated_cell = built
            .groundcover_plugin
            .objects_of_type::<Cell>()
            .next()
            .unwrap();
        let deleted_cell = built
            .deleted_plugin
            .objects_of_type::<Cell>()
            .next()
            .unwrap();

        assert_eq!(
            built.groundcover_header.masters,
            vec![("Source.esp".to_owned(), 42)]
        );
        assert_eq!(generated_static.id, "gm_test_flora_grass_01");
        assert!(generated_cell.references.contains_key(&(1, 7)));
        assert_eq!(generated_cell.references[&(1, 7)].mast_index, 1);
        assert_eq!(
            generated_cell.references[&(1, 7)].id,
            "gm_test_flora_grass_01"
        );
        assert_eq!(deleted_cell.references[&(1, 7)].id, "flora_grass_01");
    }

    #[test]
    fn source_plugin_refs_use_source_plugin_as_owner_master_only() {
        let morrowind_master = MasterSpec {
            name: "Morrowind.esm".to_owned(),
            size: 79_837_557,
        };
        let bloodmoon_master = MasterSpec {
            name: "Bloodmoon.esm".to_owned(),
            size: 9_631_798,
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
                source_load_index: 1,
                source_plugin_name: "Bloodmoon.esm".to_owned(),
                source_plugin_path: PathBuf::from("Bloodmoon.esm"),
                source_master: bloodmoon_master.clone(),
                source_static: Static {
                    id: "flora_grass_01".to_owned(),
                    mesh: "flora\\grass.nif".to_owned(),
                    ..Static::default()
                },
                generated_id: "gm_flora_grass_01".to_owned(),
            }],
            cell_plans: vec![PluginCellPlan {
                load_index: 1,
                plugin_name: "Bloodmoon.esm".to_owned(),
                plugin_path: PathBuf::from("Bloodmoon.esm"),
                source_master: bloodmoon_master,
                header_masters: vec![morrowind_master],
                groundcover_cells: vec![cell.clone()],
                deleted_cells: vec![cell],
                touched_refs: 1,
                used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
            }],
            matched_static_ids: HashSet::new(),
            used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
        };

        let built = build_plugins(&plan).unwrap();
        let generated_cell = built
            .groundcover_plugin
            .objects_of_type::<Cell>()
            .next()
            .unwrap();

        assert_eq!(
            built.groundcover_header.masters,
            vec![("Bloodmoon.esm".to_owned(), 9_631_798)]
        );
        assert!(generated_cell.references.contains_key(&(1, 7)));
        assert_eq!(generated_cell.references[&(1, 7)].mast_index, 1);
    }

    #[test]
    fn source_owner_masters_follow_load_order_not_cell_plan_order() {
        let morrowind_master = MasterSpec {
            name: "Morrowind.esm".to_owned(),
            size: 79_837_557,
        };
        let bloodmoon_master = MasterSpec {
            name: "Bloodmoon.esm".to_owned(),
            size: 9_631_798,
        };
        let mut morrowind_cell = Cell::default();
        morrowind_cell.references.insert(
            (0, 7),
            Reference {
                id: "flora_grass_01".to_owned(),
                mast_index: 0,
                ..Reference::default()
            },
        );
        let mut bloodmoon_cell = Cell::default();
        bloodmoon_cell.references.insert(
            (0, 8),
            Reference {
                id: "flora_grass_02".to_owned(),
                mast_index: 0,
                ..Reference::default()
            },
        );
        let plan = ConversionPlan {
            static_plans: vec![
                StaticPlan {
                    source_load_index: 0,
                    source_plugin_name: "Morrowind.esm".to_owned(),
                    source_plugin_path: PathBuf::from("Morrowind.esm"),
                    source_master: morrowind_master.clone(),
                    source_static: Static {
                        id: "flora_grass_01".to_owned(),
                        mesh: "flora\\grass_01.nif".to_owned(),
                        ..Static::default()
                    },
                    generated_id: "gm_flora_grass_01".to_owned(),
                },
                StaticPlan {
                    source_load_index: 2,
                    source_plugin_name: "Bloodmoon.esm".to_owned(),
                    source_plugin_path: PathBuf::from("Bloodmoon.esm"),
                    source_master: bloodmoon_master.clone(),
                    source_static: Static {
                        id: "flora_grass_02".to_owned(),
                        mesh: "flora\\grass_02.nif".to_owned(),
                        ..Static::default()
                    },
                    generated_id: "gm_flora_grass_02".to_owned(),
                },
            ],
            cell_plans: vec![
                PluginCellPlan {
                    load_index: 2,
                    plugin_name: "Bloodmoon.esm".to_owned(),
                    plugin_path: PathBuf::from("Bloodmoon.esm"),
                    source_master: bloodmoon_master,
                    header_masters: Vec::new(),
                    groundcover_cells: vec![bloodmoon_cell.clone()],
                    deleted_cells: vec![bloodmoon_cell],
                    touched_refs: 1,
                    used_static_ids: BTreeSet::from(["flora_grass_02".to_owned()]),
                },
                PluginCellPlan {
                    load_index: 0,
                    plugin_name: "Morrowind.esm".to_owned(),
                    plugin_path: PathBuf::from("Morrowind.esm"),
                    source_master: morrowind_master,
                    header_masters: Vec::new(),
                    groundcover_cells: vec![morrowind_cell.clone()],
                    deleted_cells: vec![morrowind_cell],
                    touched_refs: 1,
                    used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
                },
            ],
            matched_static_ids: HashSet::new(),
            used_static_ids: BTreeSet::from([
                "flora_grass_01".to_owned(),
                "flora_grass_02".to_owned(),
            ]),
        };

        let built = build_plugins(&plan).unwrap();

        assert_eq!(
            built.groundcover_header.masters,
            vec![
                ("Morrowind.esm".to_owned(), 79_837_557),
                ("Bloodmoon.esm".to_owned(), 9_631_798),
            ]
        );
    }

    #[test]
    fn header_master_refs_use_exact_owner_master_only() {
        let morrowind_master = MasterSpec {
            name: "Morrowind.esm".to_owned(),
            size: 79_837_557,
        };
        let tribunal_master = MasterSpec {
            name: "Tribunal.esm".to_owned(),
            size: 4_568_965,
        };
        let mut cell = Cell::default();
        cell.references.insert(
            (2, 7),
            Reference {
                id: "flora_grass_01".to_owned(),
                mast_index: 2,
                ..Reference::default()
            },
        );
        let plan = ConversionPlan {
            static_plans: vec![StaticPlan {
                source_load_index: 2,
                source_plugin_name: "Patch.esp".to_owned(),
                source_plugin_path: PathBuf::from("Patch.esp"),
                source_master: MasterSpec {
                    name: "Patch.esp".to_owned(),
                    size: 10,
                },
                source_static: Static {
                    id: "flora_grass_01".to_owned(),
                    mesh: "flora\\grass.nif".to_owned(),
                    ..Static::default()
                },
                generated_id: "gm_flora_grass_01".to_owned(),
            }],
            cell_plans: vec![PluginCellPlan {
                load_index: 2,
                plugin_name: "Patch.esp".to_owned(),
                plugin_path: PathBuf::from("Patch.esp"),
                source_master: MasterSpec {
                    name: "Patch.esp".to_owned(),
                    size: 10,
                },
                header_masters: vec![morrowind_master, tribunal_master],
                groundcover_cells: vec![cell.clone()],
                deleted_cells: vec![cell],
                touched_refs: 1,
                used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
            }],
            matched_static_ids: HashSet::new(),
            used_static_ids: BTreeSet::from(["flora_grass_01".to_owned()]),
        };

        let built = build_plugins(&plan).unwrap();
        let generated_cell = built
            .groundcover_plugin
            .objects_of_type::<Cell>()
            .next()
            .unwrap();

        assert_eq!(
            built.groundcover_header.masters,
            vec![("Tribunal.esm".to_owned(), 4_568_965)]
        );
        assert!(generated_cell.references.contains_key(&(1, 7)));
        assert_eq!(generated_cell.references[&(1, 7)].mast_index, 1);
    }

    #[test]
    fn unused_static_only_source_plugins_do_not_emit_records_or_become_masters() {
        let plan = ConversionPlan {
            static_plans: vec![StaticPlan {
                source_load_index: 0,
                source_plugin_name: "StaticOnly.esp".to_owned(),
                source_plugin_path: PathBuf::from("StaticOnly.esp"),
                source_master: MasterSpec {
                    name: "StaticOnly.esp".to_owned(),
                    size: 5,
                },
                source_static: Static {
                    id: "flora_grass_unused".to_owned(),
                    ..Static::default()
                },
                generated_id: "gm_unused".to_owned(),
            }],
            cell_plans: Vec::new(),
            matched_static_ids: HashSet::new(),
            used_static_ids: BTreeSet::new(),
        };

        let built = build_plugins(&plan).unwrap();

        assert!(built.groundcover_plugin.objects.is_empty());
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
                    used_static_ids: BTreeSet::from([format!("flora_grass_{index}")]),
                }
            })
            .collect();
        let plan = ConversionPlan {
            static_plans: Vec::new(),
            cell_plans,
            matched_static_ids: HashSet::new(),
            used_static_ids: BTreeSet::new(),
        };

        let error = build_plugins(&plan).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("only support 255"));
    }
}
