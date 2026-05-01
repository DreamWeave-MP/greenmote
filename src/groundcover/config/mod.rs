mod file;

#[cfg(test)]
mod tests;

use std::{
    fs::{File, read_to_string},
    io::{self, Write},
    path::{Path, PathBuf},
};

use regex::RegexSet;
use serde::{Deserialize, Serialize};

use crate::groundcover::{GroundcoverArgs, default};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    pub(super) fn with_output_directory(output_directory: PathBuf) -> Self {
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
    /// Loads `greenmote.toml` for GUI editing without applying transient CLI overrides or writing
    /// a generated file as a side effect.
    ///
    /// # Errors
    ///
    /// Returns filesystem errors, TOML parse errors as invalid data, or regex compilation errors as
    /// invalid input.
    pub(crate) fn load_for_edit(
        config_path: &Path,
        default_output_directory: PathBuf,
    ) -> io::Result<Self> {
        let config = if config_path.is_file() {
            let contents = read_to_string(config_path)?;
            file::GroundcoverConfigFile::from_toml(&contents, default_output_directory)?
        } else {
            Self::with_output_directory(default_output_directory)
        };

        Ok(config)
    }

    /// Saves `greenmote.toml` after validating regex-backed settings.
    ///
    /// # Errors
    ///
    /// Returns an invalid-data error for malformed regex settings or filesystem errors while
    /// writing the TOML file.
    pub(crate) fn save_for_edit(&self, path: &Path) -> io::Result<Self> {
        let mut normalized = self.clone();
        normalized.ensure_generated_outputs_are_ignored();
        normalized.compile_regex_sets()?;
        normalized.save_to(path)?;
        Ok(normalized)
    }

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
            file::GroundcoverConfigFile::from_toml(&contents, default_output_directory)?
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
        let contents = toml::to_string_pretty(&file::GroundcoverConfigFile::from_runtime(self))
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

pub(super) fn to_io_error<E: std::fmt::Display>(err: E) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err.to_string())
}
