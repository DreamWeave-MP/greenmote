use std::collections::{BTreeSet, HashMap};

use glam::Vec3;
use tes3::esp::{Landscape, LandscapeFlags};

use super::cells::CellCoord;

const CELL_SIZE: f32 = 8192.0;
const LAND_VERTEX_SPACING: f32 = 128.0;
const LAND_VERTEX_MAX: usize = 64;

pub struct TerrainIndex {
    lands: HashMap<CellCoord, Box<[[f32; 65]; 65]>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerrainSample {
    pub(crate) height: f32,
    pub(crate) normal: [f32; 3],
}

impl TerrainIndex {
    #[must_use]
    pub fn from_landscapes<'a>(landscapes: impl IntoIterator<Item = &'a Landscape>) -> Self {
        let mut lands = HashMap::new();

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

    #[must_use]
    pub fn len(&self) -> usize {
        self.lands.len()
    }

    #[must_use]
    pub fn height_at(&self, world_x: f32, world_y: f32) -> Option<f32> {
        self.sample_at(world_x, world_y).map(|sample| sample.height)
    }

    #[must_use]
    pub(crate) fn sample_at(&self, world_x: f32, world_y: f32) -> Option<TerrainSample> {
        let cell = world_cell(world_x, world_y);
        let heights = self.lands.get(&cell)?;
        Some(sample_terrain(
            heights,
            local_cell_coord(world_x, cell.0),
            local_cell_coord(world_y, cell.1),
        ))
    }

    #[must_use]
    pub fn has_cell(&self, cell: CellCoord) -> bool {
        self.lands.contains_key(&cell)
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
    sample_terrain(heights, local_x, local_y).height
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
fn sample_terrain(heights: &[[f32; 65]; 65], local_x: f32, local_y: f32) -> TerrainSample {
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

    let (height, slope_x, slope_y) = if ((x0 ^ y0) & 1) == 0 {
        if tx <= ty {
            (
                h00 + (h01 - h00) * (ty - tx) + (h11 - h00) * tx,
                (h11 - h01) / LAND_VERTEX_SPACING,
                (h01 - h00) / LAND_VERTEX_SPACING,
            )
        } else {
            (
                h00 + (h11 - h00) * ty + (h10 - h00) * (tx - ty),
                (h10 - h00) / LAND_VERTEX_SPACING,
                (h11 - h10) / LAND_VERTEX_SPACING,
            )
        }
    } else if tx + ty <= 1.0 {
        (
            interpolate_triangle(h00, h01, h10, tx, ty),
            (h10 - h00) / LAND_VERTEX_SPACING,
            (h01 - h00) / LAND_VERTEX_SPACING,
        )
    } else {
        (
            interpolate_triangle(h11, h10, h01, 1.0 - tx, 1.0 - ty),
            (h11 - h01) / LAND_VERTEX_SPACING,
            (h11 - h10) / LAND_VERTEX_SPACING,
        )
    };
    TerrainSample {
        height,
        normal: terrain_normal(slope_x, slope_y),
    }
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
