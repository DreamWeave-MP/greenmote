use std::{
    fs::{File, read_to_string},
    io::{self, Write},
    path::{Path, PathBuf},
};

use regex::RegexSet;
use serde::{Deserialize, Serialize};

use crate::groundcover::{GroundcoverArgs, default};

#[derive(Debug, Serialize, Deserialize)]
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
        Self {
            output_directory: default::output_directory(),
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
    /// Loads, merges, validates, and optionally initializes `groundcoverify.toml`.
    ///
    /// # Errors
    ///
    /// Returns filesystem errors for config IO, TOML parse errors as invalid data, or regex
    /// compilation errors as invalid input.
    pub fn get(args: GroundcoverArgs, user_config_path: &Path) -> io::Result<Self> {
        let config_path = args
            .config
            .clone()
            .unwrap_or_else(|| user_config_path.join(crate::groundcover::DEFAULT_CONFIG_NAME));
        let config_missing = !config_path.is_file();
        let mut config = if config_missing {
            Self::default()
        } else {
            let contents = read_to_string(&config_path)?;
            toml::from_str(&contents).map_err(to_io_error)?
        };

        config.apply_args(args);
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

    fn save_to(&self, path: &Path) -> io::Result<()> {
        let contents = toml::to_string_pretty(self).map_err(to_io_error)?;
        let mut file = File::create(path)?;
        file.write_all(contents.as_bytes())
    }

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
            let _ = std::fs::remove_file(path_join(&self.path, crate::groundcover::DEFAULT_CONFIG_NAME));
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

        let config = GroundcoverConfig::get(args, &dir.path).unwrap();

        assert!(config.matches_static_id("flora_grass_01"));
        assert!(!config.matches_static_id("ab_furn_impplantergrass"));
        assert!(dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME).is_file());
    }

    #[test]
    fn dry_run_does_not_default_initialize_config_file() {
        let dir = TempDir::new();
        let args = GroundcoverArgs::parse_from(["convert", "--dry-run"]);

        let config = GroundcoverConfig::get(args, &dir.path).unwrap();

        assert!(config.dry_run);
        assert!(!dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME).exists());
    }

    #[test]
    fn cli_values_merge_over_toml() {
        let dir = TempDir::new();
        let config_path = dir.path.join(crate::groundcover::DEFAULT_CONFIG_NAME);
        std::fs::write(
            &config_path,
            r#"
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

        let config = GroundcoverConfig::get(args, &dir.path).unwrap();

        assert_eq!(config.groundcover_output, "gc.omwaddon");
        assert_eq!(config.output_directory, PathBuf::from("out"));
        assert!(config.dry_run);
        assert!(config.is_ignored_plugin_name("Generated.omwaddon"));
        assert!(config.is_ignored_plugin_name("OtherGenerated.omwaddon"));
        assert!(read_to_string(config_path).unwrap().contains("gc.omwaddon"));
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
