use std::path::PathBuf;

use crate::{
    groundcover::{self, GroundcoverConfig},
    unclip::UnclipArgs,
};

use super::GreenmoteApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ConvertRunOptions {
    pub(super) dry_run: bool,
    pub(super) debug: bool,
    pub(super) auto_enable: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UnclipRunOptions {
    pub(super) target_plugins: Vec<String>,
    pub(super) verbose: bool,
    pub(super) write: bool,
}

impl ConvertRunOptions {
    #[must_use]
    pub(super) fn from_config(config: &GroundcoverConfig) -> Self {
        Self {
            dry_run: config.dry_run,
            debug: config.debug,
            auto_enable: config.auto_enable,
        }
        .normalized()
    }

    pub(super) fn apply_to_config(self, config: &mut GroundcoverConfig) {
        let normalized = self.normalized();
        config.dry_run = normalized.dry_run;
        config.debug = normalized.debug;
        config.auto_enable = normalized.auto_enable;
    }

    #[must_use]
    pub(super) fn normalized(mut self) -> Self {
        if self.dry_run {
            self.debug = false;
        }

        self
    }

    pub(super) fn set_dry_run(&mut self, enabled: bool) {
        self.dry_run = enabled;
        if enabled {
            self.debug = false;
        }
    }

    pub(super) fn set_debug(&mut self, enabled: bool) {
        self.debug = enabled;
        if enabled {
            self.dry_run = false;
        }
    }

    #[must_use]
    pub(super) fn can_edit_auto_enable(self) -> bool {
        !self.dry_run
    }
}

impl UnclipRunOptions {
    #[must_use]
    pub(super) fn from_config(config: &GroundcoverConfig) -> Self {
        let target_plugins = config
            .unclip
            .plugin
            .as_ref()
            .map(|path| vec![path.display().to_string()])
            .unwrap_or_default();

        Self {
            target_plugins,
            verbose: config.unclip.verbose.unwrap_or(false),
            write: false,
        }
    }

    pub(super) fn add_target(&mut self, target: impl Into<String>) -> bool {
        let target = target.into();
        let target = target.trim();
        if target.is_empty()
            || self
                .target_plugins
                .iter()
                .any(|existing| existing == target)
        {
            return false;
        }

        self.target_plugins.push(target.to_owned());
        true
    }

    pub(super) fn remove_target(&mut self, index: usize) -> bool {
        if index >= self.target_plugins.len() {
            return false;
        }

        self.target_plugins.remove(index);
        true
    }

    pub(super) fn clear_targets(&mut self) {
        self.target_plugins.clear();
    }

    pub(super) fn to_args_list(&self) -> Result<Vec<UnclipArgs>, String> {
        let plugins = self
            .target_plugins
            .iter()
            .map(|plugin| plugin.trim())
            .filter(|plugin| !plugin.is_empty());
        let args = plugins
            .map(|plugin| UnclipArgs {
                plugin: Some(PathBuf::from(plugin)),
                meshgenerator_ini: None,
                ignore_meshgenerator_ini: true,
                instances: None,
                verbose: Some(self.verbose),
                structured: Some(false),
                write: Some(self.write),
                write_actions: Vec::new(),
                origin_epsilon: None,
                relocation_step: None,
                relocation_steps: None,
                orientation_epsilon: None,
                include_grass_ids: Vec::new(),
                exclude_grass_ids: Vec::new(),
                include_occluder_ids: Vec::new(),
                exclude_occluder_ids: Vec::new(),
            })
            .collect::<Vec<_>>();

        if args.is_empty() {
            return Err("Choose a target plugin before running Unclip.".to_owned());
        }

        Ok(args)
    }
}

impl GreenmoteApp {
    pub(super) fn save_convert_run_options_as_defaults(&mut self) -> bool {
        if self.settings.is_dirty() {
            self.set_status(
                "Save or discard Settings changes before saving Convert run options as defaults.",
            );
            return false;
        }

        let Some(path) = self.settings.config_path().map(ToOwned::to_owned) else {
            self.set_status("No greenmote.toml path is available.");
            return false;
        };

        let options = self.convert.current_run_options();
        if self.settings.run_options() == options {
            self.convert.mark_run_options_saved(options);
            self.set_status("Run options already match saved defaults.");
            return true;
        }

        match groundcover::load_config_for_edit(self.session_openmw_cfg.as_deref(), None).and_then(
            |(_loaded_path, mut config)| {
                options.apply_to_config(&mut config);
                groundcover::save_config_for_edit(&config, &path)
            },
        ) {
            Ok(config) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Saved {}", path.display()),
                );
                self.convert.sync_run_options(options);
                self.set_status(format!("Saved run options to {}", path.display()));
                true
            }
            Err(error) => {
                self.set_status(format!("Failed to save run options: {error}"));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ConvertRunOptions, UnclipRunOptions};
    use crate::groundcover::GroundcoverConfig;

    #[test]
    fn run_options_can_disable_saved_boolean_defaults() {
        let mut config = GroundcoverConfig::default();
        config.dry_run = true;
        config.debug = true;
        config.auto_enable = true;
        let options = ConvertRunOptions {
            dry_run: false,
            debug: false,
            auto_enable: false,
        };

        options.apply_to_config(&mut config);

        assert!(!config.dry_run);
        assert!(!config.debug);
        assert!(!config.auto_enable);
    }

    #[test]
    fn run_options_round_trip_from_config() {
        let mut config = GroundcoverConfig::default();
        config.dry_run = true;
        config.debug = false;
        config.auto_enable = true;

        assert_eq!(
            ConvertRunOptions::from_config(&config),
            ConvertRunOptions {
                dry_run: true,
                debug: false,
                auto_enable: true,
            }
        );
    }

    #[test]
    fn dry_run_and_debug_are_mutually_exclusive() {
        let mut options = ConvertRunOptions::default();

        options.set_debug(true);
        assert!(options.debug);
        assert!(!options.dry_run);

        options.set_dry_run(true);
        assert!(options.dry_run);
        assert!(!options.debug);
    }

    #[test]
    fn dry_run_wins_when_saved_config_has_both_exclusive_flags() {
        let mut config = GroundcoverConfig::default();
        config.dry_run = true;
        config.debug = true;

        assert_eq!(
            ConvertRunOptions::from_config(&config),
            ConvertRunOptions {
                dry_run: true,
                debug: false,
                auto_enable: false,
            }
        );
    }

    #[test]
    fn auto_enable_is_not_editable_during_dry_run() {
        assert!(ConvertRunOptions::default().can_edit_auto_enable());
        assert!(
            !ConvertRunOptions {
                dry_run: true,
                debug: false,
                auto_enable: true,
            }
            .can_edit_auto_enable()
        );
    }

    #[test]
    fn unclip_run_options_prefill_visible_plugin_and_verbose_only() {
        let mut config = GroundcoverConfig::default();
        config.unclip.plugin = Some("groundcover.omwaddon".into());
        config.unclip.meshgenerator_ini = Some("groundcover.ini".into());
        config.unclip.verbose = Some(true);
        config.unclip.write = Some(true);

        let options = UnclipRunOptions::from_config(&config);

        assert_eq!(options.target_plugins, ["groundcover.omwaddon"]);
        assert!(options.verbose);
        assert!(!options.write);
    }

    #[test]
    fn unclip_run_options_config_prefill_creates_one_target() {
        let mut config = GroundcoverConfig::default();
        config.unclip.plugin = Some("groundcover.omwaddon".into());

        let options = UnclipRunOptions::from_config(&config);

        assert_eq!(options.target_plugins, vec!["groundcover.omwaddon"]);
    }

    #[test]
    fn unclip_run_options_build_explicit_safe_args() {
        let options = UnclipRunOptions {
            target_plugins: vec![" groundcover.omwaddon ".to_owned()],
            verbose: true,
            write: false,
        };

        let args = options.to_args_list().unwrap();
        let args = args.first().unwrap();

        assert_eq!(args.plugin, Some("groundcover.omwaddon".into()));
        assert_eq!(args.meshgenerator_ini, None);
        assert!(args.ignore_meshgenerator_ini);
        assert_eq!(args.instances, None);
        assert_eq!(args.verbose, Some(true));
        assert_eq!(args.structured, Some(false));
        assert_eq!(args.write, Some(false));
        assert!(args.write_actions.is_empty());
    }

    #[test]
    fn unclip_run_options_reject_empty_plugin() {
        assert!(UnclipRunOptions::default().to_args_list().is_err());
    }

    #[test]
    fn unclip_run_options_add_target_preserves_order_and_dedupes_exact_names() {
        let mut options = UnclipRunOptions::default();

        assert!(options.add_target(" first.omwaddon "));
        assert!(options.add_target("second.omwaddon"));
        assert!(!options.add_target("first.omwaddon"));
        assert!(!options.add_target(" "));

        assert_eq!(
            options.target_plugins,
            ["first.omwaddon", "second.omwaddon"]
        );
    }

    #[test]
    fn unclip_run_options_remove_and_clear_targets() {
        let mut options = UnclipRunOptions {
            target_plugins: vec!["first.omwaddon".to_owned(), "second.omwaddon".to_owned()],
            verbose: false,
            write: false,
        };

        assert!(options.remove_target(0));
        assert!(!options.remove_target(3));
        assert_eq!(options.target_plugins, ["second.omwaddon"]);

        options.clear_targets();
        assert!(options.target_plugins.is_empty());
    }

    #[test]
    fn unclip_run_options_builds_multiple_args() {
        let options = UnclipRunOptions {
            target_plugins: vec![" first.omwaddon ".to_owned(), "second.omwaddon".to_owned()],
            verbose: false,
            write: true,
        };

        let args = options.to_args_list().unwrap();

        assert_eq!(args.len(), 2);
        assert_eq!(args[0].plugin, Some("first.omwaddon".into()));
        assert_eq!(args[1].plugin, Some("second.omwaddon".into()));
    }

    #[test]
    fn unclip_run_options_all_args_ignore_meshgenerator_ini() {
        let options = UnclipRunOptions {
            target_plugins: vec!["first.omwaddon".to_owned(), "second.omwaddon".to_owned()],
            verbose: false,
            write: false,
        };

        let args = options.to_args_list().unwrap();

        assert!(args.iter().all(|args| args.meshgenerator_ini.is_none()));
        assert!(args.iter().all(|args| args.ignore_meshgenerator_ini));
    }
}
