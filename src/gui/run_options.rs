// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::PathBuf;

use crate::{
    groundcover::{self, GroundcoverConfig, openmw::GroundcoverEntry},
    unclip::UnclipArgs,
};

use super::GreenmoteApp;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ConvertRunOptions {
    pub(super) dry_run: bool,
    pub(super) debug: bool,
    pub(super) auto_enable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct UnclipRunOptions {
    /// The `groundcover=` plugins of the effective `openmw.cfg`, in config order.
    pub(super) entries: Vec<GroundcoverTarget>,
    /// Rewrite the checked plugins. Off means inspect only.
    pub(super) write: bool,
}

impl Default for UnclipRunOptions {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            write: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GroundcoverTarget {
    pub(super) name: String,
    /// Resolved path, or `None` when no data directory provides the plugin.
    pub(super) path: Option<PathBuf>,
    pub(super) checked: bool,
}

impl GroundcoverTarget {
    /// Found plugins start checked; missing ones can never be checked.
    pub(super) fn from_entry(entry: GroundcoverEntry) -> Self {
        Self {
            checked: entry.path.is_some(),
            name: entry.name,
            path: entry.path,
        }
    }

    pub(super) fn is_runnable(&self) -> bool {
        self.checked && self.path.is_some()
    }
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
    pub(super) fn from_entries(entries: Vec<GroundcoverEntry>, write: bool) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(GroundcoverTarget::from_entry)
                .collect(),
            write,
        }
    }

    /// Indices of the entries a run will process, in list order.
    pub(super) fn runnable_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_index, entry)| entry.is_runnable())
            .map(|(index, _entry)| index)
            .collect()
    }

    pub(super) fn runnable_names(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| entry.is_runnable())
            .map(|entry| entry.name.clone())
            .collect()
    }

    pub(super) fn to_args_list(&self) -> Result<Vec<UnclipArgs>, String> {
        let args = self
            .entries
            .iter()
            .filter(|entry| entry.checked)
            .filter_map(|entry| entry.path.clone())
            .map(|path| UnclipArgs {
                plugin: Some(path),
                write: Some(self.write),
                verbose: Some(true),
                structured: Some(false),
                ..UnclipArgs::default()
            })
            .collect::<Vec<_>>();

        if args.is_empty() {
            return Err("Check at least one groundcover plugin before running Unclip.".to_owned());
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
    use std::path::PathBuf;

    use super::{ConvertRunOptions, GroundcoverTarget, UnclipRunOptions};
    use crate::groundcover::{GroundcoverConfig, openmw::GroundcoverEntry};

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

    fn entry(name: &str, found: bool) -> GroundcoverEntry {
        GroundcoverEntry {
            name: name.to_owned(),
            path: found.then(|| PathBuf::from("/data").join(name)),
        }
    }

    #[test]
    fn unclip_run_options_default_write_enabled() {
        assert!(UnclipRunOptions::default().write);
        assert!(UnclipRunOptions::from_entries(Vec::new(), true).write);
        assert!(!UnclipRunOptions::from_entries(Vec::new(), false).write);
    }

    #[test]
    fn unclip_run_options_check_found_entries_by_default() {
        let options = UnclipRunOptions::from_entries(
            vec![
                entry("present.omwaddon", true),
                entry("missing.omwaddon", false),
            ],
            true,
        );

        assert_eq!(
            options.entries,
            [
                GroundcoverTarget {
                    name: "present.omwaddon".to_owned(),
                    path: Some(PathBuf::from("/data/present.omwaddon")),
                    checked: true,
                },
                GroundcoverTarget {
                    name: "missing.omwaddon".to_owned(),
                    path: None,
                    checked: false,
                },
            ]
        );
        assert_eq!(options.runnable_indices(), [0]);
        assert_eq!(options.runnable_names(), ["present.omwaddon"]);
    }

    #[test]
    fn unclip_run_options_build_args_for_checked_entries_only() {
        let mut options = UnclipRunOptions::from_entries(
            vec![
                entry("first.omwaddon", true),
                entry("second.omwaddon", true),
                entry("third.omwaddon", true),
                entry("missing.omwaddon", false),
            ],
            false,
        );
        options.entries[1].checked = false;

        let args = options.to_args_list().unwrap();

        assert_eq!(args.len(), 2);
        assert_eq!(args[0].plugin, Some("/data/first.omwaddon".into()));
        assert_eq!(args[1].plugin, Some("/data/third.omwaddon".into()));
        assert!(args.iter().all(|args| args.write == Some(false)));
        assert!(args.iter().all(|args| args.verbose == Some(true)));
        assert!(args.iter().all(|args| args.structured == Some(false)));
        assert!(args.iter().all(|args| args.output_plugin.is_none()));
        assert!(args.iter().all(|args| args.actions.is_empty()));
        assert_eq!(options.runnable_indices(), [0, 2]);
    }

    #[test]
    fn unclip_run_options_missing_entries_are_never_runnable_even_if_checked() {
        let mut options =
            UnclipRunOptions::from_entries(vec![entry("missing.omwaddon", false)], true);
        options.entries[0].checked = true;

        assert!(options.to_args_list().is_err());
        assert!(options.runnable_indices().is_empty());
    }

    #[test]
    fn unclip_run_options_reject_empty_selection() {
        assert!(UnclipRunOptions::default().to_args_list().is_err());

        let mut options = UnclipRunOptions::from_entries(vec![entry("first.omwaddon", true)], true);
        options.entries[0].checked = false;
        assert_eq!(
            options.to_args_list().unwrap_err(),
            "Check at least one groundcover plugin before running Unclip."
        );
    }
}
