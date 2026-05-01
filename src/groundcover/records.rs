use std::collections::HashSet;

use tes3::esp::{Cell, Plugin, Reference};

#[must_use]
pub fn process_exterior_cells(
    plugin: &Plugin,
    matched_static_ids: &HashSet<String>,
) -> (Vec<Cell>, Vec<Cell>, usize) {
    let mut groundcover_cells = Vec::new();
    let mut deleted_cells = Vec::new();
    let mut touched_refs = 0;

    for cell in plugin.objects_of_type::<Cell>().filter(|cell| cell.is_exterior()) {
        let matching_refs = cell
            .references
            .iter()
            .filter(|(_, reference)| matched_static_ids.contains(&reference.id.to_ascii_lowercase()))
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
    Cell {
        flags: source.flags,
        name: source.name.clone(),
        data: source.data.clone(),
        region: source.region.clone(),
        map_color: source.map_color,
        water_height: source.water_height,
        atmosphere_data: source.atmosphere_data.clone(),
        references: Default::default(),
    }
}

fn mark_reference_deleted(reference: &mut Reference) {
    reference.deleted = Some(true);
}
