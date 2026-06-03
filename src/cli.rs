// SPDX-License-Identifier: GPL-3.0-only

//! Command-line parser and generated CLI documentation helpers.

use std::path::PathBuf;

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use crate::{groundcover::GroundcoverArgs, unclip::UnclipArgs};

/// Top-level Greenmote command-line options.
#[derive(Parser, Debug)]
#[command(name = "greenmote", author, version)]
pub struct Cli {
    /// Path to openmw.cfg, or a directory containing openmw.cfg.
    #[arg(short = 'c', long = "openmw-cfg")]
    pub openmw_cfg: Option<PathBuf>,

    /// Path to greenmote.toml. Defaults to the `OpenMW` user config directory.
    #[arg(long = "config")]
    pub config: Option<PathBuf>,

    /// Generate shell completion script to stdout for the whole application.
    #[arg(long, value_name = "SHELL", conflicts_with = "generate_manpage")]
    pub generate_completion: Option<Shell>,

    /// Generate roff manpage to stdout for the whole application.
    #[arg(long, conflicts_with = "generate_completion")]
    pub generate_manpage: bool,

    #[command(subcommand)]
    /// Parsed subcommand, or `None` when the invocation should default to Convert.
    pub command: Option<Command>,
}

/// Top-level Greenmote subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Convert vanilla-style static exterior refs into `OpenMW` groundcover.
    Convert(GroundcoverArgs),
    /// Find and optionally fix groundcover refs clipped into terrain or statics.
    Unclip(UnclipArgs),
}

impl Cli {
    /// Returns the parsed subcommand, or `convert` with default arguments when no subcommand was
    /// supplied.
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
        assert!(cli.openmw_cfg.is_none());
        assert!(cli.config.is_none());

        let Command::Convert(args) = cli.command_or_default() else {
            panic!("no subcommand should default to convert");
        };

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
            Some(std::path::PathBuf::from("groundcover.omwaddon"))
        );
        assert_eq!(args.meshgenerator_ini, None);
        assert_eq!(args.instances, None);
        assert_eq!(args.verbose, None);
        assert_eq!(args.structured, None);
        assert_eq!(args.write, None);
        assert_eq!(args.origin_epsilon, None);
        assert_eq!(args.relocation_step, None);
        assert_eq!(args.relocation_steps, None);
        assert_eq!(args.orientation_epsilon, None);
    }

    #[test]
    fn parser_accepts_unclip_meshgenerator_ini() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--meshgenerator-ini",
            "FGM_WG.ini",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert_eq!(
            args.meshgenerator_ini,
            Some(std::path::PathBuf::from("FGM_WG.ini"))
        );
    }

    #[test]
    fn parser_rejects_removed_unclip_placement_model() {
        let error = Cli::try_parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--placement-model",
            "contact",
        ]);

        assert!(error.is_err());
    }

    #[test]
    fn parser_rejects_removed_unclip_auto_placement_model() {
        let error = Cli::try_parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--placement-model",
            "auto",
        ]);

        assert!(error.is_err());
    }

    #[test]
    fn parser_rejects_removed_unclip_contact_epsilon() {
        let error = Cli::try_parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--contact-epsilon",
            "1.25",
        ]);

        assert!(error.is_err());
    }

    #[test]
    fn parser_accepts_openmw_cfg_only_as_top_level_option() {
        for args in [
            ["greenmote", "--openmw-cfg", "profile/openmw.cfg", "convert"],
            ["greenmote", "--openmw-cfg", "profile/openmw.cfg", "unclip"],
        ] {
            let cli = Cli::parse_from(args);

            assert_eq!(cli.openmw_cfg, Some(PathBuf::from("profile/openmw.cfg")));
        }

        for args in [
            ["greenmote", "convert", "--openmw-cfg", "profile/openmw.cfg"],
            ["greenmote", "unclip", "--openmw-cfg", "profile/openmw.cfg"],
        ] {
            let result = Cli::command().try_get_matches_from(args);

            assert!(result.is_err());
        }
    }

    #[test]
    fn parser_accepts_config_only_as_top_level_option() {
        let cli = Cli::parse_from(["greenmote", "--config", "custom.toml", "convert"]);

        assert_eq!(cli.config, Some(PathBuf::from("custom.toml")));

        for args in [
            ["greenmote", "convert", "--config", "custom.toml"],
            ["greenmote", "unclip", "--config", "custom.toml"],
        ] {
            let result = Cli::command().try_get_matches_from(args);

            assert!(result.is_err());
        }
    }

    #[test]
    fn parser_accepts_unclip_policy_knobs() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--write-actions",
            "terrain-z,water-delete,static-move,orient",
            "--origin-epsilon",
            "2.5",
            "--relocation-step",
            "64",
            "--relocation-steps",
            "12",
            "--orientation-epsilon",
            "3.5",
            "--include-grass-id",
            "^flora_grass_.*$",
            "--exclude-grass-id",
            "^flora_grass_bad_.*$",
            "--include-occluder-id",
            "^terrain_.*$",
            "--exclude-occluder-id",
            "^terrain_tree_huge$",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        let policy = args.policy().unwrap();
        assert!(policy.write_actions.terrain_z());
        assert!(!policy.write_actions.static_delete());
        assert!(policy.write_actions.water_delete());
        assert!(policy.write_actions.static_move());
        assert!(policy.write_actions.orient());
        assert_close(policy.origin_epsilon, 2.5);
        assert_close(policy.relocation.step, 64.0);
        assert_eq!(policy.relocation.steps, 12);
        assert_close(policy.orientation_epsilon_degrees, 3.5);
        assert!(policy.target_filter.includes("flora_grass_01"));
        assert!(!policy.target_filter.includes("flora_grass_bad_01"));
        assert!(policy.occluder_filter.includes("terrain_rock_01"));
        assert!(!policy.occluder_filter.includes("terrain_tree_huge"));
    }

    #[test]
    fn parser_rejects_invalid_unclip_policy_knobs() {
        for flag in [
            ["--origin-epsilon", "nan"],
            ["--relocation-step", "0"],
            ["--relocation-steps", "0"],
            ["--orientation-epsilon", "-1"],
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
    fn parser_accepts_unclip_bool_false_overrides() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--instances=false",
            "--verbose=false",
            "--structured=false",
            "--write=false",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert_eq!(args.instances, Some(false));
        assert_eq!(args.verbose, Some(false));
        assert_eq!(args.structured, Some(false));
        assert_eq!(args.write, Some(false));
    }

    #[test]
    fn unclip_policy_rejects_invalid_regex_filters() {
        for flag in ["--include-grass-id", "--exclude-occluder-id"] {
            let cli = Cli::parse_from([
                "greenmote",
                "unclip",
                "--plugin",
                "groundcover.omwaddon",
                flag,
                "(",
            ]);

            let Some(Command::Unclip(args)) = cli.command else {
                panic!("unclip command should parse before policy validation");
            };

            assert!(args.policy().is_err());
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

        assert_eq!(args.structured, Some(true));
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

        assert_eq!(args.instances, Some(true));
    }

    #[test]
    fn parser_accepts_unclip_verbose() {
        let cli = Cli::parse_from([
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--verbose",
        ]);

        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };

        assert_eq!(args.verbose, Some(true));
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

        assert_eq!(args.write, Some(true));
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

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
