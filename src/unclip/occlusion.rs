use std::collections::{BTreeMap, BTreeSet};

use super::{cells::CellCoord, mesh::WorldAabb};

const CELL_SIZE: f32 = 8192.0;

#[derive(Clone)]
pub(crate) struct StaticOccluder {
    pub(crate) id: String,
    pub(crate) cell: [i32; 2],
    pub(crate) reference_key: [u32; 2],
    pub(crate) bounds: WorldAabb,
}

#[derive(Default)]
pub(crate) struct StaticOccluderIndex {
    occluders: Vec<StaticOccluder>,
    cells: BTreeMap<CellCoord, Vec<usize>>,
}

impl StaticOccluderIndex {
    pub(crate) fn new(occluders: Vec<StaticOccluder>) -> Self {
        let mut cells = BTreeMap::<CellCoord, Vec<usize>>::new();
        for (index, occluder) in occluders.iter().enumerate() {
            for cell in cells_for_bounds(occluder.bounds) {
                cells.entry(cell).or_default().push(index);
            }
        }
        Self { occluders, cells }
    }

    pub(crate) fn candidates_for(&self, bounds: WorldAabb) -> Vec<&StaticOccluder> {
        let mut indices = BTreeSet::new();
        for cell in cells_for_bounds(bounds) {
            if let Some(cell_indices) = self.cells.get(&cell) {
                indices.extend(cell_indices.iter().copied());
            }
        }
        indices
            .into_iter()
            .filter_map(|index| self.occluders.get(index))
            .filter(|occluder| occluder.bounds.intersects_xy(bounds))
            .collect()
    }

    pub(crate) fn intersects_volume(&self, bounds: WorldAabb) -> bool {
        self.candidates_for(bounds)
            .into_iter()
            .any(|occluder| bounds.intersection(occluder.bounds).is_some())
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn cells_for_bounds(bounds: WorldAabb) -> Vec<CellCoord> {
    let min_x = cell_coord(bounds.min[0]);
    let min_y = cell_coord(bounds.min[1]);
    let max_x = cell_coord(bounds.max[0] - f32::EPSILON);
    let max_y = cell_coord(bounds.max[1] - f32::EPSILON);
    let mut cells = Vec::new();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            cells.push((x, y));
        }
    }
    cells
}

#[allow(clippy::cast_possible_truncation)]
fn cell_coord(position: f32) -> i32 {
    (position / CELL_SIZE).floor() as i32
}

pub(crate) enum StaticBoundsAction<'a> {
    None,
    Delete {
        ratio: f32,
        occluder: &'a StaticOccluder,
    },
    Move {
        ratio: f32,
        occluder: &'a StaticOccluder,
    },
}

pub(crate) fn decide_static_bounds_action(
    grass_bounds: WorldAabb,
    static_occluders: &StaticOccluderIndex,
) -> StaticBoundsAction<'_> {
    let candidates = static_occluders.candidates_for(grass_bounds);
    if let Some(occluder) = candidates
        .iter()
        .copied()
        .find(|occluder| occluder.bounds.contains(grass_bounds))
    {
        return StaticBoundsAction::Delete {
            ratio: 1.0,
            occluder,
        };
    }

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
    StaticBoundsAction::Move { ratio, occluder }
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

pub(crate) fn translate_bounds_xy(bounds: WorldAabb, x: f32, y: f32) -> WorldAabb {
    WorldAabb {
        min: [bounds.min[0] + x, bounds.min[1] + y, bounds.min[2]],
        max: [bounds.max[0] + x, bounds.max[1] + y, bounds.max[2]],
    }
}

#[cfg(test)]
mod tests {
    use super::{
        StaticBoundsAction, StaticOccluder, StaticOccluderIndex, decide_static_bounds_action,
        static_bounds_occlusion_ratio,
    };
    use crate::unclip::mesh::WorldAabb;

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
            decide_static_bounds_action(grass, &occluders),
            StaticBoundsAction::None
        ));
    }

    #[test]
    fn static_bounds_action_deletes_fully_contained_ref() {
        let grass = aabb([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [-1.0, -1.0, -1.0],
            [11.0, 11.0, 11.0],
        ))]);

        match decide_static_bounds_action(grass, &occluders) {
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

        match decide_static_bounds_action(grass, &occluders) {
            StaticBoundsAction::Move { ratio, occluder } => {
                assert_close(ratio, 0.5);
                assert_eq!(occluder.id, "rock");
            }
            StaticBoundsAction::None | StaticBoundsAction::Delete { .. } => panic!("expected move"),
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
    fn static_occluder_index_reports_3d_intersection() {
        let occluders = StaticOccluderIndex::new(vec![static_occluder(aabb(
            [0.0, 0.0, 0.0],
            [10.0, 10.0, 10.0],
        ))]);

        assert!(occluders.intersects_volume(aabb([5.0, 5.0, 5.0], [15.0, 15.0, 15.0],)));
        assert!(!occluders.intersects_volume(aabb([5.0, 5.0, 10.0], [15.0, 15.0, 20.0],)));
    }

    fn aabb(min: [f32; 3], max: [f32; 3]) -> WorldAabb {
        WorldAabb { min, max }
    }

    fn static_occluder(bounds: WorldAabb) -> StaticOccluder {
        StaticOccluder {
            id: "rock".to_owned(),
            cell: [0, 0],
            reference_key: [1, 2],
            bounds,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
