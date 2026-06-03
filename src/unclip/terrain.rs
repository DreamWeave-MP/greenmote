// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeSet;

use glam::Vec3;
use rustc_hash::FxHashMap;
use tes3::esp::{Landscape, LandscapeFlags};

use super::cells::CellCoord;

const CELL_SIZE: f32 = 8192.0;
const LAND_VERTEX_SPACING: f32 = 128.0;
const LAND_VERTEX_MAX: usize = 64;

pub struct TerrainIndex {
    lands: TerrainLandMap,
}

type TerrainLandMap = FxHashMap<CellCoord, Box<[[f32; 65]; 65]>>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerrainSample {
    pub(crate) height: f32,
    pub(crate) normal: [f32; 3],
    pub(crate) angle: TerrainAngle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerrainAngle {
    pub(crate) xrot: f32,
    pub(crate) yrot: f32,
}

impl TerrainIndex {
    #[must_use]
    pub fn from_landscapes<'a>(landscapes: impl IntoIterator<Item = &'a Landscape>) -> Self {
        let mut lands = TerrainLandMap::default();

        for landscape in landscapes {
            if landscape.flags.contains(tes3::esp::ObjectFlags::DELETED) {
                lands.remove(&landscape.grid);
            } else if landscape
                .landscape_flags
                .intersects(LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS)
            {
                lands.insert(landscape.grid, landscape.decode_vertex_heights());
            }
        }

        Self { lands }
    }

    #[must_use]
    pub fn from_landscapes_in_cells<'a>(
        landscapes: impl IntoIterator<Item = &'a Landscape>,
        cells: &BTreeSet<CellCoord>,
    ) -> Self {
        Self::from_landscapes(
            landscapes
                .into_iter()
                .filter(|landscape| cells.contains(&landscape.grid)),
        )
    }

    #[cfg(test)]
    pub(crate) fn from_decoded_heights(cell: CellCoord, heights: Box<[[f32; 65]; 65]>) -> Self {
        Self {
            lands: TerrainLandMap::from_iter([(cell, heights)]),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.lands.len()
    }

    #[must_use]
    pub fn height_at(&self, world_x: f32, world_y: f32) -> Option<f32> {
        let cell = world_cell(world_x, world_y);
        let heights = self.lands.get(&cell)?;
        Some(sample_terrain_height(
            heights,
            local_cell_coord(world_x, cell.0),
            local_cell_coord(world_y, cell.1),
        ))
    }

    #[must_use]
    pub(crate) fn sample_at(&self, world_x: f32, world_y: f32) -> Option<TerrainSample> {
        let cell = world_cell(world_x, world_y);
        let heights = self.lands.get(&cell)?;
        let quad = terrain_quad(
            heights,
            local_cell_coord(world_x, cell.0),
            local_cell_coord(world_y, cell.1),
        );
        let (slope_x, slope_y) = terrain_quad_slope(quad);
        Some(TerrainSample {
            height: terrain_quad_height(quad),
            normal: terrain_normal(slope_x, slope_y),
            angle: self.generator_angle_at(world_x, world_y, cell, heights),
        })
    }

    #[must_use]
    pub fn has_cell(&self, cell: CellCoord) -> bool {
        self.lands.contains_key(&cell)
    }

    fn generator_angle_at(
        &self,
        world_x: f32,
        world_y: f32,
        fallback_cell: CellCoord,
        fallback_heights: &[[f32; 65]; 65],
    ) -> TerrainAngle {
        if let Some(angle) =
            same_cell_generator_angle(world_x, world_y, fallback_cell, fallback_heights)
        {
            return angle;
        }

        generator_angle_from_vertices(world_x, world_y, |vertex_x, vertex_y| {
            self.height_at_global_vertex(vertex_x, vertex_y, fallback_cell)
        })
    }

    #[allow(clippy::cast_sign_loss)]
    fn height_at_global_vertex(
        &self,
        vertex_x: i32,
        vertex_y: i32,
        fallback_cell: CellCoord,
    ) -> f32 {
        let cell = (vertex_x.div_euclid(64), vertex_y.div_euclid(64));
        if let Some(heights) = self.lands.get(&cell) {
            return heights[vertex_y.rem_euclid(64) as usize][vertex_x.rem_euclid(64) as usize];
        }

        // mw-groundcover-generator samples the four stencil vertices across LAND boundaries.
        // If a neighboring LAND is unavailable in this index, clamp to the sampled cell's edge
        // rather than inventing a default height that can create extreme boundary tilts.
        let heights = &self.lands[&fallback_cell];
        let min_x = fallback_cell.0 * 64;
        let min_y = fallback_cell.1 * 64;
        let local_x = (vertex_x.clamp(min_x, min_x + 64) - min_x) as usize;
        let local_y = (vertex_y.clamp(min_y, min_y + 64) - min_y) as usize;
        heights[local_y][local_x]
    }
}

#[allow(clippy::cast_possible_truncation)]
fn world_cell(coord: f32, other: f32) -> CellCoord {
    (
        (coord / CELL_SIZE).floor() as i32,
        (other / CELL_SIZE).floor() as i32,
    )
}

#[allow(clippy::cast_precision_loss)]
fn local_cell_coord(world_coord: f32, cell_coord: i32) -> f32 {
    (world_coord - cell_coord as f32 * CELL_SIZE).clamp(0.0, CELL_SIZE)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[cfg(test)]
fn sample_height(heights: &[[f32; 65]; 65], local_x: f32, local_y: f32) -> f32 {
    sample_terrain_height(heights, local_x, local_y)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn terrain_quad(heights: &[[f32; 65]; 65], local_x: f32, local_y: f32) -> TerrainQuad {
    let grid_x = (local_x / LAND_VERTEX_SPACING).clamp(0.0, LAND_VERTEX_MAX as f32);
    let grid_y = (local_y / LAND_VERTEX_SPACING).clamp(0.0, LAND_VERTEX_MAX as f32);
    let x0 = grid_x.floor().min((LAND_VERTEX_MAX - 1) as f32) as usize;
    let y0 = grid_y.floor().min((LAND_VERTEX_MAX - 1) as f32) as usize;
    let x1 = (x0 + 1).min(LAND_VERTEX_MAX);
    let y1 = (y0 + 1).min(LAND_VERTEX_MAX);
    let tx = grid_x - x0 as f32;
    let ty = grid_y - y0 as f32;
    let h00 = heights[y0][x0];
    let h10 = heights[y0][x1];
    let h01 = heights[y1][x0];
    let h11 = heights[y1][x1];

    TerrainQuad {
        x0,
        y0,
        tx,
        ty,
        h00,
        h10,
        h01,
        h11,
    }
}

#[derive(Clone, Copy)]
struct TerrainQuad {
    x0: usize,
    y0: usize,
    tx: f32,
    ty: f32,
    h00: f32,
    h10: f32,
    h01: f32,
    h11: f32,
}

fn sample_terrain_height(heights: &[[f32; 65]; 65], local_x: f32, local_y: f32) -> f32 {
    terrain_quad_height(terrain_quad(heights, local_x, local_y))
}

fn terrain_quad_height(quad: TerrainQuad) -> f32 {
    let TerrainQuad {
        x0,
        y0,
        tx,
        ty,
        h00,
        h10,
        h01,
        h11,
    } = quad;

    if ((x0 ^ y0) & 1) == 0 {
        if tx <= ty {
            h00 + (h01 - h00) * (ty - tx) + (h11 - h00) * tx
        } else {
            h00 + (h11 - h00) * ty + (h10 - h00) * (tx - ty)
        }
    } else if tx + ty <= 1.0 {
        interpolate_triangle(h00, h01, h10, tx, ty)
    } else {
        interpolate_triangle(h11, h10, h01, 1.0 - tx, 1.0 - ty)
    }
}

fn terrain_quad_slope(quad: TerrainQuad) -> (f32, f32) {
    let TerrainQuad {
        x0,
        y0,
        tx,
        ty,
        h00,
        h10,
        h01,
        h11,
    } = quad;

    if ((x0 ^ y0) & 1) == 0 {
        if tx <= ty {
            (
                (h11 - h01) / LAND_VERTEX_SPACING,
                (h01 - h00) / LAND_VERTEX_SPACING,
            )
        } else {
            (
                (h10 - h00) / LAND_VERTEX_SPACING,
                (h11 - h10) / LAND_VERTEX_SPACING,
            )
        }
    } else if tx + ty <= 1.0 {
        (
            (h10 - h00) / LAND_VERTEX_SPACING,
            (h01 - h00) / LAND_VERTEX_SPACING,
        )
    } else {
        (
            (h11 - h01) / LAND_VERTEX_SPACING,
            (h11 - h10) / LAND_VERTEX_SPACING,
        )
    }
}

#[cfg(test)]
fn sample_terrain(heights: &[[f32; 65]; 65], local_x: f32, local_y: f32) -> TerrainSample {
    let quad = terrain_quad(heights, local_x, local_y);
    let height = terrain_quad_height(quad);
    let (slope_x, slope_y) = terrain_quad_slope(quad);
    TerrainSample {
        height,
        normal: terrain_normal(slope_x, slope_y),
        angle: generator_angle_from_local_heights(heights, local_x, local_y),
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
#[cfg(test)]
fn generator_angle_from_local_heights(
    heights: &[[f32; 65]; 65],
    local_x: f32,
    local_y: f32,
) -> TerrainAngle {
    let vertex_x = (local_x / LAND_VERTEX_SPACING)
        .ceil()
        .clamp(0.0, LAND_VERTEX_MAX as f32) as usize;
    let vertex_y = (local_y / LAND_VERTEX_SPACING)
        .ceil()
        .clamp(0.0, LAND_VERTEX_MAX as f32) as usize;
    let prev_x = vertex_x.saturating_sub(1);
    let prev_y = vertex_y.saturating_sub(1);
    generator_angle_from_stencil(
        heights[vertex_y][vertex_x],
        heights[prev_y][vertex_x],
        heights[vertex_y][prev_x],
        heights[prev_y][prev_x],
    )
}

#[allow(clippy::cast_possible_truncation)]
fn generator_angle_from_vertices(
    world_x: f32,
    world_y: f32,
    mut height_at_vertex: impl FnMut(i32, i32) -> f32,
) -> TerrainAngle {
    let vertex_x = (world_x / LAND_VERTEX_SPACING).ceil() as i32;
    let vertex_y = (world_y / LAND_VERTEX_SPACING).ceil() as i32;
    generator_angle_from_stencil(
        height_at_vertex(vertex_x, vertex_y),
        height_at_vertex(vertex_x, vertex_y - 1),
        height_at_vertex(vertex_x - 1, vertex_y),
        height_at_vertex(vertex_x - 1, vertex_y - 1),
    )
}

fn generator_angle_from_stencil(
    h_xy: f32,
    h_x_prev_y: f32,
    h_prev_x_y: f32,
    h_prev: f32,
) -> TerrainAngle {
    let xrot_a = ((h_xy - h_x_prev_y) / LAND_VERTEX_SPACING).atan();
    let xrot_b = ((h_prev_x_y - h_prev) / LAND_VERTEX_SPACING).atan();
    let yrot_a = ((h_xy - h_prev_x_y) / LAND_VERTEX_SPACING).atan();
    let yrot_b = ((h_x_prev_y - h_prev) / LAND_VERTEX_SPACING).atan();

    TerrainAngle {
        xrot: -xrot_a.midpoint(xrot_b),
        yrot: yrot_a.midpoint(yrot_b),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn same_cell_generator_angle(
    world_x: f32,
    world_y: f32,
    cell: CellCoord,
    heights: &[[f32; 65]; 65],
) -> Option<TerrainAngle> {
    let vertex_x = (world_x / LAND_VERTEX_SPACING).ceil() as i32;
    let vertex_y = (world_y / LAND_VERTEX_SPACING).ceil() as i32;
    let min_x = cell.0 * 64;
    let min_y = cell.1 * 64;
    if vertex_x <= min_x || vertex_x >= min_x + 64 || vertex_y <= min_y || vertex_y >= min_y + 64 {
        return None;
    }

    let local_x = usize::try_from(vertex_x - min_x).ok()?;
    let local_y = usize::try_from(vertex_y - min_y).ok()?;
    let prev_x = local_x.checked_sub(1)?;
    let prev_y = local_y.checked_sub(1)?;
    Some(generator_angle_from_stencil(
        heights[local_y][local_x],
        heights[prev_y][local_x],
        heights[local_y][prev_x],
        heights[prev_y][prev_x],
    ))
}

fn terrain_normal(slope_x: f32, slope_y: f32) -> [f32; 3] {
    Vec3::new(-slope_x, -slope_y, 1.0).normalize().to_array()
}

fn interpolate_triangle(
    origin: f32,
    y_axis: f32,
    x_axis: f32,
    x_weight: f32,
    y_weight: f32,
) -> f32 {
    origin + (x_axis - origin) * x_weight + (y_axis - origin) * y_weight
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::cast_precision_loss)]
    fn patterned_heights(offset: f32) -> Box<[[f32; 65]; 65]> {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        for (y, row) in heights.iter_mut().enumerate() {
            for (x, height) in row.iter_mut().enumerate() {
                *height = offset + x as f32 * 3.25 - y as f32 * 1.5 + ((x ^ y) & 3) as f32;
            }
        }
        heights
    }

    #[test]
    fn samples_even_quad_triangle_height_inside_cell() {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        heights[0][0] = 0.0;
        heights[0][1] = 10.0;
        heights[1][0] = 20.0;
        heights[1][1] = 100.0;

        assert!((sample_height(&heights, 32.0, 96.0) - 35.0).abs() < f32::EPSILON);
        assert!((sample_height(&heights, 96.0, 32.0) - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn samples_odd_quad_triangle_height_inside_cell() {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        heights[0][1] = 0.0;
        heights[0][2] = 10.0;
        heights[1][1] = 20.0;
        heights[1][2] = 100.0;

        assert!((sample_height(&heights, 192.0, 64.0) - 15.0).abs() < f32::EPSILON);
        assert!((sample_height(&heights, 224.0, 96.0) - 57.5).abs() < f32::EPSILON);
    }

    #[test]
    fn samples_triangle_normal_from_height_slope() {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        heights[0][1] = 128.0;
        heights[1][1] = 128.0;

        let sample = sample_terrain(&heights, 96.0, 32.0);

        assert!((sample.normal[0] + std::f32::consts::FRAC_1_SQRT_2).abs() < 0.000_01);
        assert!(sample.normal[1].abs() < 0.000_01);
        assert!((sample.normal[2] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.000_01);
    }

    #[test]
    fn samples_generator_style_angle_from_height_stencil() {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        heights[1][2] = 128.0;
        heights[0][2] = 0.0;
        heights[1][1] = 64.0;
        heights[0][1] = 0.0;

        let sample = sample_terrain(&heights, 256.0, 128.0);

        let expected_x = -((1.0_f32).atan() + (0.5_f32).atan()) / 2.0;
        let expected_y = (0.5_f32).atan() / 2.0;
        assert!((sample.angle.xrot - expected_x).abs() < 0.000_01);
        assert!((sample.angle.yrot - expected_y).abs() < 0.000_01);
    }

    #[test]
    fn sample_at_same_cell_generator_angle_matches_global_vertex_path() {
        let terrain = TerrainIndex::from_decoded_heights((0, 0), patterned_heights(0.0));
        let sample = terrain.sample_at(320.0, 448.0).unwrap();
        let expected = generator_angle_from_vertices(320.0, 448.0, |vertex_x, vertex_y| {
            terrain.height_at_global_vertex(vertex_x, vertex_y, (0, 0))
        });

        assert_eq!(sample.angle, expected);
    }

    #[test]
    fn height_at_matches_sample_height_across_cells() {
        let terrain = TerrainIndex {
            lands: TerrainLandMap::from_iter([
                ((0, 0), patterned_heights(0.0)),
                ((-1, 0), patterned_heights(1_000.0)),
                ((0, -1), patterned_heights(2_000.0)),
            ]),
        };

        for (world_x, world_y) in [
            (0.0, 0.0),
            (32.0, 96.0),
            (8_191.5, 8_191.5),
            (-1.0, 64.0),
            (-8_192.0, 128.0),
            (64.0, -64.0),
        ] {
            assert_eq!(
                terrain.height_at(world_x, world_y),
                terrain
                    .sample_at(world_x, world_y)
                    .map(|sample| sample.height)
            );
        }

        assert_eq!(terrain.height_at(8_192.0, 0.0), None);
    }

    #[test]
    fn positive_cell_boundary_samples_neighbor_origin_vertex() {
        let mut left: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        let mut right: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        left[0][64] = 640.0;
        right[0][0] = 10.0;
        let terrain = TerrainIndex {
            lands: TerrainLandMap::from_iter([((0, 0), left), ((1, 0), right)]),
        };

        assert_eq!(terrain.height_at(8_192.0, 0.0), Some(10.0));
        assert_eq!(
            terrain.height_at(8_192.0, 0.0),
            terrain.sample_at(8_192.0, 0.0).map(|sample| sample.height)
        );
    }

    #[test]
    fn generator_style_angle_samples_present_boundary_neighbor() {
        let mut left: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        let mut right: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        left[1][63] = 64.0;
        left[0][63] = 32.0;
        right[1][0] = 160.0;
        right[0][0] = 96.0;
        let terrain = TerrainIndex {
            lands: TerrainLandMap::from_iter([((0, 0), left), ((1, 0), right)]),
        };

        let sample = terrain.sample_at(8_192.0, 128.0).unwrap();

        assert_eq!(
            sample.angle,
            generator_angle_from_stencil(160.0, 96.0, 64.0, 32.0)
        );
    }

    #[test]
    fn generator_style_angle_clamps_missing_boundary_neighbor() {
        let mut heights: Box<[[f32; 65]; 65]> = vec![[0.0; 65]; 65]
            .into_boxed_slice()
            .try_into()
            .unwrap_or_else(|_| panic!("terrain test grid should have 65 rows"));
        heights[0][0] = 64.0;
        let terrain = TerrainIndex {
            lands: TerrainLandMap::from_iter([((0, 0), heights)]),
        };

        let sample = terrain.sample_at(0.0, 0.0).unwrap();

        assert_eq!(
            sample.angle,
            TerrainAngle {
                xrot: 0.0,
                yrot: 0.0
            }
        );
    }

    #[test]
    fn texture_only_landscape_does_not_replace_height_data() {
        let mut base = Landscape {
            landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
            ..Landscape::default()
        };
        base.vertex_heights.offset = 5.0;

        let texture_only = Landscape {
            landscape_flags: LandscapeFlags::USES_TEXTURES,
            ..Landscape::default()
        };

        let terrain = TerrainIndex::from_landscapes([&base, &texture_only]);

        assert!((terrain.height_at(0.0, 0.0).unwrap_or_default() - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn terrain_index_decodes_only_requested_cells() {
        let mut included = Landscape {
            grid: (0, 0),
            landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
            ..Landscape::default()
        };
        included.vertex_heights.offset = 5.0;
        let excluded = Landscape {
            grid: (1, 0),
            landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
            ..Landscape::default()
        };

        let terrain = TerrainIndex::from_landscapes_in_cells(
            [&included, &excluded],
            &BTreeSet::from([(0, 0)]),
        );

        assert!(terrain.has_cell((0, 0)));
        assert!(!terrain.has_cell((1, 0)));
    }
}
