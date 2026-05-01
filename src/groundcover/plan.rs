use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    hash::BuildHasher,
    io,
    path::PathBuf,
};

use rayon::prelude::*;
use tes3::esp::{Cell, Header, Plugin, Static};

use crate::groundcover::{GroundcoverConfig, mesh, records};

pub struct LoadedPlugin {
    pub load_index: usize,
    pub plugin_name: String,
    pub plugin_path: PathBuf,
    pub plugin: Plugin,
}

impl LoadedPlugin {
    #[must_use]
    pub fn source_master(&self) -> MasterSpec {
        MasterSpec::from_path(&self.plugin_path)
    }

    #[must_use]
    pub fn header_masters(&self) -> Vec<MasterSpec> {
        self.plugin
            .objects_of_type::<Header>()
            .next()
            .map_or_else(Vec::new, |header| {
                header
                    .masters
                    .iter()
                    .map(|(name, size)| MasterSpec {
                        name: name.clone(),
                        size: *size,
                    })
                    .collect()
            })
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MasterSpec {
    pub name: String,
    pub size: u64,
}

impl MasterSpec {
    #[must_use]
    pub fn from_path(path: &std::path::Path) -> Self {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |name| name.to_string_lossy().to_string(),
        );
        let size = std::fs::metadata(path).map_or(0, |metadata| metadata.len());

        Self { name, size }
    }

    #[must_use]
    pub fn as_header_master(&self) -> (String, u64) {
        (self.name.clone(), self.size)
    }
}

#[derive(Debug)]
pub struct StaticPlan {
    pub source_load_index: usize,
    pub source_plugin_name: String,
    pub source_plugin_path: PathBuf,
    pub source_master: MasterSpec,
    pub source_static: Static,
}

impl StaticPlan {
    #[must_use]
    pub fn id_key(&self) -> String {
        self.source_static.id.to_ascii_lowercase()
    }

    /// Builds the generated static record for a used source static.
    ///
    /// # Errors
    ///
    /// Returns invalid input if the source mesh path cannot be safely rooted under `grass\`.
    pub fn output_static(&self) -> io::Result<Static> {
        let mut output_static = self.source_static.clone();
        output_static.mesh = mesh::grass_prefixed_mesh(&output_static.mesh)?;

        Ok(output_static)
    }

    /// Builds the VFS lookup/output path pair for a used source static mesh.
    ///
    /// # Errors
    ///
    /// Returns invalid input if the source mesh path contains unsafe components.
    pub fn mesh_copy_path(&self) -> io::Result<mesh::MeshCopyPath> {
        mesh::normalize_mesh_for_copy(&self.source_static.mesh)
    }
}

#[derive(Debug)]
pub struct PluginCellPlan {
    pub load_index: usize,
    pub plugin_name: String,
    pub plugin_path: PathBuf,
    pub source_master: MasterSpec,
    pub header_masters: Vec<MasterSpec>,
    pub groundcover_cells: Vec<Cell>,
    pub deleted_cells: Vec<Cell>,
    pub touched_refs: usize,
    pub used_static_ids: BTreeSet<String>,
}

impl PluginCellPlan {
    #[must_use]
    pub fn master_for_source_index(&self, mast_index: u32) -> Option<&MasterSpec> {
        if mast_index == 0 {
            return Some(&self.source_master);
        }

        let index = usize::try_from(mast_index - 1).ok()?;
        self.header_masters.get(index)
    }

    #[must_use]
    pub fn master_chain_for_source_index(&self, mast_index: u32) -> Option<Vec<&MasterSpec>> {
        if mast_index == 0 {
            let mut chain = self.header_masters.iter().collect::<Vec<_>>();
            chain.push(&self.source_master);
            return Some(chain);
        }

        let index = usize::try_from(mast_index - 1).ok()?;
        if index >= self.header_masters.len() {
            return None;
        }

        Some(self.header_masters.iter().take(index + 1).collect())
    }
}

impl PluginCellPlan {
    #[must_use]
    pub fn is_used(&self) -> bool {
        self.touched_refs > 0
    }
}

#[derive(Debug)]
pub struct ConversionPlan {
    pub static_plans: Vec<StaticPlan>,
    pub cell_plans: Vec<PluginCellPlan>,
    pub matched_static_ids: HashSet<String>,
    pub used_static_ids: BTreeSet<String>,
}

impl ConversionPlan {
    #[must_use]
    pub fn used_static_plans(&self) -> Vec<&StaticPlan> {
        let static_plans_by_id = self
            .static_plans
            .iter()
            .map(|static_plan| (static_plan.id_key(), static_plan))
            .collect::<BTreeMap<_, _>>();

        self.used_static_ids
            .iter()
            .filter_map(|id| static_plans_by_id.get(id).copied())
            .collect()
    }

    /// Builds the mesh copy set for generated static records.
    ///
    /// # Errors
    ///
    /// Returns invalid input if a used static contains an unsafe mesh path.
    pub fn used_mesh_paths(&self) -> io::Result<BTreeSet<mesh::MeshCopyPath>> {
        self.used_static_plans()
            .into_iter()
            .map(StaticPlan::mesh_copy_path)
            .collect()
    }
}

#[derive(Debug)]
pub struct StaticConversionPlan {
    pub static_plans: Vec<StaticPlan>,
    pub matched_static_ids: HashSet<String>,
}

impl StaticConversionPlan {
    #[must_use]
    pub fn with_cell_plans(self, mut cell_plans: Vec<PluginCellPlan>) -> ConversionPlan {
        cell_plans.sort_by(|left, right| right.load_index.cmp(&left.load_index));
        let used_static_ids = cell_plans
            .iter()
            .flat_map(|cell_plan| cell_plan.used_static_ids.iter().cloned())
            .collect();

        ConversionPlan {
            static_plans: self.static_plans,
            cell_plans,
            matched_static_ids: self.matched_static_ids,
            used_static_ids,
        }
    }
}

/// Builds the full conversion plan from already-loaded plugins.
///
/// Mesh path validation is deferred until output work is derived from used statics, so unused
/// matching statics do not fail or pay mesh-copy costs.
#[must_use]
pub fn build_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> ConversionPlan {
    let static_plan = build_static_conversion_plan(loaded_plugins, config);
    let cell_plans = if static_plan.matched_static_ids.is_empty() {
        Vec::new()
    } else {
        scan_cells_parallel(loaded_plugins, &static_plan.matched_static_ids)
    };

    static_plan.with_cell_plans(cell_plans)
}

/// Builds the conversion plan portion that only needs `Header | Static` records.
///
/// This phase records winning static definitions only. Mesh validation is deliberately deferred
/// until a static is actually used by a converted exterior reference.
#[must_use]
pub fn build_static_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> StaticConversionPlan {
    let (static_plans, matched_static_ids) = collect_winning_statics(loaded_plugins, config);

    StaticConversionPlan {
        static_plans,
        matched_static_ids,
    }
}

fn collect_winning_statics(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> (Vec<StaticPlan>, HashSet<String>) {
    let mut seen_static_ids = HashSet::new();
    let mut matched_static_ids = HashSet::new();
    let mut static_plans = Vec::new();

    let mut plugins_in_reverse_load_order = loaded_plugins.iter().collect::<Vec<_>>();
    plugins_in_reverse_load_order.sort_by(|left, right| right.load_index.cmp(&left.load_index));

    for loaded in plugins_in_reverse_load_order {
        for static_record in loaded.plugin.objects_of_type::<Static>() {
            let lower_id = static_record.id.to_ascii_lowercase();
            if !seen_static_ids.insert(lower_id.clone()) || !config.matches_static_id(&lower_id) {
                continue;
            }

            matched_static_ids.insert(lower_id);

            static_plans.push(StaticPlan {
                source_load_index: loaded.load_index,
                source_plugin_name: loaded.plugin_name.clone(),
                source_plugin_path: loaded.plugin_path.clone(),
                source_master: loaded.source_master(),
                source_static: static_record.clone(),
            });
        }
    }

    (static_plans, matched_static_ids)
}

#[must_use]
pub fn scan_cells_parallel<S: BuildHasher + Sync>(
    loaded_plugins: &[LoadedPlugin],
    matched_static_ids: &HashSet<String, S>,
) -> Vec<PluginCellPlan> {
    loaded_plugins
        .par_iter()
        .map(|loaded| {
            let (groundcover_cells, deleted_cells, touched_refs, used_static_ids) =
                records::process_exterior_cells(&loaded.plugin, matched_static_ids);

            PluginCellPlan {
                load_index: loaded.load_index,
                plugin_name: loaded.plugin_name.clone(),
                plugin_path: loaded.plugin_path.clone(),
                source_master: loaded.source_master(),
                header_masters: loaded.header_masters(),
                groundcover_cells,
                deleted_cells,
                touched_refs,
                used_static_ids,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tes3::esp::{CellData, CellFlags, Reference, TES3Object};

    use super::*;

    fn static_record(id: &str, mesh: &str) -> Static {
        Static {
            id: id.to_owned(),
            mesh: mesh.to_owned(),
            ..Static::default()
        }
    }

    fn exterior_cell(name: &str, refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        let mut cell = Cell {
            name: name.to_owned(),
            data: CellData {
                grid: (1, 2),
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }

    fn interior_cell(name: &str, refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        let mut cell = Cell {
            name: name.to_owned(),
            data: CellData {
                flags: CellFlags::IS_INTERIOR,
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }

    fn reference(id: &str) -> Reference {
        Reference {
            id: id.to_owned(),
            translation: [1.0, 2.0, 3.0],
            ..Reference::default()
        }
    }

    fn loaded(load_index: usize, objects: Vec<TES3Object>) -> LoadedPlugin {
        LoadedPlugin {
            load_index,
            plugin_name: format!("plugin-{load_index}.esp"),
            plugin_path: PathBuf::from(format!("plugin-{load_index}.esp")),
            plugin: Plugin { objects },
        }
    }

    fn config() -> GroundcoverConfig {
        let mut config = GroundcoverConfig::default();
        config.grass_ids = vec!["grass".to_owned()];
        config.exclude = vec!["planter".to_owned()];
        config.compile_regex_sets().unwrap();
        config
    }

    fn master(name: &str, size: u64) -> MasterSpec {
        MasterSpec {
            name: name.to_owned(),
            size,
        }
    }

    #[test]
    fn source_index_zero_master_chain_includes_header_masters_then_source() {
        let cell_plan = PluginCellPlan {
            load_index: 0,
            plugin_name: "Bloodmoon.esm".to_owned(),
            plugin_path: PathBuf::from("Bloodmoon.esm"),
            source_master: master("Bloodmoon.esm", 2),
            header_masters: vec![master("Morrowind.esm", 1)],
            groundcover_cells: Vec::new(),
            deleted_cells: Vec::new(),
            touched_refs: 0,
            used_static_ids: BTreeSet::new(),
        };

        let chain = cell_plan.master_chain_for_source_index(0).unwrap();

        assert_eq!(chain[0].name, "Morrowind.esm");
        assert_eq!(chain[1].name, "Bloodmoon.esm");
    }

    #[test]
    fn header_master_index_chain_includes_prefix_through_target() {
        let cell_plan = PluginCellPlan {
            load_index: 0,
            plugin_name: "Patch.esp".to_owned(),
            plugin_path: PathBuf::from("Patch.esp"),
            source_master: master("Patch.esp", 3),
            header_masters: vec![master("Morrowind.esm", 1), master("Tribunal.esm", 2)],
            groundcover_cells: Vec::new(),
            deleted_cells: Vec::new(),
            touched_refs: 0,
            used_static_ids: BTreeSet::new(),
        };

        let chain = cell_plan.master_chain_for_source_index(2).unwrap();

        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].name, "Morrowind.esm");
        assert_eq!(chain[1].name, "Tribunal.esm");
        assert!(cell_plan.master_chain_for_source_index(3).is_none());
    }

    #[test]
    fn later_static_definition_wins_for_duplicate_ids() {
        let plugins = vec![
            loaded(
                0,
                vec![static_record("flora_grass_01", "flora\\grass.nif").into()],
            ),
            loaded(
                1,
                vec![static_record("flora_grass_01", "flora\\planter.nif").into()],
            ),
        ];

        let plan = build_conversion_plan(&plugins, &config());

        assert_eq!(plan.static_plans.len(), 1);
        assert_eq!(plan.static_plans[0].source_load_index, 1);
        assert_eq!(
            plan.static_plans[0].output_static().unwrap().mesh,
            "grass\\flora\\planter.nif"
        );
        assert!(plan.matched_static_ids.contains("flora_grass_01"));
    }

    #[test]
    fn existing_grass_meshes_are_planned_for_copy_when_used() {
        let plugins = vec![loaded(
            0,
            vec![
                static_record("flora_grass_01", "Grass\\Sky_Flora_GS_01_01.nif").into(),
                exterior_cell("", [((0, 1), reference("flora_grass_01"))]).into(),
            ],
        )];

        let plan = build_conversion_plan(&plugins, &config());
        let mesh_paths = plan.used_mesh_paths().unwrap();
        let mesh_path = mesh_paths.iter().next().unwrap();

        assert_eq!(mesh_paths.len(), 1);
        assert_eq!(mesh_path.source, "grass\\sky_flora_gs_01_01.nif");
        assert_eq!(mesh_path.target, "sky_flora_gs_01_01.nif");
        assert_eq!(
            plan.static_plans[0].output_static().unwrap().mesh,
            "Grass\\Sky_Flora_GS_01_01.nif"
        );
    }

    #[test]
    fn unused_matched_static_meshes_are_not_planned_for_copy() {
        let plugins = vec![loaded(
            0,
            vec![
                static_record("flora_grass_used", "flora\\used.nif").into(),
                static_record("flora_grass_unused", "..\\unsafe-missing.nif").into(),
                exterior_cell("", [((0, 1), reference("flora_grass_used"))]).into(),
            ],
        )];

        let plan = build_conversion_plan(&plugins, &config());
        let used_static_plans = plan.used_static_plans();
        let mesh_paths = plan.used_mesh_paths().unwrap();

        assert_eq!(plan.static_plans.len(), 2);
        assert_eq!(used_static_plans.len(), 1);
        assert_eq!(used_static_plans[0].source_static.id, "flora_grass_used");
        assert_eq!(mesh_paths.len(), 1);
        assert!(mesh_paths.contains(&mesh::MeshCopyPath {
            source: "flora\\used.nif".to_owned(),
            target: "flora\\used.nif".to_owned(),
        }));
    }

    #[test]
    fn exterior_matching_refs_are_copied_and_deleted() {
        let plugins = vec![loaded(
            0,
            vec![
                static_record("flora_grass_01", "flora\\grass.nif").into(),
                exterior_cell("", [((0, 1), reference("Flora_Grass_01"))]).into(),
            ],
        )];

        let plan = build_conversion_plan(&plugins, &config());

        assert_eq!(plan.static_plans.len(), 1);
        assert_eq!(
            plan.static_plans[0].output_static().unwrap().mesh,
            "grass\\flora\\grass.nif"
        );
        assert_eq!(plan.cell_plans[0].touched_refs, 1);
        assert_eq!(
            plan.used_static_ids,
            BTreeSet::from(["flora_grass_01".to_owned()])
        );
        assert_eq!(plan.cell_plans[0].groundcover_cells.len(), 1);
        assert_eq!(plan.cell_plans[0].deleted_cells.len(), 1);
        let deleted_ref = plan.cell_plans[0].deleted_cells[0]
            .references
            .get(&(0, 1))
            .unwrap();
        assert_eq!(deleted_ref.deleted, Some(true));
    }

    #[test]
    fn interior_matching_refs_are_ignored() {
        let plugins = vec![loaded(
            0,
            vec![
                static_record("flora_grass_01", "flora\\grass.nif").into(),
                interior_cell(
                    "Caius Cosades' House",
                    [((0, 1), reference("flora_grass_01"))],
                )
                .into(),
            ],
        )];

        let plan = build_conversion_plan(&plugins, &config());

        assert_eq!(plan.static_plans.len(), 1);
        assert_eq!(plan.cell_plans[0].touched_refs, 0);
        assert!(plan.cell_plans[0].groundcover_cells.is_empty());
        assert!(plan.cell_plans[0].deleted_cells.is_empty());
    }
}
