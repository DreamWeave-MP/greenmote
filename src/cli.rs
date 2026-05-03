use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::{groundcover::GroundcoverArgs, unclip::UnclipArgs};

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
    /// Find and optionally fix groundcover refs clipped into terrain or statics.
    Unclip(UnclipArgs),
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
    fn parser_no_subcommand_defaults_to_convert() {
        let cli = Cli::parse_from(["greenmote"]);

        let Command::Convert(args) = cli.command_or_default() else {
            panic!("no subcommand should default to convert");
        };

        assert!(args.openmw_cfg.is_none());
        assert!(args.config.is_none());
        assert!(args.output.is_none());
        assert!(args.ignored_plugins.is_empty());
        assert_eq!(args.dry_run, None);
        assert_eq!(args.validate_config, None);
        assert!(!args.auto_enable);
        assert!(!args.debug);
    }

    #[test]
    fn parser_accepts_unclip_plugin() {
        let cli = Cli::parse_from(["greenmote", "unclip", "--plugin", "groundcover.omwaddon"]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert_eq!(
            args.plugin,
            std::path::PathBuf::from("groundcover.omwaddon")
        );
        assert!(args.openmw_cfg.is_none());
        assert!(!args.instances);
        assert!(!args.structured);
        assert!(!args.write);
        assert_close(args.contact_epsilon, 0.5);
        assert_close(args.origin_epsilon, 0.5);
        assert_close(args.relocation_step, 32.0);
        assert_eq!(args.relocation_steps, 8);
    }

    #[test]
    fn parser_accepts_unclip_policy_knobs() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--write-actions",
            "terrain-z,static-move",
            "--contact-epsilon",
            "1.25",
            "--origin-epsilon",
            "2.5",
            "--relocation-step",
            "64",
            "--relocation-steps",
            "12",
            "--include-id",
            "flora_grass_*",
            "--exclude-id",
            "flora_grass_bad_*",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        let policy = args.policy().unwrap();
        assert!(policy.write_actions.terrain_z);
        assert!(!policy.write_actions.static_delete);
        assert!(policy.write_actions.static_move);
        assert_close(policy.contact_epsilon, 1.25);
        assert_close(policy.origin_epsilon, 2.5);
        assert_close(policy.relocation.step, 64.0);
        assert_eq!(policy.relocation.steps, 12);
        assert!(policy.target_filter.includes("flora_grass_01"));
        assert!(!policy.target_filter.includes("flora_grass_bad_01"));
    }

    #[test]
    fn parser_rejects_invalid_unclip_policy_knobs() {
        for flag in [
            ["--contact-epsilon", "-1"],
            ["--origin-epsilon", "nan"],
            ["--relocation-step", "0"],
            ["--relocation-steps", "0"],
        ] {
            let result = Cli::command().try_get_matches_from([
                "greenmote",
                "unclip",
                "--plugin",
                "groundcover.omwaddon",
                flag[0],
                flag[1],
            ]);

            assert!(result.is_err());
        }
    }

    #[test]
    fn unclip_policy_rejects_mixed_write_action_macros() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--write-actions",
            "none,terrain-z",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse before policy validation");
        };

        assert!(args.policy().is_err());
    }

    #[test]
    fn parser_accepts_unclip_structured_output() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--structured",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert!(args.structured);
    }

    #[test]
    fn parser_accepts_unclip_instances() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--instances",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert!(args.instances);
    }

    #[test]
    fn parser_accepts_unclip_write() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--write",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert!(args.write);
    }

    #[test]
    fn parser_rejects_unclip_output_format() {
        let result = Cli::command().try_get_matches_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--format",
            "json",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn parser_rejects_unclip_verbose() {
        let result = Cli::command().try_get_matches_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--verbose",
        ]);

        assert!(result.is_err());
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
