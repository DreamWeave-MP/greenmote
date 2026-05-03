use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use greenmote::groundcover::{DELETED_PLUGIN_NAME, GROUNDCOVER_PLUGIN_NAME, GroundcoverArgs};
use tes3::esp::{Cell, CellData, Header, Plugin, Reference, Static, TES3Object};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

const PLUGINS: usize = 8;
const CELL_PLUGINS: usize = 5;
#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    statics_per_plugin: usize,
    used_statics: usize,
    grid_side: i32,
    refs_per_cell: usize,
}

const MEDIUM: Profile = Profile {
    name: "medium",
    statics_per_plugin: 250,
    used_statics: 512,
    grid_side: 64,
    refs_per_cell: 64,
};

const LARGE: Profile = Profile {
    name: "large",
    statics_per_plugin: 512,
    used_statics: 1024,
    grid_side: 96,
    refs_per_cell: 96,
};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-synthetic-convert-{name}-{}-{}",
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
#[ignore = "synthetic medium convert benchmark/torture test"]
fn synthetic_convert_medium_torture() {
    run_synthetic_convert_torture(MEDIUM);
}

#[test]
#[ignore = "synthetic large convert benchmark/torture test"]
fn synthetic_convert_large_torture() {
    run_synthetic_convert_torture(LARGE);
}

fn run_synthetic_convert_torture(profile: Profile) {
    let config_dir = TempDir::new("config");
    let data_dir = TempDir::new("data");
    let output_dir = TempDir::new("output");
    write_openmw_cfg(config_dir.path(), data_dir.path(), output_dir.path());
    write_plugins(data_dir.path(), profile);
    write_meshes(data_dir.path(), profile);

    let started = Instant::now();
    greenmote::groundcover::run(args_for(config_dir.path())).unwrap();
    let elapsed = started.elapsed();

    let groundcover = Plugin::from_path(output_dir.path().join(GROUNDCOVER_PLUGIN_NAME)).unwrap();
    let deleted = Plugin::from_path(output_dir.path().join(DELETED_PLUGIN_NAME)).unwrap();
    let generated_static_count = groundcover.objects_of_type::<Static>().count();
    let groundcover_ref_count = ref_count(&groundcover);
    let deleted_ref_count = ref_count(&deleted);
    let copied_mesh_count = count_files(output_dir.path().join("Meshes/grass"));
    let cell_count = usize::try_from(profile.grid_side * profile.grid_side).unwrap();
    let expected_touched_refs = CELL_PLUGINS * cell_count * profile.refs_per_cell / 2;

    assert_eq!(generated_static_count, profile.used_statics);
    assert_eq!(groundcover_ref_count, expected_touched_refs);
    assert_eq!(deleted_ref_count, expected_touched_refs);
    assert_eq!(copied_mesh_count, profile.used_statics);

    let log = std::fs::read_to_string(config_dir.path().join("greenmote.log")).unwrap();
    assert!(log.contains("# skipped generated plugins:"));
    assert_log_sources_follow_load_order(&log, "STAT ");
    assert_log_sources_follow_load_order(&log, "CELL refs from ");

    println!(
        "synthetic convert {}: plugins={PLUGINS}, statics={}, cells={}, refs={}, touched_refs={expected_touched_refs}, meshes={copied_mesh_count}, elapsed={elapsed:.2?}",
        profile.name,
        PLUGINS * profile.statics_per_plugin,
        profile.grid_side * profile.grid_side,
        CELL_PLUGINS * cell_count * profile.refs_per_cell,
    );
}

fn write_openmw_cfg(config_dir: &Path, data_dir: &Path, output_dir: &Path) {
    let mut contents = format!(
        "data-local={}\ndata={}\n",
        output_dir.display(),
        data_dir.display()
    );
    for plugin in plugin_names() {
        writeln!(contents, "content={plugin}").unwrap();
    }
    contents.push_str("groundcover=groundcover.omwaddon\ncontent=deleted_groundcover.omwaddon\n");
    std::fs::write(config_dir.join("openmw.cfg"), contents).unwrap();
}

fn plugin_names() -> [&'static str; PLUGINS] {
    [
        "Morrowind.esm",
        "Tribunal.esm",
        "Bloodmoon.esm",
        "TR_Mainland.esm",
        "Sky_Main.esm",
        "Cyr_Main.esm",
        "Patch_A.esp",
        "Patch_B.esp",
    ]
}

fn write_plugins(data_dir: &Path, profile: Profile) {
    for (load_index, name) in plugin_names().into_iter().enumerate() {
        let mut objects = Vec::with_capacity(profile.statics_per_plugin + 2);
        objects.push(TES3Object::Header(Header::default()));
        for local in 0..profile.statics_per_plugin {
            let id = format!(
                "bench_grass_{:04}",
                load_index * profile.statics_per_plugin + local
            );
            objects.push(static_record(&id).into());
        }
        if load_index == PLUGINS - 1 {
            objects.push(static_record("bench_grass_0000").into());
        }
        objects.push(interior_cell().into());
        if load_index < 5 {
            for y in 0..profile.grid_side {
                for x in 0..profile.grid_side {
                    objects.push(exterior_cell((x, y), profile).into());
                }
            }
        }
        let mut plugin = Plugin { objects };
        plugin.save_path(data_dir.join(name)).unwrap();
    }

    Plugin::default()
        .save_path(data_dir.join(GROUNDCOVER_PLUGIN_NAME))
        .unwrap();
    Plugin::default()
        .save_path(data_dir.join(DELETED_PLUGIN_NAME))
        .unwrap();
}

fn static_record(id: &str) -> Static {
    Static {
        id: id.to_owned(),
        mesh: format!("flora/{id}.nif"),
        ..Static::default()
    }
}

#[allow(clippy::cast_precision_loss)]
fn exterior_cell(grid: (i32, i32), profile: Profile) -> Cell {
    let mut cell = Cell {
        name: "Synthetic Region".to_owned(),
        data: CellData {
            grid,
            ..CellData::default()
        },
        ..Cell::default()
    };
    for index in 0..profile.refs_per_cell {
        let id = if index % 2 == 0 {
            format!(
                "bench_grass_{:04}",
                (cell_serial(grid, profile) * (profile.refs_per_cell / 2) + index / 2)
                    % profile.used_statics
            )
        } else {
            format!("bench_crate_{index:04}")
        };
        cell.references.insert(
            (0, u32::try_from(index).unwrap()),
            Reference {
                id,
                translation: [
                    grid.0 as f32 * 8192.0 + index as f32,
                    grid.1 as f32 * 8192.0,
                    0.0,
                ],
                ..Reference::default()
            },
        );
    }
    cell
}

fn cell_serial(grid: (i32, i32), profile: Profile) -> usize {
    usize::try_from(grid.1 * profile.grid_side + grid.0).unwrap()
}

fn interior_cell() -> Cell {
    let mut cell = Cell {
        name: "Synthetic Interior".to_owned(),
        data: CellData {
            flags: tes3::esp::CellFlags::IS_INTERIOR,
            ..CellData::default()
        },
        ..Cell::default()
    };
    cell.references.insert(
        (0, 0),
        Reference {
            id: "bench_grass_0001".to_owned(),
            ..Reference::default()
        },
    );
    cell
}

fn write_meshes(data_dir: &Path, profile: Profile) {
    let mesh_dir = data_dir.join("Meshes/flora");
    std::fs::create_dir_all(&mesh_dir).unwrap();
    for index in 0..profile.used_statics {
        std::fs::write(
            mesh_dir.join(format!("bench_grass_{index:04}.nif")),
            b"synthetic mesh",
        )
        .unwrap();
    }
}

fn args_for(config_dir: &Path) -> GroundcoverArgs {
    GroundcoverArgs {
        openmw_cfg: Some(config_dir.join("openmw.cfg")),
        config: None,
        output: None,
        ignored_plugins: Vec::new(),
        dry_run: None,
        validate_config: None,
        auto_enable: false,
        debug: false,
    }
}

fn ref_count(plugin: &Plugin) -> usize {
    plugin
        .objects_of_type::<Cell>()
        .map(|cell| cell.references.len())
        .sum()
}

fn count_files(path: PathBuf) -> usize {
    std::fs::read_dir(path)
        .unwrap()
        .map(Result::unwrap)
        .map(|entry| entry.path())
        .map(|path| if path.is_dir() { count_files(path) } else { 1 })
        .sum()
}

fn assert_log_sources_follow_load_order(log: &str, prefix: &str) {
    let names = plugin_names();
    let indices = log
        .lines()
        .filter(|line| line.starts_with(prefix))
        .map(|line| {
            names
                .iter()
                .position(|name| line.contains(&format!("\"{name}\"")))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(indices.windows(2).all(|pair| pair[0] <= pair[1]));
}
