// SPDX-License-Identifier: GPL-3.0-only

use std::{collections::BTreeSet, io};

use tes3::esp::{Plugin, Reference, TES3Object};

use crate::groundcover::CancellationToken;

use super::{args::UnclipPolicy, cells::CellCoord};

pub(crate) struct TargetRefIndex {
    pub(crate) target_cells: BTreeSet<CellCoord>,
    pub(crate) target_static_ids: BTreeSet<String>,
    pub(crate) exterior_ref_count: usize,
    cells: Vec<TargetRefCell>,
}

struct TargetRefCell {
    grid: CellCoord,
    object_index: usize,
    references: Vec<TargetRefEntry>,
}

struct TargetRefEntry {
    key: (u32, u32),
    normalized_id: String,
}

pub(crate) struct TargetRef<'a> {
    pub(crate) cell: CellCoord,
    pub(crate) key: (u32, u32),
    pub(crate) reference: &'a Reference,
    // Cached from `reference.id` during `TargetRefIndex::build`; target
    // reference IDs must not be mutated before `iter_ref_entries` is used.
    pub(crate) normalized_id: &'a str,
}

impl TargetRefIndex {
    pub(crate) fn build(
        plugin: &Plugin,
        policy: &UnclipPolicy,
        cancellation: &CancellationToken,
    ) -> io::Result<Self> {
        let mut target_cells = BTreeSet::new();
        let mut target_static_ids = BTreeSet::new();
        let mut exterior_ref_count = 0;
        let mut cells = Vec::new();

        for (object_index, object) in plugin.objects.iter().enumerate() {
            super::check_cancellation(cancellation)?;
            let TES3Object::Cell(cell) = object else {
                continue;
            };
            if !cell.is_exterior() {
                continue;
            }

            exterior_ref_count += cell.references.len();
            let mut references = Vec::new();
            for (key, reference) in &cell.references {
                if !policy.target_filter.includes(&reference.id) {
                    continue;
                }
                let normalized_id = reference.id.to_lowercase();
                references.push(TargetRefEntry {
                    key: *key,
                    normalized_id: normalized_id.clone(),
                });
                if reference.deleted != Some(true) {
                    target_static_ids.insert(normalized_id);
                }
            }
            if references.is_empty() {
                continue;
            }

            references.sort_by_key(|reference| reference.key);
            target_cells.insert(cell.data.grid);
            cells.push(TargetRefCell {
                grid: cell.data.grid,
                object_index,
                references,
            });
        }

        cells.sort_by_key(|cell| (cell.grid, cell.object_index));

        Ok(Self {
            target_cells,
            target_static_ids,
            exterior_ref_count,
            cells,
        })
    }

    #[cfg(test)]
    pub(crate) fn iter_refs<'a>(
        &'a self,
        plugin: &'a Plugin,
    ) -> impl Iterator<Item = (CellCoord, (u32, u32), &'a Reference)> + 'a {
        self.iter_ref_entries(plugin)
            .map(|entry| (entry.cell, entry.key, entry.reference))
    }

    pub(crate) fn iter_ref_entries<'a>(
        &'a self,
        plugin: &'a Plugin,
    ) -> impl Iterator<Item = TargetRef<'a>> + 'a {
        self.cells.iter().flat_map(|target_cell| {
            let TES3Object::Cell(cell) = &plugin.objects[target_cell.object_index] else {
                unreachable!("target ref index cell should still point to a CELL")
            };
            target_cell.references.iter().filter_map(move |entry| {
                cell.references.get(&entry.key).map(|reference| TargetRef {
                    cell: target_cell.grid,
                    key: entry.key,
                    reference,
                    normalized_id: &entry.normalized_id,
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use tes3::esp::{Cell, CellData, Plugin, Reference, TES3Object};

    use crate::unclip::args::{
        IdFilter, RelocationPolicy, RoadTextureFilter, UnclipPolicy, WriteActions,
    };

    use super::TargetRefIndex;

    #[test]
    fn target_static_ids_follow_target_filter() {
        let mut deleted = reference_with_id("flora_deleted");
        deleted.deleted = Some(true);
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((1, 2), reference_with_id("flora_grass_01")),
                ((3, 4), reference_with_id("terrain_rock_01")),
                ((5, 6), deleted),
            ]))],
        };
        let policy = test_policy_with_filter(&["^flora_.*"], &[]);

        let ids = TargetRefIndex::build(
            &plugin,
            &policy,
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap()
        .target_static_ids;

        assert!(ids.contains("flora_grass_01"));
        assert!(!ids.contains("flora_deleted"));
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

        let policy = test_policy_with_filter(&[], &[]);

        assert_eq!(
            TargetRefIndex::build(
                &plugin,
                &policy,
                &crate::groundcover::CancellationToken::default()
            )
            .unwrap()
            .exterior_ref_count,
            3
        );
    }

    #[test]
    fn target_exterior_cells_follow_target_filter() {
        let mut deleted = reference_with_id("flora_deleted");
        deleted.deleted = Some(true);
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
                TES3Object::Cell(exterior_cell_at((3, 0), [((5, 6), deleted)])),
            ],
        };
        let policy = test_policy_with_filter(&["^flora_.*"], &[]);

        assert_eq!(
            TargetRefIndex::build(
                &plugin,
                &policy,
                &crate::groundcover::CancellationToken::default()
            )
            .unwrap()
            .target_cells,
            std::collections::BTreeSet::from([(1, 0), (3, 0)])
        );
    }

    #[test]
    fn target_ref_index_sorts_duplicate_cells_by_grid_object_and_key() {
        let plugin = Plugin {
            objects: vec![
                TES3Object::Cell(exterior_cell_at(
                    (2, 0),
                    [
                        ((4, 0), reference_with_id("flora_late_key")),
                        ((1, 0), reference_with_id("flora_early_key")),
                    ],
                )),
                TES3Object::Cell(exterior_cell_at(
                    (1, 0),
                    [((3, 0), reference_with_id("flora_other_cell"))],
                )),
                TES3Object::Cell(exterior_cell_at(
                    (2, 0),
                    [((2, 0), reference_with_id("flora_duplicate_cell"))],
                )),
            ],
        };
        let policy = test_policy_with_filter(&["^flora_.*"], &[]);

        let index = TargetRefIndex::build(
            &plugin,
            &policy,
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();
        let refs = index
            .iter_refs(&plugin)
            .map(|(grid, key, reference)| (grid, key, reference.id.as_str()))
            .collect::<Vec<_>>();

        assert_eq!(
            refs,
            vec![
                ((1, 0), (3, 0), "flora_other_cell"),
                ((2, 0), (1, 0), "flora_early_key"),
                ((2, 0), (4, 0), "flora_late_key"),
                ((2, 0), (2, 0), "flora_duplicate_cell"),
            ]
        );
    }

    #[test]
    fn target_ref_entries_reuse_normalized_ids_in_stable_order() {
        let plugin = Plugin {
            objects: vec![TES3Object::Cell(exterior_cell([
                ((2, 0), reference_with_id("Flora_Late")),
                ((1, 0), reference_with_id("FLORA_Early")),
            ]))],
        };
        let policy = test_policy_with_filter(&["^flora_.*"], &[]);
        let index = TargetRefIndex::build(
            &plugin,
            &policy,
            &crate::groundcover::CancellationToken::default(),
        )
        .unwrap();

        let entries = index
            .iter_ref_entries(&plugin)
            .map(|entry| (entry.key, entry.normalized_id))
            .collect::<Vec<_>>();

        assert_eq!(
            entries,
            vec![((1, 0), "flora_early"), ((2, 0), "flora_late")]
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
            origin_epsilon: 0.5,
            orientation_epsilon_degrees: 1.0,
            relocation: RelocationPolicy {
                step: 32.0,
                steps: 8,
            },
            target_filter: IdFilter::new(
                &include_ids
                    .iter()
                    .map(|pattern| (*pattern).to_owned())
                    .collect::<Vec<_>>(),
                &exclude_ids
                    .iter()
                    .map(|pattern| (*pattern).to_owned())
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
            occluder_filter: IdFilter::new(&[], &[]).unwrap(),
            road_texture_filter: RoadTextureFilter::new(&[], &[]).unwrap(),
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
