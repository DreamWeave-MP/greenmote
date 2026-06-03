// SPDX-License-Identifier: GPL-3.0-only

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use clap::Parser;
use greenmote::{Cli, Command};
use serde_json::Value;
use tes3::{
    esp::{
        Cell, CellData, Header, Landscape, LandscapeFlags, Plugin, Reference, Static, TES3Object,
    },
    nif::{
        NiAVObject, NiGeometry, NiGeometryData, NiLink, NiObjectNET, NiStream, NiTriBasedGeom,
        NiTriBasedGeomData, NiTriShape, NiTriShapeData, NiType,
    },
};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

type NifVec3 = rapier3d::math::Vec3;

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    grid_side: i32,
    refs_per_cell: usize,
}

const MEDIUM: Profile = Profile {
    name: "medium",
    grid_side: 48,
    refs_per_cell: 32,
};

const LARGE: Profile = Profile {
    name: "large",
    grid_side: 96,
    refs_per_cell: 48,
};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-synthetic-unclip-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
#[ignore = "synthetic medium unclip benchmark/torture test"]
fn synthetic_unclip_medium_torture() {
    run_synthetic_unclip_torture(MEDIUM);
}

#[test]
#[ignore = "synthetic large unclip benchmark/torture test"]
fn synthetic_unclip_large_torture() {
    run_synthetic_unclip_torture(LARGE);
}

fn run_synthetic_unclip_torture(profile: Profile) {
    let config_dir = TempDir::new("config");
    let data_dir = TempDir::new("data");
    write_openmw_cfg(config_dir.path(), data_dir.path());
    write_meshes(data_dir.path());
    write_context_plugin(data_dir.path(), profile);
    write_target_plugin(data_dir.path(), profile);

    let args = unclip_args(data_dir.path().join("TargetGroundcover.omwaddon"));
    let mut stdout = Vec::new();
    let started = Instant::now();
    greenmote::unclip::run_with_output(
        Some(&config_dir.path().join("openmw.cfg")),
        None,
        &args,
        &mut stdout,
    )
    .unwrap();
    let elapsed = started.elapsed();

    let report: Value = serde_json::from_slice(&stdout).unwrap();
    let summary = &report["summary"];
    let target_refs = profile.target_refs();
    assert_eq!(summary["target_refs_total"], target_refs);
    assert_eq!(summary["target_refs_matching_filter"], target_refs);
    assert_eq!(summary["filtered_refs_with_mesh_contact"], target_refs);
    assert_eq!(
        summary["filtered_refs_mesh_contact_above_terrain"],
        profile.count_serial_mods(&[10, 11, 12])
    );
    assert_eq!(
        summary["filtered_refs_mesh_contact_below_terrain"],
        profile.count_serial_mods(&[13, 14, 15])
    );
    assert_eq!(
        summary["filtered_refs_static_bounds_fully_occluded"],
        profile.count_serial_mods(&[16])
    );
    assert!(
        summary["filtered_refs_static_bounds_relocatable"]
            .as_u64()
            .unwrap()
            >= 3_000
    );
    assert!(config_dir.path().join("greenmote.log").is_file());

    println!(
        "synthetic unclip {}: cells={}, refs={target_refs}, terrain_above={}, terrain_below={}, static_fully_occluded={}, static_relocatable={}, elapsed={elapsed:.2?}",
        profile.name,
        profile.grid_side * profile.grid_side,
        summary["filtered_refs_mesh_contact_above_terrain"],
        summary["filtered_refs_mesh_contact_below_terrain"],
        summary["filtered_refs_static_bounds_fully_occluded"],
        summary["filtered_refs_static_bounds_relocatable"],
    );
}

impl Profile {
    fn target_refs(self) -> usize {
        usize::try_from(self.grid_side * self.grid_side).unwrap() * self.refs_per_cell
    }

    fn count_serial_mods(self, mods: &[usize]) -> usize {
        (0..self.target_refs())
            .filter(|serial| mods.contains(&(serial % 20)))
            .count()
    }
}

fn write_openmw_cfg(config_dir: &Path, data_dir: &Path) {
    std::fs::write(
        config_dir.join("openmw.cfg"),
        format!(
            "data-local={}\ndata={}\ncontent=Context.esm\n",
            data_dir.display(),
            data_dir.display()
        ),
    )
    .unwrap();
}

fn unclip_args(target: PathBuf) -> greenmote::unclip::UnclipArgs {
    let cli = Cli::parse_from([
        "greenmote".into(),
        "unclip".into(),
        "--plugin".into(),
        target.into_os_string(),
        "--structured".into(),
    ]);
    let Command::Unclip(args) = cli.command_or_default() else {
        panic!("expected unclip args");
    };
    *args
}

fn write_meshes(data_dir: &Path) {
    let mesh_dir = data_dir.join("Meshes/bench");
    std::fs::create_dir_all(&mesh_dir).unwrap();
    write_nif(
        &mesh_dir.join("grass.nif"),
        &[
            [-16.0, -16.0, 0.0],
            [16.0, -16.0, 0.0],
            [-16.0, 16.0, 0.0],
            [16.0, 16.0, 32.0],
        ],
    );
    write_nif(
        &mesh_dir.join("rock_full.nif"),
        &[
            [-32.0, -32.0, -8.0],
            [32.0, -32.0, -8.0],
            [-32.0, 32.0, -8.0],
            [32.0, 32.0, 48.0],
        ],
    );
    write_nif(
        &mesh_dir.join("rock_partial.nif"),
        &[
            [0.0, -32.0, -8.0],
            [32.0, -32.0, -8.0],
            [0.0, 32.0, -8.0],
            [32.0, 32.0, 48.0],
        ],
    );
}

fn write_nif(path: &Path, vertices: &[[f32; 3]; 4]) {
    let geometry_data = NiTriShapeData {
        base: NiTriBasedGeomData {
            base: NiGeometryData {
                vertices: vertices
                    .iter()
                    .map(|vertex| NifVec3::new(vertex[0], vertex[1], vertex[2]))
                    .collect(),
                ..NiGeometryData::default()
            },
        },
        triangles: vec![[0, 1, 2], [1, 2, 3]],
        shared_normals: Vec::new(),
    };
    let mut stream = NiStream::new();
    let data_key = stream.objects.insert(NiType::from(geometry_data));
    let shape = NiTriShape {
        base: NiTriBasedGeom {
            base: NiGeometry {
                base: NiAVObject {
                    base: NiObjectNET {
                        name: "synthetic".to_owned(),
                        ..NiObjectNET::default()
                    },
                    ..NiAVObject::default()
                },
                geometry_data: NiLink::new(data_key),
                ..NiGeometry::default()
            },
        },
    };
    let shape_key = stream.objects.insert(NiType::from(shape));
    stream.roots.push(NiLink::new(shape_key));
    std::fs::write(path, stream.save_bytes().unwrap()).unwrap();
}

fn write_context_plugin(data_dir: &Path, profile: Profile) {
    let mut objects = vec![
        TES3Object::Header(Header::default()),
        static_record("bench_rock_full", "bench/rock_full.nif").into(),
        static_record("bench_rock_partial", "bench/rock_partial.nif").into(),
    ];
    for y in -1..=profile.grid_side {
        for x in -1..=profile.grid_side {
            objects.push(landscape((x, y)).into());
        }
    }
    for y in 0..profile.grid_side {
        for x in 0..profile.grid_side {
            objects.push(context_cell((x, y), profile).into());
        }
    }
    let mut plugin = Plugin { objects };
    plugin.save_path(data_dir.join("Context.esm")).unwrap();
}

fn write_target_plugin(data_dir: &Path, profile: Profile) {
    let mut objects = vec![
        TES3Object::Header(Header::default()),
        static_record("bench_grass", "bench/grass.nif").into(),
    ];
    for y in 0..profile.grid_side {
        for x in 0..profile.grid_side {
            objects.push(target_cell((x, y), profile).into());
        }
    }
    let mut plugin = Plugin { objects };
    plugin
        .save_path(data_dir.join("TargetGroundcover.omwaddon"))
        .unwrap();
}

fn static_record(id: &str, mesh: &str) -> Static {
    Static {
        id: id.to_owned(),
        mesh: mesh.to_owned(),
        ..Static::default()
    }
}

fn landscape(grid: (i32, i32)) -> Landscape {
    Landscape {
        grid,
        landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
        ..Landscape::default()
    }
}

fn target_cell(grid: (i32, i32), profile: Profile) -> Cell {
    let mut cell = exterior_cell(grid);
    for local in 0..profile.refs_per_cell {
        let serial = serial(grid, local, profile);
        cell.references.insert(
            (0, u32::try_from(local).unwrap()),
            Reference {
                id: "bench_grass".to_owned(),
                translation: [world_x(grid, local), world_y(grid), target_z(serial)],
                ..Reference::default()
            },
        );
    }
    cell
}

fn context_cell(grid: (i32, i32), profile: Profile) -> Cell {
    let mut cell = exterior_cell(grid);
    let mut key = 0;
    for local in 0..profile.refs_per_cell {
        let serial = serial(grid, local, profile);
        let id = match serial % 20 {
            16 => "bench_rock_full",
            17 => "bench_rock_partial",
            _ => continue,
        };
        cell.references.insert(
            (0, key),
            Reference {
                id: id.to_owned(),
                translation: [world_x(grid, local), world_y(grid), 0.0],
                ..Reference::default()
            },
        );
        key += 1;
    }
    cell
}

fn exterior_cell(grid: (i32, i32)) -> Cell {
    Cell {
        name: "Synthetic Region".to_owned(),
        data: CellData {
            grid,
            ..CellData::default()
        },
        ..Cell::default()
    }
}

fn serial(grid: (i32, i32), local: usize, profile: Profile) -> usize {
    usize::try_from(grid.1 * profile.grid_side + grid.0).unwrap() * profile.refs_per_cell + local
}

#[allow(clippy::cast_precision_loss)]
fn world_x(grid: (i32, i32), local: usize) -> f32 {
    grid.0 as f32 * 8192.0 + 128.0 + local as f32 * 64.0
}

#[allow(clippy::cast_precision_loss)]
fn world_y(grid: (i32, i32)) -> f32 {
    grid.1 as f32 * 8192.0 + 128.0
}

fn target_z(serial: usize) -> f32 {
    match serial % 20 {
        10..=12 => 64.0,
        13..=15 => -64.0,
        _ => 0.0,
    }
}
