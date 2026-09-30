// SPDX-License-Identifier: MIT OR Apache-2.0

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
    Unclip(Box<UnclipArgs>),
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

    fn unclip_args(args: &[&str]) -> UnclipArgs {
        let cli = Cli::parse_from(args);
        let Some(Command::Unclip(args)) = cli.command else {
            panic!("unclip command should parse");
        };
        *args
    }

    fn unclip_config(args: &UnclipArgs) -> crate::unclip::config::UnclipConfig {
        crate::unclip::config::UnclipConfig::merge(
            args,
            crate::unclip::config::PersistedUnclipConfig::default(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn parser_accepts_unclip_plugin() {
        let args = unclip_args(&["greenmote", "unclip", "--plugin", "groundcover.omwaddon"]);

        assert_eq!(
            args.plugin,
            Some(std::path::PathBuf::from("groundcover.omwaddon"))
        );
        assert_eq!(args.write, None);
        assert_eq!(args.output_plugin, None);
        assert_eq!(args.verbose, None);
        assert_eq!(args.structured, None);
        assert!(args.actions.is_empty());
        assert_eq!(args.float_tolerance, None);
        assert_eq!(args.max_sink, None);
        assert_eq!(args.sink, None);
        assert_eq!(args.relocation_step, None);
        assert_eq!(args.relocation_steps, None);
        assert_eq!(args.orientation_epsilon, None);
        assert_eq!(args.ignore_missing_meshes, None);
        assert!(!args.no_default_occluder_excludes);
        assert!(!args.no_default_road_textures);
    }

    #[test]
    fn parser_accepts_unclip_write() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--write",
        ]);
        assert_eq!(args.write, Some(true));

        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--write",
            "--output-plugin",
            "rewritten.omwaddon",
        ]);
        assert_eq!(args.write, Some(true));
        assert_eq!(
            args.output_plugin,
            Some(std::path::PathBuf::from("rewritten.omwaddon"))
        );
    }

    #[test]
    fn parser_accepts_unclip_ignore_missing_meshes() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--ignore-missing-meshes",
        ]);

        assert_eq!(args.ignore_missing_meshes, Some(true));
    }

    #[test]
    fn parser_accepts_unclip_policy_knobs() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--actions",
            "terrain-z,orient",
            "--float-tolerance",
            "0.5",
            "--max-sink",
            "10",
            "--sink",
            "1",
            "--relocation-step",
            "64",
            "--relocation-steps",
            "12",
            "--orientation-epsilon",
            "3.5",
            "--max-tilt",
            "90",
            "--include-grass-id",
            "^flora_grass_.*$",
            "--exclude-grass-id",
            "^flora_grass_bad_.*$",
            "--include-occluder-id",
            "^terrain_.*$",
            "--exclude-occluder-id",
            "^terrain_tree_huge$",
            "--road-texture-path",
            "^textures/road/custom_.*\\.dds$",
        ]);

        assert_eq!(
            args.actions,
            vec![
                crate::unclip::ActionArg::TerrainZ,
                crate::unclip::ActionArg::Orient
            ]
        );
        assert_eq!(args.float_tolerance, Some(0.5));
        assert_eq!(args.max_sink, Some(10.0));
        assert_eq!(args.sink, Some(1.0));

        let policy = unclip_config(&args).policy().unwrap();
        assert!(policy.actions.terrain_z());
        assert!(policy.actions.orient());
        assert!(!policy.actions.water_delete());
        assert!(!policy.actions.road_delete());
        assert!(!policy.actions.static_delete());
        assert!(!policy.actions.static_move());
        assert_close(policy.float_tolerance, 0.5);
        assert_close(policy.max_sink, 10.0);
        assert_close(policy.sink, 1.0);
        assert_close(policy.relocation.step, 64.0);
        assert_eq!(policy.relocation.steps, 12);
        assert_close(policy.orientation_epsilon_degrees, 3.5);
        assert_close(policy.max_tilt_degrees, 90.0);
        assert!(policy.target_filter.includes("flora_grass_01"));
        assert!(!policy.target_filter.includes("flora_grass_bad_01"));
        assert!(policy.occluder_filter.includes("terrain_rock_01"));
        assert!(!policy.occluder_filter.includes("terrain_tree_huge"));
        assert!(
            policy
                .road_texture_filter
                .includes("textures/road/custom_good.dds")
        );
    }

    #[test]
    fn parser_rejects_invalid_unclip_policy_knobs() {
        for flag in [
            ["--float-tolerance", "nan"],
            ["--max-sink", "-1"],
            ["--sink", "inf"],
            ["--relocation-step", "0"],
            ["--relocation-steps", "0"],
            ["--orientation-epsilon", "-1"],
            ["--max-tilt", "0"],
            ["--max-tilt", "90.5"],
            ["--max-tilt", "nan"],
            ["--actions", "all"],
        ] {
            let result = Cli::command().try_get_matches_from([
                "greenmote",
                "unclip",
                "--plugin",
                "groundcover.omwaddon",
                flag[0],
                flag[1],
            ]);

            assert!(
                result.is_err(),
                "{} {} should be rejected",
                flag[0],
                flag[1]
            );
        }
    }

    #[test]
    fn parser_accepts_unclip_bool_false_overrides() {
        let args = unclip_args(&[
            "greenmote",
            "unclip",
            "--plugin",
            "groundcover.omwaddon",
            "--verbose=false",
            "--structured=false",
            "--write=false",
            "--ignore-missing-meshes=false",
        ]);

        assert_eq!(args.verbose, Some(false));
        assert_eq!(args.structured, Some(false));
        assert_eq!(args.write, Some(false));
        assert_eq!(args.ignore_missing_meshes, Some(false));
    }

    #[test]
    fn unclip_policy_rejects_invalid_regex_filters() {
        for flag in [
            "--include-grass-id",
            "--exclude-grass-id",
            "--include-occluder-id",
            "--exclude-occluder-id",
            "--road-texture-path",
        ] {
            let args = unclip_args(&[
                "greenmote",
                "unclip",
                "--plugin",
                "groundcover.omwaddon",
                flag,
                "(",
            ]);

            assert!(unclip_config(&args).policy().is_err());
        }
    }

    #[test]
    fn parser_rejects_removed_unclip_flags() {
        for flags in [
            vec!["--dry-run"],
            vec!["--in-place"],
            vec!["--instances"],
            vec!["--meshgenerator-ini", "FGM_WG.ini"],
            vec!["--ignore-meshgenerator-ini"],
            vec!["--origin-epsilon", "2.5"],
            vec!["--write-actions", "all"],
        ] {
            let mut command = vec!["greenmote", "unclip", "--plugin", "groundcover.omwaddon"];
            command.extend(flags.iter().copied());
            let result = Cli::command().try_get_matches_from(command);

            assert!(result.is_err(), "{flags:?} should be rejected");
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
