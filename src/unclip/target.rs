use std::collections::BTreeSet;

use tes3::esp::{Cell, Plugin};

use super::{args::UnclipPolicy, cells::CellCoord};

pub(crate) fn target_exterior_cells(plugin: &Plugin, policy: &UnclipPolicy) -> BTreeSet<CellCoord> {
    plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .filter(|cell| {
            cell.references
                .values()
                .any(|reference| policy.target_filter.includes(&reference.id))
        })
        .map(|cell| cell.data.grid)
        .collect()
}

pub(crate) fn target_reference_static_ids(
    plugin: &Plugin,
    policy: &UnclipPolicy,
) -> BTreeSet<String> {
    plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .flat_map(|cell| cell.references.values())
        .filter(|reference| reference.deleted != Some(true))
        .filter(|reference| policy.target_filter.includes(&reference.id))
        .map(|reference| reference.id.to_lowercase())
        .collect()
}

pub(crate) fn sorted_exterior_cells(plugin: &Plugin) -> Vec<&Cell> {
    let mut cells = plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .collect::<Vec<_>>();
    cells.sort_by_key(|cell| cell.data.grid);
    cells
}

pub(crate) fn target_exterior_ref_count(plugin: &Plugin) -> usize {
    plugin
        .objects_of_type::<Cell>()
        .filter(|cell| cell.is_exterior())
        .map(|cell| cell.references.len())
        .sum()
}

#[cfg(test)]
mod tests {
    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use crate::unclip::args::{RelocationPolicy, TargetFilter, UnclipPolicy, WriteActions};

    use super::{target_exterior_cells, target_exterior_ref_count, target_reference_static_ids};

    #[test]
    fn target_static_ids_follow_target_filter() {
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((1, 2), reference_with_id("flora_grass_01")),
                ((3, 4), reference_with_id("terrain_rock_01")),
            ]))],
        };
        let policy = test_policy_with_filter(&["flora_*"], &[]);

        let ids = target_reference_static_ids(&plugin, &policy);

        assert!(ids.contains("flora_grass_01"));
        assert!(!ids.contains("terrain_rock_01"));
    }

    #[test]
    fn target_exterior_ref_count_includes_filtered_and_deleted_refs() {
        let mut deleted = reference_with_id("flora_deleted");
        deleted.deleted = Some(true);
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((1, 2), reference_with_id("flora_grass_01")),
                ((3, 4), reference_with_id("terrain_rock_01")),
                ((5, 6), deleted),
            ]))],
        };

        assert_eq!(target_exterior_ref_count(&plugin), 3);
    }

    #[test]
    fn target_exterior_cells_follow_target_filter() {
        let plugin = Plugin {
            objects: vec![
                TES3Object::Cell(exterior_cell_at(
                    (1, 0),
                    [((1, 2), reference_with_id("flora_grass_01"))],
                )),
                TES3Object::Cell(exterior_cell_at(
                    (2, 0),
                    [((3, 4), reference_with_id("terrain_rock_01"))],
                )),
            ],
        };
        let policy = test_policy_with_filter(&["flora_*"], &[]);

        assert_eq!(
            target_exterior_cells(&plugin, &policy),
            std::collections::BTreeSet::from([(1, 0)])
        );
    }

    fn reference_with_id(id: &str) -> Reference {
        Reference {
            id: id.to_owned(),
            ..Reference::default()
        }
    }

    fn test_policy_with_filter(include_ids: &[&str], exclude_ids: &[&str]) -> UnclipPolicy {
        UnclipPolicy {
            write_actions: WriteActions::all(),
            contact_epsilon: 0.5,
            origin_epsilon: 0.5,
            orientation_epsilon_degrees: 1.0,
            relocation: RelocationPolicy {
                step: 32.0,
                steps: 8,
            },
            target_filter: TargetFilter::new(
                &include_ids
                    .iter()
                    .map(|pattern| (*pattern).to_owned())
                    .collect::<Vec<_>>(),
                &exclude_ids
                    .iter()
                    .map(|pattern| (*pattern).to_owned())
                    .collect::<Vec<_>>(),
            ),
        }
    }

    fn exterior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
        exterior_cell_at((0, 0), refs)
    }

    fn exterior_cell_at(
        grid: (i32, i32),
        refs: impl IntoIterator<Item = ((u32, u32), Reference)>,
    ) -> Cell {
        let mut cell = Cell {
            data: CellData {
                grid,
                ..CellData::default()
            },
            ..Cell::default()
        };
        cell.references.extend(refs);
        cell
    }
}
