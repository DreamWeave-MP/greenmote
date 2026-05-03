use std::{
    collections::{BTreeSet, HashSet},
    hash::BuildHasher,
};

use tes3::esp::{Cell, Plugin, Reference};

#[must_use]
pub fn process_exterior_cells<S: BuildHasher>(
    plugin: &Plugin,
    matched_static_ids: &HashSet<String, S>,
) -> (Vec<Cell>, Vec<Cell>, usize, BTreeSet<String>) {
    let mut groundcover_cells = Vec::new();
    let mut deleted_cells = Vec::new();
    let mut touched_refs = 0;
    let mut used_static_ids = BTreeSet::new();

    for cell in plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
    {
        let mut groundcover_cell = None;
        let mut deleted_cell = None;

        for (key, reference) in &cell.references {
            let static_id = reference.id.to_ascii_lowercase();
            if !matched_static_ids.contains(&static_id) {
                continue;
            }

            let groundcover_cell = groundcover_cell
                .get_or_insert_with(|| minimal_cell_shell(cell, cell.references.len()));
            let deleted_cell =
                deleted_cell.get_or_insert_with(|| minimal_cell_shell(cell, cell.references.len()));

            used_static_ids.insert(static_id);
            groundcover_cell.references.insert(*key, reference.clone());

            let mut deleted_reference = reference.clone();
            mark_reference_deleted(&mut deleted_reference);
            deleted_cell.references.insert(*key, deleted_reference);
            touched_refs += 1;
        }

        if let (Some(groundcover_cell), Some(deleted_cell)) = (groundcover_cell, deleted_cell) {
            groundcover_cells.push(groundcover_cell);
            deleted_cells.push(deleted_cell);
        }
    }

    (
        groundcover_cells,
        deleted_cells,
        touched_refs,
        used_static_ids,
    )
}

fn minimal_cell_shell(source: &Cell, reference_capacity: usize) -> Cell {
    let mut cell = Cell {
        flags: source.flags,
        name: source.name.clone(),
        data: source.data.clone(),
        region: source.region.clone(),
        map_color: source.map_color,
        water_height: source.water_height,
        atmosphere_data: source.atmosphere_data.clone(),
        ..Cell::default()
    };
    cell.references.reserve(reference_capacity);
    cell
}

fn mark_reference_deleted(reference: &mut Reference) {
    reference.deleted = Some(true);
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tes3::esp::{CellData, CellFlags};

    use super::*;

    fn reference(id: &str) -> Reference {
        Reference {
            id: id.to_owned(),
            translation: [1.0, 2.0, 3.0],
            ..Reference::default()
        }
    }

    fn exterior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        let mut cell = Cell {
            name: "Ascadian Isles Region".to_owned(),
            data: CellData {
                grid: (3, -4),
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }

    fn interior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        let mut cell = Cell {
            name: "Balmora, Caius Cosades' House".to_owned(),
            data: CellData {
                flags: CellFlags::IS_INTERIOR,
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }

    #[test]
    fn touched_exterior_cells_keep_only_matching_refs_and_metadata() {
        let plugin = Plugin {
            objects: vec![
                exterior_cell([
                    ((0, 1), reference("Flora_Grass_01")),
                    ((0, 2), reference("crate_01")),
                ])
                .into(),
            ],
        };
        let matched_static_ids = HashSet::from(["flora_grass_01".to_owned()]);

        let (groundcover_cells, deleted_cells, touched_refs, used_static_ids) =
            process_exterior_cells(&plugin, &matched_static_ids);

        assert_eq!(touched_refs, 1);
        assert_eq!(
            used_static_ids,
            BTreeSet::from(["flora_grass_01".to_owned()])
        );
        assert_eq!(groundcover_cells.len(), 1);
        assert_eq!(groundcover_cells[0].name, "Ascadian Isles Region");
        assert_eq!(groundcover_cells[0].data.grid, (3, -4));
        assert!(groundcover_cells[0].references.contains_key(&(0, 1)));
        assert!(!groundcover_cells[0].references.contains_key(&(0, 2)));
        assert_eq!(deleted_cells[0].references[&(0, 1)].deleted, Some(true));
    }

    #[test]
    fn matching_refs_in_interior_cells_are_ignored() {
        let plugin = Plugin {
            objects: vec![interior_cell([((0, 1), reference("flora_grass_01"))]).into()],
        };
        let matched_static_ids = HashSet::from(["flora_grass_01".to_owned()]);

        let (groundcover_cells, deleted_cells, touched_refs, used_static_ids) =
            process_exterior_cells(&plugin, &matched_static_ids);

        assert_eq!(touched_refs, 0);
        assert!(used_static_ids.is_empty());
        assert!(groundcover_cells.is_empty());
        assert!(deleted_cells.is_empty());
    }

    #[test]
    fn mixed_case_reference_ids_match_lowercase_static_ids() {
        let plugin = Plugin {
            objects: vec![exterior_cell([((0, 1), reference("FlOrA_GrAsS_01"))]).into()],
        };
        let matched_static_ids = HashSet::from(["flora_grass_01".to_owned()]);

        let (groundcover_cells, _, touched_refs, used_static_ids) =
            process_exterior_cells(&plugin, &matched_static_ids);

        assert_eq!(touched_refs, 1);
        assert_eq!(
            used_static_ids,
            BTreeSet::from(["flora_grass_01".to_owned()])
        );
        assert!(groundcover_cells[0].references.contains_key(&(0, 1)));
    }
}
