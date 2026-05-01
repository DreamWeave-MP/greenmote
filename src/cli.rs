use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::groundcover::GroundcoverArgs;

#[derive(Parser, Debug)]
#[command(name = "greenmote", author, version)]
pub struct Cli {
    /// Generate shell completion script to stdout for the whole application.
    #[arg(long, value_name = "SHELL", conflicts_with = "generate_manpage")]
    pub generate_completion: Option<Shell>,

    /// Generate roff manpage to stdout for the whole application.
    #[arg(long, conflicts_with = "generate_completion")]
    pub generate_manpage: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Convert vanilla-style static exterior refs into `OpenMW` groundcover.
    Convert(GroundcoverArgs),
}

impl Cli {
    #[must_use]
    pub fn command_or_default(self) -> Command {
        self.command
            .unwrap_or_else(|| Command::Convert(GroundcoverArgs::default()))
    }

    /// Writes generated top-level CLI output when requested.
    ///
    /// # Errors
    ///
    /// Returns manpage rendering errors from the output writer.
    pub fn handle_generated_output(
        &self,
        stdout: &mut dyn std::io::Write,
    ) -> std::io::Result<bool> {
        if let Some(shell) = self.generate_completion {
            let mut command = Self::command();
            clap_complete::generate(shell, &mut command, "greenmote", stdout);
            return Ok(true);
        }

        if self.generate_manpage {
            clap_mangen::Man::new(Self::command()).render(stdout)?;
            return Ok(true);
        }

        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::*;

    #[test]
    fn generated_output_flags_are_top_level() {
        let cli = Cli::parse_from(["greenmote", "--generate-manpage"]);

        assert!(cli.generate_manpage);
        assert!(cli.command.is_none());
    }

    #[test]
    fn convert_does_not_accept_generated_output_flags() {
        let result =
            Cli::command().try_get_matches_from(["greenmote", "convert", "--generate-manpage"]);

        assert!(result.is_err());
    }

    #[test]
    fn no_subcommand_defaults_to_convert() {
        let cli = Cli::parse_from(["greenmote"]);

        let Command::Convert(args) = cli.command_or_default();

        assert!(args.openmw_cfg.is_none());
        assert!(args.config.is_none());
        assert!(args.output.is_none());
        assert!(args.ignored_plugins.is_empty());
        assert_eq!(args.dry_run, None);
        assert_eq!(args.validate_config, None);
        assert!(!args.auto_enable);
        assert!(!args.debug);
    }
}
