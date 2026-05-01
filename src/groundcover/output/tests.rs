use std::{
    collections::{BTreeSet, HashSet},
    path::PathBuf,
};

use tes3::esp::{Cell, Reference, Static};

use crate::groundcover::plan::{ConversionPlan, MasterSpec, PluginCellPlan, StaticPlan};

use super::*;

fn master(name: &str, size: u64) -> MasterSpec {
    MasterSpec {
        name: name.to_owned(),
        size,
    }
}

fn static_plan(source_master: MasterSpec, source_id: &str, generated_id: &str) -> StaticPlan {
    StaticPlan {
        source_load_index: 0,
        source_plugin_name: source_master.name.clone(),
        source_plugin_path: PathBuf::from(&source_master.name),
        source_master,
        source_static: Static {
            id: source_id.to_owned(),
            mesh: "flora\\grass.nif".to_owned(),
            ..Static::default()
        },
        generated_id: generated_id.to_owned(),
    }
}

fn reference(id: &str, mast_index: u32) -> Reference {
    Reference {
        id: id.to_owned(),
        mast_index,
        ..Reference::default()
    }
}

fn cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
    let mut cell = Cell::default();
    cell.references.extend(refs);
    cell
}

fn cell_plan(
    load_index: usize,
    source_master: MasterSpec,
    header_masters: Vec<MasterSpec>,
    groundcover_cell: Cell,
    used_static_id: &str,
) -> PluginCellPlan {
    PluginCellPlan {
        load_index,
        plugin_name: source_master.name.clone(),
        plugin_path: PathBuf::from(&source_master.name),
        source_master,
        header_masters,
        groundcover_cells: vec![groundcover_cell.clone()],
        deleted_cells: vec![groundcover_cell],
        touched_refs: 1,
        used_static_ids: BTreeSet::from([used_static_id.to_owned()]),
    }
}

fn plan(
    static_plans: Vec<StaticPlan>,
    cell_plans: Vec<PluginCellPlan>,
    used_static_ids: BTreeSet<String>,
) -> ConversionPlan {
    ConversionPlan {
        static_plans,
        cell_plans,
        matched_static_ids: HashSet::new(),
        used_static_ids,
    }
}

#[test]
fn empty_plan_builds_empty_plugins() {
    let built = build_plugins(&plan(Vec::new(), Vec::new(), BTreeSet::new())).unwrap();

    assert!(built.groundcover_plugin.objects.is_empty());
    assert!(built.deleted_plugin.objects.is_empty());
    assert!(built.groundcover_header.masters.is_empty());
    assert!(built.deleted_header.masters.is_empty());
}

#[test]
fn copied_cell_refs_are_remapped_to_generated_master_indices() {
    let source_master = master("Source.esp", 42);
    let cell = cell([((0, 7), reference("flora_grass_01", 0))]);
    let plan = plan(
        vec![static_plan(
            source_master.clone(),
            "flora_grass_01",
            "gm_test_flora_grass_01",
        )],
        vec![cell_plan(
            0,
            source_master,
            Vec::new(),
            cell,
            "flora_grass_01",
        )],
        BTreeSet::from(["flora_grass_01".to_owned()]),
    );

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
    let morrowind_master = master("Morrowind.esm", 79_837_557);
    let bloodmoon_master = master("Bloodmoon.esm", 9_631_798);
    let cell = cell([((0, 7), reference("flora_grass_01", 0))]);
    let plan = plan(
        vec![static_plan(
            bloodmoon_master.clone(),
            "flora_grass_01",
            "gm_flora_grass_01",
        )],
        vec![cell_plan(
            1,
            bloodmoon_master,
            vec![morrowind_master],
            cell,
            "flora_grass_01",
        )],
        BTreeSet::from(["flora_grass_01".to_owned()]),
    );

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
    let morrowind_master = master("Morrowind.esm", 79_837_557);
    let bloodmoon_master = master("Bloodmoon.esm", 9_631_798);
    let morrowind_cell = cell([((0, 7), reference("flora_grass_01", 0))]);
    let bloodmoon_cell = cell([((0, 8), reference("flora_grass_02", 0))]);
    let plan = plan(
        vec![
            static_plan(
                morrowind_master.clone(),
                "flora_grass_01",
                "gm_flora_grass_01",
            ),
            StaticPlan {
                source_load_index: 2,
                ..static_plan(
                    bloodmoon_master.clone(),
                    "flora_grass_02",
                    "gm_flora_grass_02",
                )
            },
        ],
        vec![
            cell_plan(
                2,
                bloodmoon_master,
                Vec::new(),
                bloodmoon_cell,
                "flora_grass_02",
            ),
            cell_plan(
                0,
                morrowind_master,
                Vec::new(),
                morrowind_cell,
                "flora_grass_01",
            ),
        ],
        BTreeSet::from(["flora_grass_01".to_owned(), "flora_grass_02".to_owned()]),
    );

    let built = build_plugins(&plan).unwrap();

    assert_eq!(
        built.groundcover_header.masters,
        vec![
            ("Morrowind.esm".to_owned(), 79_837_557),
            ("Bloodmoon.esm".to_owned(), 9_631_798)
        ]
    );
}

#[test]
fn header_master_refs_use_exact_owner_master_only() {
    let patch_master = master("Patch.esp", 10);
    let tribunal_master = master("Tribunal.esm", 4_568_965);
    let cell = cell([((2, 7), reference("flora_grass_01", 2))]);
    let plan = plan(
        vec![static_plan(
            patch_master.clone(),
            "flora_grass_01",
            "gm_flora_grass_01",
        )],
        vec![cell_plan(
            2,
            patch_master,
            vec![master("Morrowind.esm", 79_837_557), tribunal_master],
            cell,
            "flora_grass_01",
        )],
        BTreeSet::from(["flora_grass_01".to_owned()]),
    );

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
    let plan = plan(
        vec![static_plan(
            master("StaticOnly.esp", 5),
            "flora_grass_unused",
            "gm_unused",
        )],
        Vec::new(),
        BTreeSet::new(),
    );

    let built = build_plugins(&plan).unwrap();

    assert!(built.groundcover_plugin.objects.is_empty());
    assert!(built.groundcover_header.masters.is_empty());
    assert!(built.deleted_header.masters.is_empty());
}

#[test]
fn generated_master_count_over_esp_reference_limit_fails() {
    let cell_plans = (0..=255)
        .map(|index| {
            let source_master =
                master(&format!("Source{index}.esp"), u64::try_from(index).unwrap());
            cell_plan(
                index,
                source_master,
                Vec::new(),
                cell([(
                    (0, u32::try_from(index).unwrap()),
                    reference(&format!("flora_grass_{index}"), 0),
                )]),
                &format!("flora_grass_{index}"),
            )
        })
        .collect();
    let plan = plan(Vec::new(), cell_plans, BTreeSet::new());

    let error = build_plugins(&plan).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("only support 255"));
}
