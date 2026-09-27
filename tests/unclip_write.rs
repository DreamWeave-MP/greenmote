// SPDX-License-Identifier: GPL-3.0-only

//! End-to-end unclip: dry run, in-place write, copy write, verification, idempotency.
#![allow(clippy::float_cmp, clippy::similar_names)]

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
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

type NifVec3 = tes3::nif::glam::Vec3;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-unclip-write-{name}-{}-{}",
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

const FLOATING: (u32, u32) = (0, 1);
const BURIED: (u32, u32) = (0, 2);
const FINE: (u32, u32) = (0, 3);
const IN_ROCK: (u32, u32) = (0, 4);
const AT_ROCK_EDGE: (u32, u32) = (0, 5);
const INHERITED_FLOATING: (u32, u32) = (1, 9);

struct Fixture {
    config_dir: TempDir,
    data_dir: TempDir,
    data_local: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let config_dir = TempDir::new(format!("{name}-config").as_str());
        let data_dir = TempDir::new(format!("{name}-data").as_str());
        let data_local = data_dir.path().join("local");
        std::fs::create_dir_all(&data_local).unwrap();
        std::fs::write(
            config_dir.path().join("openmw.cfg"),
            format!(
                "data-local={}\ndata={}\ncontent=Context.esm\n",
                data_local.display(),
                data_dir.path().display()
            ),
        )
        .unwrap();
        write_meshes(data_dir.path());
        write_context_plugin(data_dir.path());
        let target = data_dir.path().join("Grass.esp");
        write_target_plugin(&target);
        Self {
            config_dir,
            data_dir,
            data_local,
            target,
        }
    }

    fn run(&self, extra: &[&str]) -> Value {
        let mut argv = vec![
            "greenmote".to_owned(),
            "unclip".to_owned(),
            "--plugin".to_owned(),
            self.target.display().to_string(),
            "--structured".to_owned(),
            "--verbose".to_owned(),
        ];
        argv.extend(extra.iter().map(|arg| (*arg).to_owned()));
        let cli = Cli::parse_from(argv);
        let Command::Unclip(args) = cli.command_or_default() else {
            panic!("expected unclip args");
        };
        let mut stdout = Vec::new();
        greenmote::unclip::run_with_output(
            Some(&self.config_dir.path().join("openmw.cfg")),
            None,
            &args,
            &mut stdout,
        )
        .unwrap();
        serde_json::from_slice(&stdout).unwrap()
    }
}

#[test]
fn plugin_outside_data_directories_uses_its_own_mod_folder() {
    let fixture = Fixture::new("outside");
    let mod_dir = fixture.data_dir.path().join("unlisted-mod");
    std::fs::create_dir_all(mod_dir.join("Meshes/patchtest")).unwrap();
    std::fs::rename(
        fixture.data_dir.path().join("Meshes/patchtest/grass.nif"),
        mod_dir.join("Meshes/patchtest/grass.nif"),
    )
    .unwrap();
    let target = mod_dir.join("Grass.esp");
    std::fs::rename(&fixture.target, &target).unwrap();
    let fixture = Fixture { target, ..fixture };

    let report = fixture.run(&[]);
    assert_eq!(
        report["added_data_directories"],
        serde_json::json!([mod_dir.display().to_string()])
    );
    assert_eq!(report["counts"]["skip"], 0, "{report}");
    assert_eq!(report["counts"]["total"], 6);
}

#[test]
fn missing_grass_meshes_are_reported_loudly() {
    let fixture = Fixture::new("missing-mesh");
    std::fs::remove_file(fixture.data_dir.path().join("Meshes/patchtest/grass.nif")).unwrap();

    let report = fixture.run(&[]);
    assert_eq!(report["counts"]["skip"], 6);
    let errors = report["mesh_errors"].as_object().unwrap();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.values().next().unwrap(), 6);
}

fn verdict_of(report: &Value, key: (u32, u32)) -> &Value {
    report["refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["key"] == serde_json::json!([key.0, key.1]))
        .unwrap_or_else(|| panic!("no verdict for {key:?}"))
}

#[test]
fn dry_run_reports_verdicts_and_writes_nothing() {
    let fixture = Fixture::new("dry-run");
    let report = fixture.run(&[]);

    assert_eq!(report["mode"]["mode"], "dry_run");
    assert_eq!(
        report["mode"]["would_write"],
        fixture.target.display().to_string()
    );
    assert_eq!(report["counts"]["total"], 6);
    assert_eq!(verdict_of(&report, FLOATING)["verdict"]["verdict"], "fix");
    assert_eq!(verdict_of(&report, FLOATING)["verdict"]["grounded"], true);
    assert_eq!(verdict_of(&report, BURIED)["verdict"]["verdict"], "fix");
    assert_eq!(verdict_of(&report, FINE)["verdict"]["verdict"], "keep");
    assert_eq!(verdict_of(&report, IN_ROCK)["verdict"]["verdict"], "delete");
    assert_eq!(
        verdict_of(&report, IN_ROCK)["verdict"]["reason"]["kind"],
        "inside_static"
    );
    assert_eq!(
        verdict_of(&report, AT_ROCK_EDGE)["verdict"]["verdict"],
        "fix"
    );
    assert_eq!(verdict_of(&report, AT_ROCK_EDGE)["verdict"]["moved"], true);
    assert_eq!(
        verdict_of(&report, INHERITED_FLOATING)["verdict"]["verdict"],
        "fix"
    );
    assert!(report.get("write").is_none());
    assert!(!fixture.data_dir.path().join("Grass.esp.bak").exists());
    let log = std::fs::read_to_string(
        fixture
            .config_dir
            .path()
            .join(greenmote::unclip::UNCLIP_LOG_NAME),
    )
    .unwrap();
    assert!(log.contains("delete_inside_static"), "{log}");
}

#[test]
fn output_plugin_writes_a_complete_copy_and_leaves_the_source_alone() {
    let fixture = Fixture::new("copy");
    let copy = fixture.data_local.join("out/Grass_unclipped.esp");
    let report = fixture.run(&["--write", "--output-plugin", copy.to_str().unwrap()]);

    let write = &report["write"];
    assert_eq!(write["replaced_source"], false);
    assert_eq!(write["verified"], true);
    assert_eq!(write["refs_fixed"], 4);
    assert_eq!(write["refs_deleted"], 1);
    assert_eq!(write["backups"], serde_json::json!([]));

    let written = Plugin::from_path(&copy).unwrap();
    assert_eq!(
        written.header().unwrap().masters,
        vec![("Morrowind.esm".to_owned(), 1)]
    );
    assert!(written.objects_of_type::<Static>().next().is_some());
    let cell = written.objects_of_type::<Cell>().next().unwrap();
    assert_eq!(cell.references.len(), 5, "own deleted ref is gone");
    assert!(cell.references[&FLOATING].translation[2] < 1.0);
    assert_eq!(cell.references[&FINE].translation[2], -1.0);
    assert!(cell.references.contains_key(&INHERITED_FLOATING));

    let source = Plugin::from_path(&fixture.target).unwrap();
    let source_cell = source.objects_of_type::<Cell>().next().unwrap();
    assert_eq!(source_cell.references[&FLOATING].translation[2], 30.0);
    assert!(!fixture.data_dir.path().join("Grass.esp.bak").exists());
}

#[test]
fn in_place_write_is_idempotent_and_keeps_backups() {
    let fixture = Fixture::new("in-place");
    let report = fixture.run(&["--write"]);
    let write = &report["write"];
    assert_eq!(write["replaced_source"], true);
    assert_eq!(write["verified"], true);
    assert_eq!(
        write["backups"],
        serde_json::json!([
            fixture
                .data_dir
                .path()
                .join("Grass.esp.greenmote-original")
                .display()
                .to_string(),
            fixture
                .data_dir
                .path()
                .join("Grass.esp.bak")
                .display()
                .to_string()
        ])
    );

    let source = Plugin::from_path(&fixture.target).unwrap();
    let cell = source.objects_of_type::<Cell>().next().unwrap();
    assert!(!cell.references.contains_key(&IN_ROCK), "own ref removed");
    assert_eq!(cell.references[&FINE].translation[2], -1.0);
    assert!(cell.references[&FLOATING].translation[2] < 1.0);
    assert!(cell.references.contains_key(&INHERITED_FLOATING));

    let second = fixture.run(&[]);
    assert_eq!(second["counts"]["fix"], 0, "{second}");
    assert_eq!(second["counts"]["delete"], 0, "{second}");
    assert_eq!(second["counts"]["keep"], 5);
}

fn unit_grass() -> [[f32; 3]; 4] {
    [
        [-16.0, -16.0, 0.0],
        [16.0, -16.0, 0.0],
        [-16.0, 16.0, 0.0],
        [16.0, 16.0, 32.0],
    ]
}

fn write_meshes(data_dir: &Path) {
    let mesh_dir = data_dir.join("Meshes/patchtest");
    std::fs::create_dir_all(&mesh_dir).unwrap();
    write_nif(&mesh_dir.join("grass.nif"), &unit_grass());
    // A solid box so refs at its centre are genuinely inside the convex hull.
    let (min, max) = ([-64.0, -64.0, -8.0], [64.0, 64.0, 96.0]);
    let mut corners = Vec::new();
    for x in [min[0], max[0]] {
        for y in [min[1], max[1]] {
            for z in [min[2], max[2]] {
                corners.push([x, y, z]);
            }
        }
    }
    write_nif_box(&mesh_dir.join("rock.nif"), &corners);
}

fn write_nif(path: &Path, vertices: &[[f32; 3]; 4]) {
    write_nif_points(path, vertices);
}

fn write_nif_points(path: &Path, vertices: &[[f32; 3]]) {
    let triangles = (2..vertices.len())
        .map(|index| {
            [
                0,
                u16::try_from(index - 1).unwrap(),
                u16::try_from(index).unwrap(),
            ]
        })
        .collect::<Vec<_>>();
    write_nif_triangles(path, vertices, triangles);
}

/// Corners ordered (x, y, z) with z fastest, as produced by `write_meshes`; twelve closed faces.
fn write_nif_box(path: &Path, corners: &[[f32; 3]]) {
    let quads = [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ];
    let triangles = quads
        .iter()
        .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
        .collect::<Vec<_>>();
    write_nif_triangles(path, corners, triangles);
}

fn write_nif_triangles(path: &Path, vertices: &[[f32; 3]], triangles: Vec<[u16; 3]>) {
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
        triangles,
        shared_normals: Vec::new(),
    };
    let mut stream = NiStream::new();
    let data_key = stream.objects.insert(NiType::from(geometry_data));
    let shape = NiTriShape {
        base: NiTriBasedGeom {
            base: NiGeometry {
                base: NiAVObject {
                    base: NiObjectNET {
                        name: "patchtest".to_owned(),
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

fn write_context_plugin(data_dir: &Path) {
    let mut objects = vec![
        TES3Object::Header(Header::default()),
        Static {
            id: "patchtest_rock".to_owned(),
            mesh: "patchtest/rock.nif".to_owned(),
            ..Static::default()
        }
        .into(),
    ];
    for y in -1..=1 {
        for x in -1..=1 {
            objects.push(
                Landscape {
                    grid: (x, y),
                    landscape_flags: LandscapeFlags::USES_VERTEX_HEIGHTS_AND_NORMALS,
                    ..Landscape::default()
                }
                .into(),
            );
        }
    }
    let mut cell = exterior_cell();
    cell.references.insert(
        (0, 1),
        Reference {
            id: "patchtest_rock".to_owned(),
            translation: [2000.0, 2000.0, 0.0],
            ..Reference::default()
        },
    );
    objects.push(cell.into());
    let mut plugin = Plugin { objects };
    plugin.save_path(data_dir.join("Context.esm")).unwrap();
}

fn write_target_plugin(path: &Path) {
    let mut cell = exterior_cell();
    let grass = |x: f32, y: f32, z: f32| Reference {
        id: "patchtest_grass".to_owned(),
        translation: [x, y, z],
        temporary: true,
        ..Reference::default()
    };
    cell.references.insert(FLOATING, grass(500.0, 500.0, 30.0));
    cell.references.insert(BURIED, grass(600.0, 500.0, -30.0));
    cell.references.insert(FINE, grass(700.0, 500.0, -1.0));
    cell.references.insert(IN_ROCK, grass(2000.0, 2000.0, 0.0));
    cell.references
        .insert(AT_ROCK_EDGE, grass(2070.0, 2000.0, 0.0));
    let mut inherited = grass(800.0, 500.0, 25.0);
    inherited.mast_index = 1;
    inherited.refr_index = INHERITED_FLOATING.1;
    cell.references.insert(INHERITED_FLOATING, inherited);
    let mut plugin = Plugin {
        objects: vec![
            TES3Object::Header(Header {
                masters: vec![("Morrowind.esm".to_owned(), 1)],
                ..Header::default()
            }),
            Static {
                id: "patchtest_grass".to_owned(),
                mesh: "patchtest/grass.nif".to_owned(),
                ..Static::default()
            }
            .into(),
            cell.into(),
        ],
    };
    plugin.save_path(path).unwrap();
}

fn exterior_cell() -> Cell {
    Cell {
        name: "Patch Region".to_owned(),
        data: CellData {
            grid: (0, 0),
            ..CellData::default()
        },
        ..Cell::default()
    }
}
