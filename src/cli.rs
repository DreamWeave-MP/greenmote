use clap::{Parser, Subcommand};

use crate::groundcover::GroundcoverArgs;

#[derive(Parser, Debug)]
#[command(name = "greenmote", author, version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Convert vanilla-style static exterior refs into OpenMW groundcover.
    Convert(GroundcoverArgs),
}
