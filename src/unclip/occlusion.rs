use std::collections::{BTreeMap, BTreeSet};

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

#[derive(Default)]
pub(crate) struct StaticOccluderIndex {
    occluders: Vec<StaticOccluder>,
    cells: BTreeMap<CellCoord, Vec<usize>>,
    large_occluders: Vec<usize>,
}

impl StaticOccluderIndex {
    pub(crate) fn new(occluders: Vec<StaticOccluder>) -> Self {
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
        let mut indices = BTreeSet::new();
        if let Some(cells) = cell_span_for_bounds(bounds) {
            for cell in cells {
                if let Some(cell_indices) = self.cells.get(&cell) {
                    indices.extend(cell_indices.iter().copied());
                }
            }
            indices.extend(self.large_occluders.iter().copied());
        } else {
            indices.extend(0..self.occluders.len());
        }
        indices
            .into_iter()
            .filter_map(|index| self.occluders.get(index))
            .filter(|occluder| occluder.bounds.intersects_xy(bounds))
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn intersects_volume(&self, bounds: WorldAabb) -> bool {
        self.intersects_shape(&RapierCollider::from_world_aabb(bounds))
    }

    pub(crate) fn intersects_shape(&self, collider: &RapierCollider) -> bool {
        let bounds = collider.bounds();
        if let Some(cells) = cell_span_for_bounds(bounds) {
            for cell in cells {
                if let Some(cell_indices) = self.cells.get(&cell)
                    && cell_indices
                        .iter()
                        .any(|&index| self.intersects_shape_index(collider, index))
                {
                    return true;
                }
            }

            return self
                .large_occluders
                .iter()
                .any(|&index| self.intersects_shape_index(collider, index));
        }

        self.occluders
            .iter()
            .any(|occluder| shape_intersects(occluder, collider))
    }

    fn intersects_shape_index(&self, collider: &RapierCollider, index: usize) -> bool {
        self.occluders
            .get(index)
            .is_some_and(|occluder| shape_intersects(occluder, collider))
    }
}

fn shape_intersects(occluder: &StaticOccluder, collider: &RapierCollider) -> bool {
    occluder.bounds.intersection(collider.bounds()).is_some()
        && occluder.collider.intersects(collider)
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

#[derive(Clone, Copy)]
pub(crate) enum StaticBoundsAction<'a> {
    None,
    Delete {
        ratio: f32,
        occluder: &'a StaticOccluder,
    },
    Move {
        ratio: f32,
        occluder: &'a StaticOccluder,
        reason: StaticBoundsBlockReason,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StaticBoundsBlockReason {
    Volume,
    Clearance,
}

pub(crate) fn decide_static_bounds_action<'a>(
    grass_bounds: WorldAabb,
    grass_collider: &RapierCollider,
    clearance_collider: &RapierCollider,
    static_occluders: &'a StaticOccluderIndex,
) -> StaticBoundsAction<'a> {
    let candidates = static_occluders.candidates_for(grass_bounds);
    if let Some(occluder) = candidates
        .iter()
        .copied()
        .find(|occluder| occluder.collider.contains(grass_collider))
    {
        return StaticBoundsAction::Delete {
            ratio: 1.0,
            occluder,
        };
    }

    let clearance_candidates = static_occluders.candidates_for(clearance_collider.bounds());
    if let Some(occluder) = clearance_candidates
        .iter()
        .copied()
        .find(|occluder| shape_intersects(occluder, clearance_collider))
    {
        return StaticBoundsAction::Move {
            ratio: 0.0,
            occluder,
            reason: StaticBoundsBlockReason::Clearance,
        };
    }

    let candidates = candidates
        .into_iter()
        .filter(|occluder| shape_intersects(occluder, grass_collider))
        .collect::<Vec<_>>();
    let candidate_bounds = candidates
        .iter()
        .map(|occluder| occluder.bounds)
        .collect::<Vec<_>>();
    let ratio = static_bounds_occlusion_ratio(grass_bounds, &candidate_bounds);
    if ratio <= f32::EPSILON {
        return StaticBoundsAction::None;
    }
    let Some(occluder) = primary_occluder(grass_bounds, &candidates) else {
        return StaticBoundsAction::None;
    };
    StaticBoundsAction::Move {
        ratio,
        occluder,
        reason: StaticBoundsBlockReason::Volume,
    }
}

fn primary_occluder<'a>(
    grass_bounds: WorldAabb,
    occluders: &[&'a StaticOccluder],
) -> Option<&'a StaticOccluder> {
    occluders
        .iter()
        .copied()
        .filter_map(|occluder| {
            grass_bounds
                .intersection(occluder.bounds)
                .map(|intersection| (occluder, intersection.volume()))
        })
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(occluder, _)| occluder)
}

pub(crate) fn static_bounds_occlusion_ratio(
    grass_bounds: WorldAabb,
    occluders: &[WorldAabb],
) -> f32 {
    let grass_volume = grass_bounds.volume();
    if grass_volume <= f32::EPSILON {
        return 0.0;
    }

    if let [occluder] = occluders {
        return grass_bounds
            .intersection(*occluder)
            .map_or(0.0, |intersection| {
                (intersection.volume() / grass_volume).min(1.0)
            });
    }

    let intersections = occluders
        .iter()
        .filter_map(|occluder| grass_bounds.intersection(*occluder))
        .collect::<Vec<_>>();
    let occluded = union_volume(&intersections);

    (occluded / grass_volume).min(1.0)
}

fn union_volume(bounds: &[WorldAabb]) -> f32 {
    if bounds.is_empty() {
        return 0.0;
    }

    let mut xs = bounds
        .iter()
        .flat_map(|bounds| [bounds.min[0], bounds.max[0]])
        .collect::<Vec<_>>();
    let mut ys = bounds
        .iter()
        .flat_map(|bounds| [bounds.min[1], bounds.max[1]])
        .collect::<Vec<_>>();
    let mut zs = bounds
        .iter()
        .flat_map(|bounds| [bounds.min[2], bounds.max[2]])
        .collect::<Vec<_>>();
    sort_dedup_f32(&mut xs);
    sort_dedup_f32(&mut ys);
    sort_dedup_f32(&mut zs);

    let mut volume = 0.0;
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            for z in zs.windows(2) {
                let center = [
                    (x[0] + x[1]) * 0.5,
                    (y[0] + y[1]) * 0.5,
                    (z[0] + z[1]) * 0.5,
                ];
                if bounds.iter().any(|bounds| bounds.contains_point(center)) {
                    volume += (x[1] - x[0]) * (y[1] - y[0]) * (z[1] - z[0]);
                }
            }
        }
    }

    volume
}

fn sort_dedup_f32(values: &mut Vec<f32>) {
    values.sort_by(f32::total_cmp);
    values.dedup_by(|left, right| (*left - *right).abs() <= f32::EPSILON);
}

#[cfg(test)]
mod tests {
    use super::{
        StaticBoundsAction, StaticBoundsBlockReason, StaticOccluder, StaticOccluderIndex,
        cell_span_for_bounds, decide_static_bounds_action, static_bounds_occlusion_ratio,
    };
    use crate::unclip::{
        mesh::{MeshAabb, WorldAabb},
        physics::RapierCollider,
    };

    #[test]
    fn static_bounds_occlusion_ratio_counts_intersection_volume() {
        let grass = aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let occluder = aabb([0.0, 0.0, 0.0], [5.0, 10.0, 10.0]);

        assert_close(static_bounds_occlusion_ratio(grass, &[occluder]), 0.5);
    }

    #[test]
    fn static_bounds_occlusion_ratio_uses_union_volume() {
        let grass = aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let left = aabb([0.0, 0.0, 0.0], [7.0, 10.0, 10.0]);
        let right = aabb([3.0, 0.0, 0.0], [10.0, 10.0, 10.0]);

        assert_close(static_bounds_occlusion_ratio(grass, &[left, right]), 1.0);
    }

    #[test]
    fn static_bounds_action_ignores_clear_ref() {
        let grass = aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [20.0, 0.0, 0.0],
            [30.0, 10.0, 10.0],
        ))]);

        assert!(matches!(
            decide_static_bounds_action(
                grass,
                &RapierCollider::from_world_aabb(grass),
                &RapierCollider::from_world_aabb(grass),
                &occluders
            ),
            StaticBoundsAction::None
        ));
    }

    #[test]
    fn static_bounds_action_ignores_broad_aabb_overlap_without_shape_collision() {
        let grass_collider =
            RapierCollider::from_world_aabb(aabb([-0.25, 3.25, -0.25], [0.25, 3.75, 0.25]));
        let grass = grass_collider.bounds();
        let occluders = StaticOccluderIndex::new(vec![static_occluder_from_collider(
            RapierCollider::from_mesh_bounds(
                mesh_aabb([-5.0, -0.5, -0.5], [5.0, 0.5, 0.5]),
                [0.0; 3],
                [0.0, 0.0, std::f32::consts::FRAC_PI_4],
                None,
            ),
        )]);

        assert!(occluders.candidates_for(grass).len() == 1);
        assert!(matches!(
            decide_static_bounds_action(grass, &grass_collider, &grass_collider, &occluders),
            StaticBoundsAction::None
        ));
    }

    #[test]
    fn static_bounds_action_detects_actual_shape_collision() {
        let grass = RapierCollider::from_world_aabb(aabb([0.0; 3], [2.0; 3]));
        let occluders =
            StaticOccluderIndex::new(vec![static_occluder(aabb([1.0, 1.0, 1.0], [3.0; 3]))]);

        assert!(matches!(
            decide_static_bounds_action(
                grass.bounds(),
                &grass,
                &RapierCollider::from_world_aabb(aabb([20.0; 3], [21.0; 3])),
                &occluders
            ),
            StaticBoundsAction::Move {
                reason: StaticBoundsBlockReason::Volume,
                ..
            }
        ));
    }

    #[test]
    fn static_bounds_action_deletes_fully_contained_ref() {
        let grass = aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [-1.0, -1.0, -1.0],
            [11.0, 11.0, 11.0],
        ))]);

        match decide_static_bounds_action(
            grass,
            &RapierCollider::from_world_aabb(grass),
            &RapierCollider::from_world_aabb(aabb([20.0; 3], [21.0; 3])),
            &occluders,
        ) {
            StaticBoundsAction::Delete { ratio, occluder } => {
                assert_close(ratio, 1.0);
                assert_eq!(occluder.id, "rock");
            }
            StaticBoundsAction::None | StaticBoundsAction::Move { .. } => panic!("expected delete"),
        }
    }

    #[test]
    fn static_bounds_action_moves_partially_occluded_ref() {
        let grass = aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [5.0, 0.0, 0.0],
            [10.0, 10.0, 10.0],
        ))]);

        match decide_static_bounds_action(
            grass,
            &RapierCollider::from_world_aabb(grass),
            &RapierCollider::from_world_aabb(aabb([20.0; 3], [21.0; 3])),
            &occluders,
        ) {
            StaticBoundsAction::Move {
                ratio,
                occluder,
                reason,
            } => {
                assert_close(ratio, 0.5);
                assert_eq!(occluder.id, "rock");
                assert_eq!(reason, StaticBoundsBlockReason::Volume);
            }
            StaticBoundsAction::None | StaticBoundsAction::Delete { .. } => panic!("expected move"),
        }
    }

    #[test]
    fn static_bounds_action_uses_clearance_for_thin_board_with_low_volume_ratio() {
        let grass_bounds = mesh_aabb([-50.0, -50.0, 0.0], [50.0, 50.0, 100.0]);
        let grass_collider =
            RapierCollider::from_mesh_bounds(grass_bounds, [0.0; 3], [0.0; 3], None);
        let clearance = RapierCollider::placement_clearance_from_mesh_bounds(
            grass_bounds,
            [0.0; 3],
            [0.0; 3],
            None,
        );
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [-2.0, -60.0, 40.0],
            [2.0, 60.0, 42.0],
        ))]);

        match decide_static_bounds_action(
            grass_collider.bounds(),
            &grass_collider,
            &clearance,
            &occluders,
        ) {
            StaticBoundsAction::Move { ratio, reason, .. } => {
                assert_close(ratio, 0.0);
                assert_eq!(reason, StaticBoundsBlockReason::Clearance);
            }
            StaticBoundsAction::None | StaticBoundsAction::Delete { .. } => {
                panic!("expected clearance move")
            }
        }
    }

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

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
