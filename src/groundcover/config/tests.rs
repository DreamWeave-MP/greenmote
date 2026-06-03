// SPDX-License-Identifier: GPL-3.0-only

use std::{
    fs::{create_dir, read_to_string},
    io,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::Parser;

use super::*;

static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "greenmote-config-test-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        create_dir(&path).unwrap();

        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn default_config_path(dir: &TempDir) -> PathBuf {
    dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME)
}

fn get_config(
    args: GroundcoverArgs,
    dir: &TempDir,
    output_directory: PathBuf,
) -> io::Result<GroundcoverConfig> {
    GroundcoverConfig::get(
        args,
        &default_config_path(dir),
        resolved_output_directory(output_directory),
        Some(dir.path.join("openmw.cfg")),
    )
}

fn resolved_output_directory(path: PathBuf) -> crate::groundcover::openmw::ConvertOutputDirectory {
    crate::groundcover::openmw::ConvertOutputDirectory {
        path,
        source: crate::groundcover::openmw::ConvertOutputDirectorySource::OpenMwDataLocal,
    }
}

fn working_directory_output(path: PathBuf) -> crate::groundcover::openmw::ConvertOutputDirectory {
    crate::groundcover::openmw::ConvertOutputDirectory {
        path,
        source: crate::groundcover::openmw::ConvertOutputDirectorySource::WorkingDirectoryFallback,
    }
}

#[test]
fn missing_config_default_initializes_next_to_user_config() {
    let dir = TempDir::new();
    let args = GroundcoverArgs::parse_from(["convert"]);
    let default_output_directory = dir.path.join("data-local");

    let config = get_config(args, &dir, default_output_directory.clone()).unwrap();

    assert_eq!(config.output_directory, default_output_directory);
    assert!(config.matches_static_id("flora_grass_01"));
    assert!(!config.matches_static_id("ab_furn_impplantergrass"));
    assert!(default_config_path(&dir).is_file());
    let contents = read_to_string(default_config_path(&dir)).unwrap();
    assert!(contents.contains("[convert]"));
    assert!(contents.contains("[unclip]"));
    assert!(!contents.contains("openmw_cfg"));
    assert!(!contents.contains("plugin ="));
    assert!(!contents.contains("dry_run"));
    assert!(!contents.contains("write = false"));
    assert!(!contents.contains("validate_config"));
    assert!(!contents.contains("fallback_output_directory"));
    assert!(
        !contents
            .lines()
            .any(|line| line.starts_with("output_directory ="))
    );
}

#[test]
fn save_for_edit_preserves_unclip_config() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[unclip]
plugin = "custom-groundcover.omwaddon"
dry_run = true
include_grass_ids = ["flora_.*"]
"#,
    )
    .unwrap();

    let mut config = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();
    config.dry_run = true;
    config.save_for_edit(&config_path).unwrap();
    let contents = read_to_string(config_path).unwrap();

    assert!(contents.contains("plugin = \"custom-groundcover.omwaddon\""));
    assert!(!contents.contains("dry_run"));
    assert!(!contents.contains("write = true"));
    assert!(contents.contains("include_grass_ids = [\"flora_.*\"]"));
    assert!(contents.contains("road_texture_paths = ["));
    assert!(!contents.contains("road_texture_paths = []"));
}

#[test]
fn save_for_edit_returns_persistent_convert_dry_run_state() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    let mut config = GroundcoverConfig::with_output_directory(dir.path.join("data-local"));
    config.dry_run = true;

    let saved = config.save_for_edit_new(&config_path).unwrap();
    let contents = read_to_string(config_path).unwrap();

    assert!(!saved.dry_run);
    assert!(!contents.contains("dry_run"));
}

#[test]
fn load_for_edit_defaults_missing_unclip_road_texture_paths() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[unclip]
plugin = "x.omwaddon"
"#,
    )
    .unwrap();

    let config = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();

    assert_eq!(
        config.unclip.road_texture_paths,
        crate::unclip::config::PersistedUnclipConfig::generated_default().road_texture_paths
    );
}

#[test]
fn load_for_edit_creates_missing_config_with_defaults() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);

    let config = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();
    let contents = read_to_string(config_path).unwrap();

    assert_eq!(
        config.unclip.road_texture_paths,
        crate::unclip::config::PersistedUnclipConfig::generated_default().road_texture_paths
    );
    assert!(contents.contains("road_texture_paths = ["));
    assert!(!contents.contains("include_road_texture_paths"));
    assert!(!contents.contains("exclude_road_texture_paths"));
}

#[test]
fn load_for_edit_ignores_legacy_unclip_write() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[unclip]
plugin = "custom-groundcover.omwaddon"
write = false
"#,
    )
    .unwrap();

    let config = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();

    assert_eq!(config.unclip.legacy_write, None);
}

#[test]
fn load_for_edit_ignores_conflicting_stale_unclip_write_and_dry_run() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r"
[unclip]
write = true
dry_run = true
",
    )
    .unwrap();

    let config = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();

    assert_eq!(config.unclip.legacy_write, None);
}

#[test]
fn load_save_removes_stale_unclip_runtime_keys() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[unclip]
plugin = "custom-groundcover.omwaddon"
meshgenerator_ini = "meshgenerator.ini"
verbose = true
structured = true
dry_run = true
instances = true
"#,
    )
    .unwrap();

    let config = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();
    config.save_for_edit(&config_path).unwrap();
    let contents = read_to_string(config_path).unwrap();

    assert!(!contents.contains("meshgenerator_ini"));
    assert!(!contents.contains("verbose"));
    assert!(!contents.contains("structured"));
    assert!(!contents.contains("dry_run"));
    assert!(!contents.contains("instances"));
}

#[test]
fn runtime_openmw_cfg_is_not_persisted_to_greenmote_toml() {
    let dir = TempDir::new();
    let config_path = default_config_path(&dir);
    let openmw_cfg = dir.path.join("profile").join("openmw.cfg");
    let mut config = GroundcoverConfig::with_output_directory(dir.path.join("data-local"));
    config.openmw_cfg = Some(openmw_cfg.clone());

    config.save_for_edit_new(&config_path).unwrap();
    let loaded = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("fallback-data-local")),
        None,
    )
    .unwrap();
    let contents = read_to_string(config_path).unwrap();

    assert_eq!(loaded.openmw_cfg, None);
    assert!(!contents.contains("openmw_cfg"));
    assert!(contents.contains("[convert]"));
    assert!(contents.contains("[unclip]"));
}

#[test]
fn dry_run_does_not_default_initialize_config_file() {
    let dir = TempDir::new();
    let args = GroundcoverArgs::parse_from(["convert", "--dry-run"]);
    let default_output_directory = dir.path.join("data-local");

    let config = get_config(args, &dir, default_output_directory.clone()).unwrap();

    assert!(config.dry_run);
    assert_eq!(config.output_directory, default_output_directory);
    assert!(
        !dir.path
            .join(crate::groundcover::DEFAULT_CONFIG_NAME)
            .exists()
    );
}

#[test]
fn validate_config_is_cli_only() {
    let dir = TempDir::new();
    let args = GroundcoverArgs::parse_from(["convert", "--validate-config"]);
    let default_output_directory = dir.path.join("data-local");

    let result = get_config(args, &dir, default_output_directory);

    assert!(result.is_err());
    assert!(
        !dir.path
            .join(crate::groundcover::DEFAULT_CONFIG_NAME)
            .exists()
    );
}

#[test]
fn stale_unknown_convert_fields_are_ignored() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
validate_config = true

[convert]
validate_config = true
old_option = "unused"
dry_run = true

[unclip]
contact_epsilon = 99.0
old_option = "unused"
"#,
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);

    let config = get_config(args, &dir, dir.path.join("data-local")).unwrap();

    assert!(!config.dry_run);
    assert!(!config.validate_config);
}

#[test]
fn cli_values_merge_over_toml() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[convert]
ignored_plugins = ["Generated"]
dry_run = false
"#,
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from([
        "convert",
        "--dry-run",
        "--ignore",
        "OtherGenerated",
        "--output",
        "out",
    ]);
    let default_output_directory = dir.path.join("data-local");

    let config = get_config(args, &dir, default_output_directory).unwrap();

    assert_eq!(config.output_directory, PathBuf::from("out"));
    assert!(config.dry_run);
    assert!(config.is_ignored_plugin_name("Generated.omwaddon"));
    assert!(config.is_ignored_plugin_name("OtherGenerated.omwaddon"));
}

#[test]
fn missing_toml_output_directory_uses_effective_data_local() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[convert]
ignored_plugins = ["Generated"]
"#,
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);
    let default_output_directory = dir.path.join("profile-data-local");

    let config = get_config(args, &dir, default_output_directory.clone()).unwrap();

    assert_eq!(config.output_directory, default_output_directory);
}

#[test]
fn stale_convert_output_names_are_ignored() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[convert]
groundcover_output = "gc.omwaddon"
deleted_output = "deleted_gc.omwaddon"
"#,
    )
    .unwrap();

    let config = GroundcoverConfig::get(
        GroundcoverArgs::parse_from(["convert"]),
        &default_config_path(&dir),
        resolved_output_directory(dir.path.join("data-local")),
        None,
    )
    .unwrap();

    assert_eq!(config.output_directory, dir.path.join("data-local"));
}

#[test]
fn working_directory_output_is_used_when_openmw_has_no_data_local() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(&config_path, "[convert]\n").unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);
    let working_directory = dir.path.join("working-directory");

    let config = GroundcoverConfig::get(
        args,
        &config_path,
        working_directory_output(working_directory.clone()),
        Some(dir.path.join("openmw.cfg")),
    )
    .unwrap();

    assert_eq!(config.output_directory, working_directory);
    assert_eq!(
        config.output_directory_source,
        crate::groundcover::openmw::ConvertOutputDirectorySource::WorkingDirectoryFallback
    );
}

#[test]
fn generated_outputs_are_not_name_ignored_by_default() {
    let mut config = GroundcoverConfig::default();

    config.compile_regex_sets().unwrap();

    assert!(!config.is_ignored_plugin_name(crate::groundcover::GROUNDCOVER_PLUGIN_NAME));
    assert!(!config.is_ignored_plugin_name(crate::groundcover::DELETED_PLUGIN_NAME));
}

#[test]
fn stale_root_convert_keys_are_ignored() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
groundcover_output = "legacy_gc.omwaddon"
ignored_plugins = ["LegacyGenerated"]
dry_run = true
"#,
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);

    let config = get_config(args, &dir, dir.path.join("data-local")).unwrap();

    assert!(!config.dry_run);
    assert!(!config.is_ignored_plugin_name("LegacyGenerated.omwaddon"));
}

#[test]
fn invalid_regex_fails_validation() {
    let mut config = GroundcoverConfig {
        grass_ids: vec!["[".to_owned()],
        ..GroundcoverConfig::default()
    };

    assert!(config.compile_regex_sets().is_err());
}

#[test]
fn convert_match_regexes_are_case_insensitive() {
    let mut config = GroundcoverConfig {
        grass_ids: vec!["FERN".to_owned()],
        exclude: vec!["PLANTER".to_owned()],
        ignored_plugins: vec!["GENERATED".to_owned()],
        ..GroundcoverConfig::default()
    };
    config.compile_regex_sets().unwrap();

    assert!(config.matches_static_id("flora_fern_01"));
    assert!(!config.matches_static_id("flora_fern_planter"));
    assert!(config.is_ignored_plugin_name("my-generated-plugin.esp"));
}

#[test]
fn convert_match_patterns_do_not_apply_to_static_mesh_paths() {
    let mut config = GroundcoverConfig {
        grass_ids: vec!["grass".to_owned()],
        exclude: vec!["grassplane".to_owned()],
        ..GroundcoverConfig::default()
    };
    config.compile_regex_sets().unwrap();

    assert!(!config.matches_static_id("sky_flora_gs_01_01"));
    assert!(config.matches_static_id("grassblade"));
}

#[test]
fn load_for_edit_validates_regexes() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r#"
[convert]
grass_ids = ["["]
"#,
    )
    .unwrap();

    let result = GroundcoverConfig::load_for_edit(
        &config_path,
        resolved_output_directory(dir.path.join("data-local")),
        None,
    );

    assert!(result.is_err());
}

#[test]
fn replacing_config_backs_up_existing_file() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(&config_path, "definitely not toml").unwrap();
    let temp_path = config_path.with_extension("toml.tmp");
    let config = GroundcoverConfig::with_output_directory(dir.path.join("data-local"));

    config.save_for_edit_new(&temp_path).unwrap();
    super::edit::replace_config_with_backup(&config_path, &temp_path).unwrap();

    assert_eq!(
        read_to_string(config_path.with_extension("toml.bak")).unwrap(),
        "definitely not toml"
    );
    assert!(read_to_string(config_path).unwrap().contains("[convert]"));
}

#[test]
fn replacing_config_rejects_directory_as_backup_source_and_keeps_temp() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    let temp_path = config_path.with_extension("toml.tmp");
    std::fs::create_dir(&config_path).unwrap();
    std::fs::write(&temp_path, "[convert]\n").unwrap();

    let result = super::edit::replace_config_with_backup(&config_path, &temp_path);

    assert!(result.is_err());
    assert!(config_path.is_dir());
    assert!(temp_path.is_file());
}

#[cfg(unix)]
#[test]
fn replacing_config_backs_up_dangling_config_symlink() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    let temp_path = config_path.with_extension("toml.tmp");
    std::os::unix::fs::symlink(dir.path.join("missing.toml"), &config_path).unwrap();
    std::fs::write(&temp_path, "[convert]\n").unwrap();

    let result = super::edit::replace_config_with_backup(&config_path, &temp_path);

    assert!(result.is_ok());
    assert!(read_to_string(&config_path).unwrap().contains("[convert]"));
    assert!(
        std::fs::symlink_metadata(config_path.with_extension("toml.bak"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[cfg(unix)]
#[test]
fn backup_selection_skips_dangling_symlink() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    let backup_path = config_path.with_extension("toml.bak");
    let next_backup_path = config_path.with_extension("toml.bak.1");
    std::fs::write(&config_path, "definitely not toml").unwrap();
    std::os::unix::fs::symlink(dir.path.join("missing-backup.toml"), &backup_path).unwrap();

    super::edit::back_up_existing_config(&config_path).unwrap();

    assert!(
        std::fs::symlink_metadata(&backup_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        read_to_string(next_backup_path).unwrap(),
        "definitely not toml"
    );
}
