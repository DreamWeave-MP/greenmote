use std::io;

use clap::Parser;

mod cli;
pub mod groundcover;

pub use cli::{Cli, Command};

pub fn run() -> io::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Convert(args) => groundcover::run(args),
    }
}
