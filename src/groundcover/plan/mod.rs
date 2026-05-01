mod cells;
mod ids;
mod model;
mod statics;

pub use cells::scan_cells_parallel;
pub use model::{
    ConversionPlan, LoadedPlugin, MasterSpec, PluginCellPlan, StaticConversionPlan, StaticPlan,
};
pub use statics::build_static_conversion_plan;

use crate::groundcover::GroundcoverConfig;

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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, path::PathBuf};

    use tes3::esp::{Cell, CellData, CellFlags, Reference, Static, TES3Object};

    use crate::groundcover::mesh;

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
            plugin: tes3::esp::Plugin { objects },
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
    fn source_index_zero_resolves_to_source_plugin() {
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

        let master = cell_plan.master_for_source_index(0).unwrap();

        assert_eq!(master.name, "Bloodmoon.esm");
    }

    #[test]
    fn header_master_index_resolves_to_exact_owner() {
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

        let master = cell_plan.master_for_source_index(2).unwrap();

        assert_eq!(master.name, "Tribunal.esm");
        assert!(cell_plan.master_for_source_index(3).is_none());
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
            plan.static_plans[0].output_static().unwrap().id,
            plan.static_plans[0].generated_id
        );
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
            plan.static_plans[0].output_static().unwrap().id,
            plan.static_plans[0].generated_id
        );
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
