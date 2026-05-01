use crate::groundcover::{self, GroundcoverArgs, GroundcoverConfig};

use super::GreenmoteApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ConvertRunOptions {
    pub(super) dry_run: bool,
    pub(super) debug: bool,
    pub(super) auto_enable: bool,
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

        let args = GroundcoverArgs::default();
        match groundcover::load_config_for_edit(&args).and_then(|(_loaded_path, mut config)| {
            options.apply_to_config(&mut config);
            groundcover::save_config_for_edit(&config, &path)
        }) {
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
    use super::ConvertRunOptions;
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
}
