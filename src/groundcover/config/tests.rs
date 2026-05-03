use std::{
    fs::{create_dir, read_to_string},
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

#[test]
fn missing_config_default_initializes_next_to_user_config() {
    let dir = TempDir::new();
    let args = GroundcoverArgs::parse_from(["convert"]);
    let default_output_directory = dir.path.join("data-local");

    let config = GroundcoverConfig::get(args, &dir.path, default_output_directory.clone()).unwrap();

    assert_eq!(config.output_directory, default_output_directory);
    assert!(config.matches_static_id("flora_grass_01"));
    assert!(!config.matches_static_id("ab_furn_impplantergrass"));
    assert!(
        dir.path
            .join(crate::groundcover::DEFAULT_CONFIG_NAME)
            .is_file()
    );
    let contents = read_to_string(dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME)).unwrap();
    assert!(contents.contains("[convert]"));
    assert!(!contents.contains("validate_config"));
}

#[test]
fn dry_run_does_not_default_initialize_config_file() {
    let dir = TempDir::new();
    let args = GroundcoverArgs::parse_from(["convert", "--dry-run"]);
    let default_output_directory = dir.path.join("data-local");

    let config = GroundcoverConfig::get(args, &dir.path, default_output_directory.clone()).unwrap();

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

    let result = GroundcoverConfig::get(args, &dir.path, default_output_directory);

    assert!(result.is_err());
    assert!(
        !dir.path
            .join(crate::groundcover::DEFAULT_CONFIG_NAME)
            .exists()
    );
}

#[test]
fn persisted_validate_config_is_rejected() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        r"
[convert]
validate_config = true
",
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);

    let result = GroundcoverConfig::get(args, &dir.path, dir.path.join("data-local"));

    assert!(result.is_err());
}

#[test]
fn root_validate_config_is_rejected() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(&config_path, "validate_config = true\n").unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);

    let result = GroundcoverConfig::get(args, &dir.path, dir.path.join("data-local"));

    assert!(result.is_err());
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

    let config = GroundcoverConfig::get(args, &dir.path, default_output_directory).unwrap();

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

    let config = GroundcoverConfig::get(args, &dir.path, default_output_directory.clone()).unwrap();

    assert_eq!(config.output_directory, default_output_directory);
}

#[test]
fn convert_output_names_are_rejected() {
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

    let result = GroundcoverConfig::get(
        GroundcoverArgs::parse_from(["convert"]),
        &dir.path,
        dir.path.join("data-local"),
    );

    assert!(result.is_err());
}

#[test]
fn stale_generated_platform_data_local_yields_to_effective_data_local() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    std::fs::write(
        &config_path,
        format!(
            "output_directory = {:?}\n",
            openmw_config::default_data_local_path()
                .display()
                .to_string()
        ),
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);
    let effective_data_local = dir.path.join("Morrowind").join("overwrite");

    let config = GroundcoverConfig::get(args, &dir.path, effective_data_local.clone()).unwrap();

    assert_eq!(config.output_directory, effective_data_local);
}

#[test]
fn non_default_toml_output_directory_remains_user_override() {
    let dir = TempDir::new();
    let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
    let custom_output = dir.path.join("custom-output");
    std::fs::write(
        &config_path,
        format!(
            "output_directory = {:?}\n",
            custom_output.display().to_string()
        ),
    )
    .unwrap();
    let args = GroundcoverArgs::parse_from(["convert"]);
    let effective_data_local = dir.path.join("Morrowind").join("overwrite");

    let config = GroundcoverConfig::get(args, &dir.path, effective_data_local).unwrap();

    assert_eq!(config.output_directory, custom_output);
}

#[test]
fn generated_outputs_are_not_name_ignored_by_default() {
    let mut config = GroundcoverConfig::default();

    config.compile_regex_sets().unwrap();

    assert!(!config.is_ignored_plugin_name(crate::groundcover::GROUNDCOVER_PLUGIN_NAME));
    assert!(!config.is_ignored_plugin_name(crate::groundcover::DELETED_PLUGIN_NAME));
}

#[test]
fn root_convert_keys_are_not_part_of_the_schema() {
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

    let config = GroundcoverConfig::get(args, &dir.path, dir.path.join("data-local")).unwrap();

    assert_eq!(
        crate::groundcover::GROUNDCOVER_PLUGIN_NAME,
        "groundcover.omwaddon"
    );
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

    let result = GroundcoverConfig::load_for_edit(&config_path, dir.path.join("data-local"));

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
