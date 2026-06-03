// SPDX-License-Identifier: GPL-3.0-only

use std::{path::Path, sync::atomic::AtomicU64};

use clap::Parser;
use greenmote::{Cli, Command};
use serde_json::Value;
use tes3::esp::{Cell, CellData, Header, Plugin, Reference, TES3Object};

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: std::path::PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-unclip-noop-{name}-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
fn unclip_no_matching_target_refs_skips_missing_context_plugins() {
    let config_dir = TempDir::new("config");
    let data_dir = TempDir::new("data");
    write_openmw_cfg(config_dir.path(), data_dir.path());
    let target_path = data_dir.path().join("TargetGroundcover.omwaddon");
    write_target_plugin(&target_path);

    let args = unclip_args(&target_path);
    let mut stdout = Vec::new();

    greenmote::unclip::run_with_output(
        Some(&config_dir.path().join("openmw.cfg")),
        None,
        &args,
        &mut stdout,
    )
    .unwrap();

    let report: Value = serde_json::from_slice(&stdout).unwrap();
    let summary = &report["summary"];
    assert_eq!(summary["target_refs_total"], 1);
    assert_eq!(summary["target_refs_matching_filter"], 0);
    assert_eq!(summary["active_cells"], 0);
    assert_eq!(summary["loaded_terrain_cells_total"], 0);
    assert_eq!(report["write"]["written"], false);
    assert_eq!(report["write"]["no_write_reason"], "no_refs_changed");
    assert!(config_dir.path().join("greenmote.log").is_file());
    assert!(
        !data_dir
            .path()
            .join("TargetGroundcover.omwaddon.001")
            .exists()
    );
}

fn write_openmw_cfg(config_dir: &Path, data_dir: &Path) {
    std::fs::write(
        config_dir.join("openmw.cfg"),
        format!(
            "data-local={}\ndata={}\ncontent=MissingContext.esm\n",
            data_dir.display(),
            data_dir.display()
        ),
    )
    .unwrap();
}

fn unclip_args(target: &Path) -> greenmote::unclip::UnclipArgs {
    let cli = Cli::parse_from([
        std::ffi::OsString::from("greenmote"),
        std::ffi::OsString::from("unclip"),
        std::ffi::OsString::from("--plugin"),
        target.as_os_str().to_os_string(),
        std::ffi::OsString::from("--include-grass-id"),
        std::ffi::OsString::from("^flora_"),
        std::ffi::OsString::from("--structured"),
        std::ffi::OsString::from("--write"),
    ]);
    let Command::Unclip(args) = cli.command_or_default() else {
        panic!("expected unclip args");
    };
    args
}

fn write_target_plugin(path: &Path) {
    let mut plugin = Plugin {
        objects: vec![
            TES3Object::Header(Header {
                num_objects: 2,
                ..Header::default()
            }),
            TES3Object::Cell(exterior_cell([((1, 2), reference("crate_01"))])),
        ],
    };
    plugin.save_path(path).unwrap();
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

fn reference(id: &str) -> Reference {
    Reference {
        id: id.to_owned(),
        ..Reference::default()
    }
}
