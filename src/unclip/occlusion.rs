// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;

use super::{cells::CellCoord, mesh::WorldAabb, physics::RapierCollider};

const CELL_SIZE: f32 = 8192.0;
const MAX_INDEXED_CELL_SPAN: i32 = 32;

#[derive(Clone)]
pub(crate) struct StaticOccluder {
    pub(crate) id: String,
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) bounds: WorldAabb,
    pub(crate) collider: RapierCollider,
}

impl StaticOccluder {
    /// Static id plus the reference that placed it, for reports.
    pub(crate) fn describe(&self) -> String {
        format!(
            "{} (ref {}:{} in cell {},{})",
            self.id, self.reference_key[0], self.reference_key[1], self.cell[0], self.cell[1]
        )
    }
}

#[cfg(test)]
impl StaticOccluderIndex {
    pub(crate) fn intersects_shape(&self, collider: &RapierCollider) -> bool {
        self.candidates_for(collider.bounds())
            .iter()
            .any(|occluder| occluder.collider.intersects(collider))
    }

    pub(crate) fn intersects_volume(&self, bounds: WorldAabb) -> bool {
        self.intersects_shape(&RapierCollider::from_world_aabb(bounds))
    }
}

#[cfg(test)]
impl StaticOccluder {
    pub(crate) fn new_for_test(id: &str, collider: RapierCollider) -> Self {
        Self {
            id: id.to_owned(),
            cell: [0, 0],
            reference_key: [0, 0],
            bounds: collider.bounds(),
            collider,
        }
    }
}

#[derive(Default)]
pub(crate) struct StaticOccluderIndex {
    occluders: Vec<StaticOccluder>,
    cells: BTreeMap<CellCoord, Vec<usize>>,
    large_occluders: Vec<usize>,
}

impl StaticOccluderIndex {
    pub(crate) fn new(occluders: Vec<StaticOccluder>) -> Self {
        let occluders = occluders
            .into_iter()
            .map(|mut occluder| {
                // Keep this as the index/query broad bounds for the actual collider shape.
                occluder.bounds = occluder.collider.bounds();
                occluder
            })
            .collect::<Vec<_>>();
        let mut cells = BTreeMap::<CellCoord, Vec<usize>>::new();
        let mut large_occluders = Vec::new();
        for (index, occluder) in occluders.iter().enumerate() {
            if let Some(occluder_cells) = cell_span_for_bounds(occluder.bounds) {
                for cell in occluder_cells {
                    cells.entry(cell).or_default().push(index);
                }
            } else {
                large_occluders.push(index);
            }
        }
        Self {
            occluders,
            cells,
            large_occluders,
        }
    }

    pub(crate) fn candidates_for(&self, bounds: WorldAabb) -> Vec<&StaticOccluder> {
        let mut indices = Vec::new();
        if let Some(cells) = cell_span_for_bounds(bounds) {
            for cell in cells {
                if let Some(cell_indices) = self.cells.get(&cell) {
                    indices.extend_from_slice(cell_indices);
                }
            }
            indices.extend(self.large_occluders.iter().copied());
        } else {
            indices.extend(0..self.occluders.len());
        }
        indices.sort_unstable();
        indices.dedup();
        indices
            .into_iter()
            .filter_map(|index| self.occluders.get(index))
            .filter(|occluder| occluder.bounds.intersects_xy(bounds))
            .collect()
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cell_span_for_bounds(bounds: WorldAabb) -> Option<CellSpan> {
    let min_x = cell_coord(bounds.min[0]);
    let min_y = cell_coord(bounds.min[1]);
    let max_x = exclusive_max_cell_coord(bounds.max[0]);
    let max_y = exclusive_max_cell_coord(bounds.max[1]);
    if i64::from(max_x) - i64::from(min_x) >= i64::from(MAX_INDEXED_CELL_SPAN)
        || i64::from(max_y) - i64::from(min_y) >= i64::from(MAX_INDEXED_CELL_SPAN)
    {
        return None;
    }
    Some(CellSpan {
        min_x,
        max_x,
        max_y,
        next_x: min_x,
        next_y: min_y,
    })
}

#[allow(clippy::cast_possible_truncation)]
fn cell_coord(position: f32) -> i32 {
    (position / CELL_SIZE).floor() as i32
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn exclusive_max_cell_coord(position: f32) -> i32 {
    ((position / CELL_SIZE).ceil() as i32).saturating_sub(1)
}

#[derive(Clone)]
struct CellSpan {
    min_x: i32,
    max_x: i32,
    max_y: i32,
    next_x: i32,
    next_y: i32,
}

impl Iterator for CellSpan {
    type Item = CellCoord;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next_y > self.max_y || self.next_x > self.max_x {
            return None;
        }

        let cell = (self.next_x, self.next_y);
        if self.next_x == self.max_x {
            self.next_x = self.min_x;
            self.next_y += 1;
        } else {
            self.next_x += 1;
        }
        Some(cell)
    }
}

#[cfg(test)]
mod tests {
    use super::{StaticOccluder, StaticOccluderIndex, cell_span_for_bounds};
    use crate::unclip::{
        mesh::{LocalObb, MeshAabb, MeshColliderParts, WorldAabb},
        physics::RapierCollider,
    };
    use glam::Quat;

    #[test]
    fn static_occluder_index_finds_cross_cell_occluders() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [8190.0, 0.0, 0.0],
            [8200.0, 10.0, 10.0],
        ))]);

        assert_eq!(
            occluders
                .candidates_for(aabb([8195.0, 0.0, 0.0], [8205.0, 10.0, 10.0]))
                .len(),
            1
        );
    }

    #[test]
    fn static_occluder_index_keeps_candidate_order_by_occluder_index() {
        let occluders = StaticOccluderIndex::new(vec![
            static_occluder_with_id("first", aabb([8193.0, 0.0, 0.0], [8200.0, 10.0, 10.0])),
            static_occluder_with_id("second", aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])),
        ]);

        let ids = occluders
            .candidates_for(aabb([0.0, 0.0, 0.0], [8200.0, 10.0, 10.0]))
            .into_iter()
            .map(|occluder| occluder.id.as_str())
            .collect::<Vec<_>>();

        assert_eq!(ids, ["first", "second"]);
    }

    #[test]
    fn static_occluder_index_intersects_cross_cell_occluders_without_dedup() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [8190.0, 0.0, 0.0],
            [8200.0, 10.0, 10.0],
        ))]);

        let query = aabb([8191.0, 0.0, 0.0], [8193.0, 10.0, 10.0]);

        assert_eq!(occluders.candidates_for(query).len(), 1);
        assert!(occluders.intersects_volume(query));
    }

    #[test]
    fn static_occluder_index_uses_collider_bounds_when_visual_bounds_are_smaller() {
        let visual_bounds = mesh_aabb([8188.0, -1.0, -1.0], [8191.0, 1.0, 1.0])
            .world_aabb([0.0; 3], [0.0; 3], None);
        let collider_parts = MeshColliderParts::from_local_obbs([LocalObb {
            center: [8193.0, 0.0, 0.0],
            half_extents: [3.0, 1.0, 1.0],
            orientation: Quat::from_rotation_z(std::f32::consts::FRAC_PI_4),
        }]);
        let collider =
            RapierCollider::from_mesh_collider_parts(&collider_parts, [0.0; 3], [0.0; 3], None);
        assert!(collider.bounds().max[0] > visual_bounds.max[0]);

        let occluders = StaticOccluderIndex::new(vec![StaticOccluder {
            id: "rock".to_owned(),
            cell: [0, 0],
            reference_key: [1, 2],
            bounds: visual_bounds,
            collider: collider.clone(),
        }]);
        let query = RapierCollider::from_world_aabb(aabb([8192.0, -0.5, -0.5], [8192.5, 0.5, 0.5]));

        assert_eq!(occluders.candidates_for(query.bounds()).len(), 1);
        assert!(occluders.intersects_shape(&query));
    }

    #[test]
    fn static_occluder_index_intersects_large_occluders() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [0.0, 0.0, 0.0],
            [8192.0 * 40.0, 10.0, 10.0],
        ))]);

        assert!(occluders.intersects_volume(aabb(
            [8192.0 * 20.0, 0.0, 0.0],
            [8192.0 * 20.0 + 1.0, 1.0, 1.0],
        )));
    }

    #[test]
    fn static_occluder_index_reports_no_volume_intersection_for_duplicate_cell_hits() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [8190.0, 0.0, 0.0],
            [8200.0, 10.0, 10.0],
        ))]);

        assert!(!occluders.intersects_volume(aabb([8191.0, 0.0, 10.0], [8193.0, 10.0, 20.0],)));
    }

    #[test]
    fn static_occluder_index_ignores_distant_cells() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 10.0],
        ))]);

        assert!(
            occluders
                .candidates_for(aabb([8192.0, 0.0, 0.0], [8202.0, 10.0, 10.0]))
                .is_empty()
        );
    }

    #[test]
    fn static_occluder_index_does_not_index_exact_max_border_into_next_cell() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [0.0, 0.0, 0.0],
            [8192.0, 10.0, 10.0],
        ))]);

        assert!(
            occluders
                .candidates_for(aabb([8192.0, 0.0, 0.0], [8202.0, 10.0, 10.0]))
                .is_empty()
        );
    }

    #[test]
    fn static_occluder_index_keeps_large_occluders_visible() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [0.0, 0.0, 0.0],
            [8192.0 * 40.0, 10.0, 10.0],
        ))]);

        assert_eq!(
            occluders
                .candidates_for(aabb(
                    [8192.0 * 20.0, 0.0, 0.0],
                    [8192.0 * 20.0 + 1.0, 1.0, 1.0]
                ))
                .len(),
            1
        );
    }

    #[test]
    fn static_occluder_index_reports_3d_intersection() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 10.0],
        ))]);

        assert!(occluders.intersects_volume(aabb([5.0, 5.0, 5.0], [15.0, 15.0, 15.0],)));
        assert!(!occluders.intersects_volume(aabb([5.0, 5.0, 10.0], [15.0, 15.0, 20.0],)));
    }

    #[test]
    fn cell_span_treats_max_bounds_as_exclusive() {
        assert_eq!(
            cells_for_test(aabb([0.0, 0.0, 0.0], [8192.0, 8192.0, 1.0])),
            vec![(0, 0)]
        );
        assert_eq!(
            cells_for_test(aabb([-8192.0, -8192.0, 0.0], [0.0, 0.0, 1.0])),
            vec![(-1, -1)]
        );
    }

    #[test]
    fn cell_span_includes_crossed_cells_and_empty_zero_width_bounds() {
        assert_eq!(
            cells_for_test(aabb([8191.0, 0.0, 0.0], [8193.0, 1.0, 1.0])),
            vec![(0, 0), (1, 0)]
        );
        assert!(cells_for_test(aabb([0.0, 0.0, 0.0], [0.0, 1.0, 1.0])).is_empty());
    }

    fn aabb(min: [f32; 3], max: [f32; 3]) -> WorldAabb {
        WorldAabb { min, max }
    }

    fn mesh_aabb(min: [f32; 3], max: [f32; 3]) -> MeshAabb {
        MeshAabb { min, max }
    }

    fn static_occluder(bounds: WorldAabb) -> StaticOccluder {
        static_occluder_from_collider(RapierCollider::from_world_aabb(bounds))
    }

    fn static_occluder_with_id(id: &str, bounds: WorldAabb) -> StaticOccluder {
        let mut occluder = static_occluder(bounds);
        occluder.id = id.to_owned();
        occluder
    }

    fn static_occluder_from_collider(collider: RapierCollider) -> StaticOccluder {
        let bounds = collider.bounds();
        StaticOccluder {
            id: "rock".to_owned(),
            cell: [0, 0],
            reference_key: [1, 2],
            bounds,
            collider,
        }
    }

    fn cells_for_test(bounds: WorldAabb) -> Vec<(i32, i32)> {
        cell_span_for_bounds(bounds).into_iter().flatten().collect()
    }
}
