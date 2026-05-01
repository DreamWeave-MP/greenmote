use std::{
    fs::{File, read_to_string},
    io::{self, Write},
    path::{Path, PathBuf},
};

use regex::RegexSet;
use serde::{Deserialize, Serialize};

use crate::groundcover::{GroundcoverArgs, default};

#[derive(Debug, Serialize, Deserialize)]
// These are persisted/CLI-facing runtime toggles. Hiding them behind enums would make the Rust
// type prettier and the TOML schema worse. That is not a trade.
#[allow(clippy::struct_excessive_bools)]
pub struct GroundcoverConfig {
    #[serde(default = "default::output_directory")]
    pub output_directory: PathBuf,

    #[serde(default = "default::groundcover_output")]
    pub groundcover_output: String,

    #[serde(default = "default::deleted_output")]
    pub deleted_output: String,

    #[serde(default = "default::grass_ids")]
    pub grass_ids: Vec<String>,

    #[serde(default = "default::exclude")]
    pub exclude: Vec<String>,

    #[serde(default = "default::ignored_plugins")]
    pub ignored_plugins: Vec<String>,

    #[serde(default)]
    pub dry_run: bool,

    #[serde(default)]
    pub validate_config: bool,

    #[serde(default)]
    pub debug: bool,

    #[serde(default)]
    pub auto_enable: bool,

    #[serde(skip, default = "RegexSet::empty")]
    include_set: RegexSet,

    #[serde(skip, default = "RegexSet::empty")]
    exclude_set: RegexSet,

    #[serde(skip, default = "RegexSet::empty")]
    ignored_plugin_set: RegexSet,
}

impl Default for GroundcoverConfig {
    fn default() -> Self {
        Self::with_output_directory(default::output_directory())
    }
}

impl GroundcoverConfig {
    fn with_output_directory(output_directory: PathBuf) -> Self {
        Self {
            output_directory,
            groundcover_output: default::groundcover_output(),
            deleted_output: default::deleted_output(),
            grass_ids: default::grass_ids(),
            exclude: default::exclude(),
            ignored_plugins: default::ignored_plugins(),
            dry_run: false,
            validate_config: false,
            debug: false,
            auto_enable: false,
            include_set: RegexSet::empty(),
            exclude_set: RegexSet::empty(),
            ignored_plugin_set: RegexSet::empty(),
        }
    }
}

impl GroundcoverConfig {
    /// Loads, merges, validates, and optionally initializes `greenmote.toml`.
    ///
    /// # Errors
    ///
    /// Returns filesystem errors for config IO, TOML parse errors as invalid data, or regex
    /// compilation errors as invalid input.
    pub fn get(
        args: GroundcoverArgs,
        user_config_path: &Path,
        default_output_directory: PathBuf,
    ) -> io::Result<Self> {
        let config_path = args
            .config
            .clone()
            .unwrap_or_else(|| user_config_path.join(crate::groundcover::DEFAULT_CONFIG_NAME));
        let config_missing = !config_path.is_file();
        let mut config = if config_missing {
            Self::with_output_directory(default_output_directory)
        } else {
            let contents = read_to_string(&config_path)?;
            GroundcoverConfigFile::from_toml(&contents, default_output_directory)?
        };

        config.apply_args(args);
        config.ensure_generated_outputs_are_ignored();
        config.compile_regex_sets()?;

        if config_missing && !config.dry_run && !config.validate_config {
            config.save_to(&config_path)?;
        }

        Ok(config)
    }

    fn apply_args(&mut self, mut args: GroundcoverArgs) {
        if let Some(output) = args.output.take() {
            self.output_directory = output;
        }
        self.ignored_plugins.append(&mut args.ignored_plugins);

        if let Some(dry_run) = args.dry_run {
            self.dry_run = dry_run;
            if dry_run {
                self.validate_config = false;
            }
        }

        if let Some(validate_config) = args.validate_config {
            self.validate_config = validate_config;
            if validate_config {
                self.dry_run = false;
            }
        }

        self.auto_enable |= args.auto_enable;
        self.debug |= args.debug;
    }

    fn ensure_generated_outputs_are_ignored(&mut self) {
        if !self
            .ignored_plugins
            .iter()
            .any(|plugin| plugin == &self.deleted_output)
        {
            self.ignored_plugins.push(self.deleted_output.clone());
        }
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        let contents = toml::to_string_pretty(&GroundcoverConfigFile::from_runtime(self))
            .map_err(to_io_error)?;
        let mut file = File::create(path)?;
        file.write_all(contents.as_bytes())
    }

    /// Compiles the include, exclude, and ignored-plugin regex sets used during conversion.
    ///
    /// # Errors
    ///
    /// Returns an invalid-data error if any configured regex is malformed.
    pub fn compile_regex_sets(&mut self) -> io::Result<()> {
        self.include_set = RegexSet::new(&self.grass_ids).map_err(to_io_error)?;
        self.exclude_set = RegexSet::new(&self.exclude).map_err(to_io_error)?;
        self.ignored_plugin_set = RegexSet::new(&self.ignored_plugins).map_err(to_io_error)?;

        Ok(())
    }

    #[must_use]
    pub fn matches_static_id(&self, id: &str) -> bool {
        self.include_set.is_match(id) && !self.exclude_set.is_match(id)
    }

    #[must_use]
    pub fn is_ignored_plugin_name(&self, plugin_name: &str) -> bool {
        self.ignored_plugin_set.is_match(plugin_name)
    }
}

#[derive(Debug, Deserialize, Serialize)]
// Mirrors the public TOML schema so we can distinguish an omitted output directory from one the user
// intentionally set. Convert-specific knobs live under `[convert]`; root-level command knobs are
// not supported because this tool is still wet paint, not a museum.
struct GroundcoverConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_directory: Option<PathBuf>,

    #[serde(default)]
    convert: ConvertConfigFile,
}

#[derive(Debug, Default, Deserialize, Serialize)]
// These are persisted/CLI-facing runtime toggles. Hiding them behind enums would make the Rust
// type prettier and the TOML schema worse. That is not a trade.
#[allow(clippy::struct_excessive_bools)]
struct ConvertConfigFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groundcover_output: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    deleted_output: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    grass_ids: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    exclude: Option<Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    ignored_plugins: Option<Vec<String>>,

    #[serde(default)]
    dry_run: bool,

    #[serde(default)]
    validate_config: bool,

    #[serde(default)]
    debug: bool,

    #[serde(default)]
    auto_enable: bool,
}

impl GroundcoverConfigFile {
    fn from_runtime(config: &GroundcoverConfig) -> Self {
        Self {
            output_directory: Some(config.output_directory.clone()),
            convert: ConvertConfigFile {
                groundcover_output: Some(config.groundcover_output.clone()),
                deleted_output: Some(config.deleted_output.clone()),
                grass_ids: Some(config.grass_ids.clone()),
                exclude: Some(config.exclude.clone()),
                ignored_plugins: Some(config.ignored_plugins.clone()),
                dry_run: config.dry_run,
                validate_config: config.validate_config,
                debug: config.debug,
                auto_enable: config.auto_enable,
            },
        }
    }

    fn from_toml(
        contents: &str,
        default_output_directory: PathBuf,
    ) -> io::Result<GroundcoverConfig> {
        let file = toml::from_str::<Self>(contents).map_err(to_io_error)?;
        let convert = file.convert;

        Ok(GroundcoverConfig {
            output_directory: resolved_output_directory(
                file.output_directory,
                default_output_directory,
            ),
            groundcover_output: convert
                .groundcover_output
                .unwrap_or_else(default::groundcover_output),
            deleted_output: convert
                .deleted_output
                .unwrap_or_else(default::deleted_output),
            grass_ids: convert.grass_ids.unwrap_or_else(default::grass_ids),
            exclude: convert.exclude.unwrap_or_else(default::exclude),
            ignored_plugins: convert
                .ignored_plugins
                .unwrap_or_else(default::ignored_plugins),
            dry_run: convert.dry_run,
            validate_config: convert.validate_config,
            debug: convert.debug,
            auto_enable: convert.auto_enable,
            include_set: RegexSet::empty(),
            exclude_set: RegexSet::empty(),
            ignored_plugin_set: RegexSet::empty(),
        })
    }
}

fn resolved_output_directory(
    configured_output_directory: Option<PathBuf>,
    default_output_directory: PathBuf,
) -> PathBuf {
    let Some(configured_output_directory) = configured_output_directory else {
        return default_output_directory;
    };

    // Early versions of greenmote wrote the platform default data-local path into generated TOML.
    // Treat that exact value as a generated default, not a user override, so profile-specific
    // `data-local=` keeps owning the output directory.
    if paths_equal(
        &configured_output_directory,
        &openmw_config::default_data_local_path(),
    ) && !paths_equal(&configured_output_directory, &default_output_directory)
    {
        default_output_directory
    } else {
        configured_output_directory
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn to_io_error<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err.to_string())
}

#[cfg(test)]
mod tests {
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
            let _ = std::fs::remove_file(path_join(
                &self.path,
                crate::groundcover::DEFAULT_CONFIG_NAME,
            ));
            let _ = std::fs::remove_dir(&self.path);
        }
    }

    fn path_join(path: &Path, child: &str) -> PathBuf {
        path.join(child)
    }

    #[test]
    fn missing_config_default_initializes_next_to_user_config() {
        let dir = TempDir::new();
        let args = GroundcoverArgs::parse_from(["convert"]);
        let default_output_directory = dir.path.join("data-local");

        let config =
            GroundcoverConfig::get(args, &dir.path, default_output_directory.clone()).unwrap();

        assert_eq!(config.output_directory, default_output_directory);
        assert!(config.matches_static_id("flora_grass_01"));
        assert!(!config.matches_static_id("ab_furn_impplantergrass"));
        assert!(
            dir.path
                .join(crate::groundcover::DEFAULT_CONFIG_NAME)
                .is_file()
        );
        assert!(
            read_to_string(dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME))
                .unwrap()
                .contains("[convert]")
        );
    }

    #[test]
    fn dry_run_does_not_default_initialize_config_file() {
        let dir = TempDir::new();
        let args = GroundcoverArgs::parse_from(["convert", "--dry-run"]);
        let default_output_directory = dir.path.join("data-local");

        let config =
            GroundcoverConfig::get(args, &dir.path, default_output_directory.clone()).unwrap();

        assert!(config.dry_run);
        assert_eq!(config.output_directory, default_output_directory);
        assert!(
            !dir.path
                .join(crate::groundcover::DEFAULT_CONFIG_NAME)
                .exists()
        );
    }

    #[test]
    fn cli_values_merge_over_toml() {
        let dir = TempDir::new();
        let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
        std::fs::write(
            &config_path,
            r#"
[convert]
groundcover_output = "gc.omwaddon"
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

        assert_eq!(config.groundcover_output, "gc.omwaddon");
        assert_eq!(config.output_directory, PathBuf::from("out"));
        assert!(config.dry_run);
        assert!(config.is_ignored_plugin_name("Generated.omwaddon"));
        assert!(config.is_ignored_plugin_name("OtherGenerated.omwaddon"));
        assert!(read_to_string(config_path).unwrap().contains("gc.omwaddon"));
    }

    #[test]
    fn missing_toml_output_directory_uses_effective_data_local() {
        let dir = TempDir::new();
        let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
        std::fs::write(
            &config_path,
            r#"
[convert]
groundcover_output = "gc.omwaddon"
"#,
        )
        .unwrap();
        let args = GroundcoverArgs::parse_from(["convert"]);
        let default_output_directory = dir.path.join("profile-data-local");

        let config =
            GroundcoverConfig::get(args, &dir.path, default_output_directory.clone()).unwrap();

        assert_eq!(config.output_directory, default_output_directory);
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
    fn configured_deleted_output_is_ignored_automatically() {
        let dir = TempDir::new();
        let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
        std::fs::write(
            &config_path,
            r#"
[convert]
deleted_output = "my_deleted_groundcover.omwaddon"
"#,
        )
        .unwrap();
        let args = GroundcoverArgs::parse_from(["convert"]);

        let config = GroundcoverConfig::get(args, &dir.path, dir.path.join("data-local")).unwrap();

        assert!(config.is_ignored_plugin_name("my_deleted_groundcover.omwaddon"));
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

        assert_eq!(config.groundcover_output, default::groundcover_output());
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
}
