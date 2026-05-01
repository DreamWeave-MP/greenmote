use std::{
    collections::{BTreeSet, HashSet},
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
    pub output_static: Static,
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
    pub mesh_paths: BTreeSet<String>,
}

#[derive(Debug)]
pub struct StaticConversionPlan {
    pub static_plans: Vec<StaticPlan>,
    pub matched_static_ids: HashSet<String>,
    pub mesh_paths: BTreeSet<String>,
}

impl StaticConversionPlan {
    #[must_use]
    pub fn with_cell_plans(self, mut cell_plans: Vec<PluginCellPlan>) -> ConversionPlan {
        cell_plans.sort_by(|left, right| right.load_index.cmp(&left.load_index));

        ConversionPlan {
            static_plans: self.static_plans,
            cell_plans,
            matched_static_ids: self.matched_static_ids,
            mesh_paths: self.mesh_paths,
        }
    }
}

/// Builds the full conversion plan from already-loaded plugins.
///
/// # Errors
///
/// Returns invalid input if a matched static uses an unsafe mesh path.
pub fn build_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> io::Result<ConversionPlan> {
    let static_plan = build_static_conversion_plan(loaded_plugins, config)?;
    let cell_plans = if static_plan.matched_static_ids.is_empty() {
        Vec::new()
    } else {
        scan_cells_parallel(loaded_plugins, &static_plan.matched_static_ids)
    };

    Ok(static_plan.with_cell_plans(cell_plans))
}

/// Builds the conversion plan portion that only needs `Header | Static` records.
///
/// # Errors
///
/// Returns invalid input if a matched static uses an unsafe mesh path.
pub fn build_static_conversion_plan(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> io::Result<StaticConversionPlan> {
    let (static_plans, matched_static_ids, mesh_paths) =
        collect_winning_statics(loaded_plugins, config)?;

    Ok(StaticConversionPlan {
        static_plans,
        matched_static_ids,
        mesh_paths,
    })
}

fn collect_winning_statics(
    loaded_plugins: &[LoadedPlugin],
    config: &GroundcoverConfig,
) -> io::Result<(Vec<StaticPlan>, HashSet<String>, BTreeSet<String>)> {
    let mut seen_static_ids = HashSet::new();
    let mut matched_static_ids = HashSet::new();
    let mut static_plans = Vec::new();
    let mut mesh_paths = BTreeSet::new();

    let mut plugins_in_reverse_load_order = loaded_plugins.iter().collect::<Vec<_>>();
    plugins_in_reverse_load_order.sort_by(|left, right| right.load_index.cmp(&left.load_index));

    for loaded in plugins_in_reverse_load_order {
        for static_record in loaded.plugin.objects_of_type::<Static>() {
            let lower_id = static_record.id.to_ascii_lowercase();
            if !seen_static_ids.insert(lower_id.clone()) || !config.matches_static_id(&lower_id) {
                continue;
            }

            matched_static_ids.insert(lower_id);

            if let Some(mesh_path) = mesh::normalize_mesh_for_copy(&static_record.mesh)? {
                mesh_paths.insert(mesh_path);
            }

            let mut output_static = static_record.clone();
            output_static.mesh = mesh::grass_prefixed_mesh(&output_static.mesh)?;

            static_plans.push(StaticPlan {
                source_load_index: loaded.load_index,
                source_plugin_name: loaded.plugin_name.clone(),
                source_plugin_path: loaded.plugin_path.clone(),
                source_master: loaded.source_master(),
                output_static,
            });
        }
    }

    Ok((static_plans, matched_static_ids, mesh_paths))
}

#[must_use]
pub fn scan_cells_parallel<S: BuildHasher + Sync>(
    loaded_plugins: &[LoadedPlugin],
    matched_static_ids: &HashSet<String, S>,
) -> Vec<PluginCellPlan> {
    loaded_plugins
        .par_iter()
        .map(|loaded| {
            let (groundcover_cells, deleted_cells, touched_refs) =
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

        let plan = build_conversion_plan(&plugins, &config()).unwrap();

        assert_eq!(plan.static_plans.len(), 1);
        assert_eq!(plan.static_plans[0].source_load_index, 1);
        assert_eq!(
            plan.static_plans[0].output_static.mesh,
            "grass\\flora\\planter.nif"
        );
        assert!(plan.matched_static_ids.contains("flora_grass_01"));
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

        let plan = build_conversion_plan(&plugins, &config()).unwrap();

        assert_eq!(plan.static_plans.len(), 1);
        assert_eq!(
            plan.static_plans[0].output_static.mesh,
            "grass\\flora\\grass.nif"
        );
        assert_eq!(plan.cell_plans[0].touched_refs, 1);
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

        let plan = build_conversion_plan(&plugins, &config()).unwrap();

        assert_eq!(plan.static_plans.len(), 1);
        assert_eq!(plan.cell_plans[0].touched_refs, 0);
        assert!(plan.cell_plans[0].groundcover_cells.is_empty());
        assert!(plan.cell_plans[0].deleted_cells.is_empty());
    }
}
