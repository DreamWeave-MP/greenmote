use std::path::PathBuf;

use clap::Parser;

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

    /// Include per-reference instance diagnostics.
    #[arg(long = "instances")]
    pub instances: bool,

    /// Emit machine-readable compact JSON. With --instances, emits newline-delimited JSON records.
    #[arg(long = "structured")]
    pub structured: bool,
}
