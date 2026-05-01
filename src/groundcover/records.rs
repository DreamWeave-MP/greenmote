use std::{collections::HashSet, hash::BuildHasher};

use tes3::esp::{Cell, Plugin, Reference};

#[must_use]
pub fn process_exterior_cells<S: BuildHasher>(
    plugin: &Plugin,
    matched_static_ids: &HashSet<String, S>,
) -> (Vec<Cell>, Vec<Cell>, usize) {
    let mut groundcover_cells = Vec::new();
    let mut deleted_cells = Vec::new();
    let mut touched_refs = 0;

    for cell in plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
    {
        let matching_refs = cell
            .references
            .iter()
            .filter(|(_, reference)| {
                matched_static_ids.contains(&reference.id.to_ascii_lowercase())
            })
            .collect::<Vec<_>>();

        if matching_refs.is_empty() {
            continue;
        }

        let mut groundcover_cell = minimal_cell_shell(cell);
        let mut deleted_cell = minimal_cell_shell(cell);

        for (key, reference) in matching_refs {
            groundcover_cell.references.insert(*key, reference.clone());

            let mut deleted_reference = reference.clone();
            mark_reference_deleted(&mut deleted_reference);
            deleted_cell.references.insert(*key, deleted_reference);
            touched_refs += 1;
        }

        groundcover_cells.push(groundcover_cell);
        deleted_cells.push(deleted_cell);
    }

    (groundcover_cells, deleted_cells, touched_refs)
}

fn minimal_cell_shell(source: &Cell) -> Cell {
    let mut cell = source.clone();
    cell.references.clear();
    cell
}

fn mark_reference_deleted(reference: &mut Reference) {
    reference.deleted = Some(true);
}
