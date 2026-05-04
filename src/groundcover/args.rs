use std::path::PathBuf;

use clap::Parser;

#[derive(Parser, Clone, Debug, Default)]
#[command(
    name = "convert",
    about = "Convert vanilla-style static exterior refs into OpenMW groundcover."
)]
pub struct GroundcoverArgs {
    /// Path to `greenmote.toml`. Defaults to the `OpenMW` user config directory.
    #[arg(long = "config")]
    pub config: Option<PathBuf>,

    /// Output directory for generated plugins and copied meshes.
    #[arg(short = 'o', long = "output")]
    pub output: Option<PathBuf>,

    /// Ignore plugins whose file names match these regexes. Merged with TOML config.
    #[arg(long = "ignore", value_delimiter = ',')]
    pub ignored_plugins: Vec<String>,

    /// Build and print the conversion plan without writing files.
    #[arg(
        long = "dry-run",
        conflicts_with = "validate_config",
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    pub dry_run: Option<bool>,

    /// Validate config and regexes without loading plugins or writing files.
    #[arg(
        long = "validate-config",
        conflicts_with = "dry_run",
        num_args = 0..=1,
        default_missing_value = "true",
        value_name = "BOOL"
    )]
    pub validate_config: Option<bool>,

    /// Automatically add generated plugins to openmw.cfg.
    #[arg(short = 'e', long = "auto-enable")]
    pub auto_enable: bool,

    /// Print extra conversion diagnostics.
    #[arg(short = 'd', long = "debug")]
    pub debug: bool,
}
