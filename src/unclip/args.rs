use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Parser, Clone, Debug)]
#[command(
    name = "unclip",
    about = "Inspect groundcover refs against OpenMW terrain before unclipping."
)]
pub struct UnclipArgs {
    /// Path to openmw.cfg, or a directory containing openmw.cfg.
    #[arg(short = 'c', long = "openmw-cfg")]
    pub openmw_cfg: Option<PathBuf>,

    /// Groundcover plugin to inspect. May be a filesystem path or a VFS plugin name.
    #[arg(short = 'p', long = "plugin", value_name = "PLUGIN")]
    pub plugin: PathBuf,

    /// Include human-readable per-reference diagnostic strings in structured output.
    #[arg(long = "verbose")]
    pub verbose: bool,

    /// Report output format.
    #[arg(long = "format", value_enum, default_value_t = UnclipOutputFormat::Yaml)]
    pub format: UnclipOutputFormat,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum UnclipOutputFormat {
    /// YAML report.
    #[default]
    Yaml,
    /// JSON report.
    Json,
    /// TOML report.
    Toml,
}

impl UnclipOutputFormat {
    #[must_use]
    pub const fn serialize_type(self) -> vfstool_lib::SerializeType {
        match self {
            Self::Json => vfstool_lib::SerializeType::Json,
            Self::Yaml => vfstool_lib::SerializeType::Yaml,
            Self::Toml => vfstool_lib::SerializeType::Toml,
        }
    }
}
