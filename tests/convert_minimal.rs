use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use tes3::esp::{Cell, CellData, CellFlags, Header, Plugin, Reference, Static, TES3Object};

use greenmote::groundcover::{
    DELETED_PLUGIN_NAME, GROUNDCOVER_PLUGIN_NAME, GroundcoverArgs, LOG_NAME,
};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-convert-test-{name}-{}-{}",
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
fn convert_minimal_fixture_writes_plugins_and_copied_meshes() {
    let config_dir = TempDir::new("config");
    let data_dir = TempDir::new("data");
    let output_dir = TempDir::new("output");
    write_openmw_cfg(config_dir.path(), data_dir.path(), output_dir.path());
    write_source_plugin(data_dir.path());
    write_mesh(data_dir.path(), "Meshes/flora/grass.nif", b"mesh");

    greenmote::groundcover::run(args_for(config_dir.path())).unwrap();

    let groundcover_path = output_dir.path().join(GROUNDCOVER_PLUGIN_NAME);
    let deleted_path = output_dir.path().join(DELETED_PLUGIN_NAME);
    assert!(groundcover_path.is_file());
    assert!(deleted_path.is_file());
    assert!(
        output_dir
            .path()
            .join("Meshes/grass/flora/grass.nif")
            .is_file()
    );
    assert!(config_dir.path().join(LOG_NAME).is_file());

    let groundcover = Plugin::from_path(&groundcover_path).unwrap();
    let deleted = Plugin::from_path(&deleted_path).unwrap();
    let source_size = std::fs::metadata(data_dir.path().join("Source.esp"))
        .unwrap()
        .len();

    assert_eq!(
        header(&groundcover).masters,
        vec![("Source.esp".to_owned(), source_size)]
    );
    assert_eq!(
        header(&deleted).masters,
        vec![("Source.esp".to_owned(), source_size)]
    );
    assert_eq!(header(&groundcover).num_objects, 2);
    assert_eq!(header(&deleted).num_objects, 1);

    let generated_static = groundcover.objects_of_type::<Static>().next().unwrap();
    assert!(generated_static.id.starts_with("gm_"));
    assert_eq!(generated_static.mesh, "grass\\flora\\grass.nif");
    assert_eq!(groundcover.objects_of_type::<Static>().count(), 1);
    assert!(
        !output_dir
            .path()
            .join("Meshes/grass/flora/missing-unused.nif")
            .exists()
    );

    let groundcover_cell = groundcover.objects_of_type::<Cell>().next().unwrap();
    assert_eq!(groundcover_cell.data.grid, (1, 2));
    assert!(groundcover_cell.references.contains_key(&(1, 7)));
    assert!(!groundcover_cell.references.contains_key(&(1, 8)));
    assert_eq!(groundcover_cell.references[&(1, 7)].mast_index, 1);
    assert_eq!(groundcover_cell.references[&(1, 7)].id, generated_static.id);
    assert_eq!(groundcover_cell.references[&(1, 7)].deleted, None);

    let deleted_cell = deleted.objects_of_type::<Cell>().next().unwrap();
    assert_eq!(deleted_cell.references.len(), 1);
    assert_eq!(deleted_cell.references[&(1, 7)].id, "Flora_Grass_01");
    assert_eq!(deleted_cell.references[&(1, 7)].deleted, Some(true));

    let log = std::fs::read_to_string(config_dir.path().join(LOG_NAME)).unwrap();
    let static_line = log.lines().find(|line| line.starts_with("STAT ")).unwrap();
    assert!(static_line.contains("STAT \"flora_grass_01\" from \"Source.esp\""));
    assert!(static_line.contains(&format!("generated {:?}", generated_static.id)));
}

#[test]
fn convert_dry_run_writes_no_outputs() {
    let config_dir = TempDir::new("dry-config");
    let data_dir = TempDir::new("dry-data");
    let output_dir = TempDir::new("dry-output");
    write_openmw_cfg(config_dir.path(), data_dir.path(), output_dir.path());
    write_source_plugin(data_dir.path());
    write_mesh(data_dir.path(), "Meshes/flora/grass.nif", b"mesh");
    let mut args = args_for(config_dir.path());
    args.dry_run = Some(true);

    greenmote::groundcover::run(args).unwrap();

    assert!(!output_dir.path().join(GROUNDCOVER_PLUGIN_NAME).exists());
    assert!(!output_dir.path().join(DELETED_PLUGIN_NAME).exists());
    assert!(!output_dir.path().join("Meshes").exists());
}

#[test]
fn convert_outputs_are_deterministic_for_same_fixture() {
    let first = run_minimal_fixture("deterministic-a");
    let second = run_minimal_fixture("deterministic-b");

    assert_eq!(first.groundcover, second.groundcover);
    assert_eq!(first.deleted, second.deleted);
}

struct GeneratedBytes {
    groundcover: Vec<u8>,
    deleted: Vec<u8>,
}

fn run_minimal_fixture(name: &str) -> GeneratedBytes {
    let config_dir = TempDir::new(&format!("{name}-config"));
    let data_dir = TempDir::new(&format!("{name}-data"));
    let output_dir = TempDir::new(&format!("{name}-output"));
    write_openmw_cfg(config_dir.path(), data_dir.path(), output_dir.path());
    write_source_plugin(data_dir.path());
    write_mesh(data_dir.path(), "Meshes/flora/grass.nif", b"mesh");

    greenmote::groundcover::run(args_for(config_dir.path())).unwrap();

    GeneratedBytes {
        groundcover: std::fs::read(output_dir.path().join(GROUNDCOVER_PLUGIN_NAME)).unwrap(),
        deleted: std::fs::read(output_dir.path().join(DELETED_PLUGIN_NAME)).unwrap(),
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

fn write_openmw_cfg(config_dir: &Path, data_dir: &Path, output_dir: &Path) {
    std::fs::write(
        config_dir.join("openmw.cfg"),
        format!(
            "data-local={}\ndata={}\ncontent=Source.esp\n",
            output_dir.display(),
            data_dir.display()
        ),
    )
    .unwrap();
}

fn write_source_plugin(data_dir: &Path) {
    let mut plugin = Plugin {
        objects: vec![
            TES3Object::Header(Header {
                num_objects: 4,
                ..Header::default()
            }),
            static_record("flora_grass_01", "flora/grass.nif").into(),
            static_record("flora_grass_unused", "flora/missing-unused.nif").into(),
            exterior_cell([
                ((0, 7), reference("Flora_Grass_01")),
                ((0, 8), reference("crate_01")),
            ])
            .into(),
            interior_cell([((0, 9), reference("flora_grass_01"))]).into(),
        ],
    };
    plugin.save_path(data_dir.join("Source.esp")).unwrap();
}

fn static_record(id: &str, mesh: &str) -> Static {
    Static {
        id: id.to_owned(),
        mesh: mesh.to_owned(),
        ..Static::default()
    }
}

fn exterior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
    let mut cell = Cell {
        name: "Ascadian Isles Region".to_owned(),
        data: CellData {
            grid: (1, 2),
            ..CellData::default()
        },
        ..Cell::default()
    };
    cell.references.extend(refs);
    cell
}

fn interior_cell(refs: impl IntoIterator<Item = ((u32, u32), Reference)>) -> Cell {
    let mut cell = Cell {
        name: "Balmora, Caius Cosades' House".to_owned(),
        data: CellData {
            flags: CellFlags::IS_INTERIOR,
            ..CellData::default()
        },
        ..Cell::default()
    };
    cell.references.extend(refs);
    cell
}

fn reference(id: &str) -> Reference {
    Reference {
        id: id.to_owned(),
        translation: [1.0, 2.0, 3.0],
        ..Reference::default()
    }
}

fn write_mesh(data_dir: &Path, relative_path: &str, contents: &[u8]) {
    let path = relative_path
        .split('/')
        .fold(data_dir.to_path_buf(), |path, part| path.join(part));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn header(plugin: &Plugin) -> &Header {
    plugin.objects_of_type::<Header>().next().unwrap()
}
