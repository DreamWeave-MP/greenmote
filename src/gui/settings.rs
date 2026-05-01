use std::path::{Path, PathBuf};

use eframe::egui;

use crate::groundcover::{self, GroundcoverArgs, GroundcoverConfig};

use super::{GreenmoteApp, Screen};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsTab {
    General,
    Convert,
}

pub(super) struct SettingsUiState {
    pub(super) selected_tab: SettingsTab,
    pub(super) draft: SettingsDraft,
    pub(super) config_path: Option<PathBuf>,
    pub(super) pending_action: Option<PendingSettingsAction>,
    pub(super) loaded: bool,
    pub(super) dirty: bool,
    pub(super) status: String,
    pub(super) error: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum PendingSettingsAction {
    ShowScreen(Screen),
    SelectTab(SettingsTab),
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct SettingsDraft {
    output_directory: String,
    groundcover_output: String,
    deleted_output: String,
    grass_ids: String,
    exclude: String,
    ignored_plugins: String,
    dry_run: bool,
    validate_config: bool,
    debug: bool,
    auto_enable: bool,
}

impl Default for SettingsUiState {
    fn default() -> Self {
        Self {
            selected_tab: SettingsTab::General,
            draft: SettingsDraft::from_config(&GroundcoverConfig::default()),
            config_path: None,
            pending_action: None,
            loaded: false,
            dirty: false,
            status: String::new(),
            error: None,
        }
    }
}

impl GreenmoteApp {
    pub(super) fn show_settings_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    self.settings.selected_tab == SettingsTab::General,
                    "General",
                )
                .clicked()
            {
                self.request_settings_tab(SettingsTab::General);
            }
            if ui
                .selectable_label(
                    self.settings.selected_tab == SettingsTab::Convert,
                    "Convert",
                )
                .clicked()
            {
                self.request_settings_tab(SettingsTab::Convert);
            }
        });
        ui.separator();

        let footer_height = 44.0;
        let available = ui.available_size();
        let body_height = (available.y - footer_height).max(0.0);

        ui.allocate_ui_with_layout(
            egui::vec2(available.x, body_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(560.0);
                        match self.settings.selected_tab {
                            SettingsTab::General => self.show_general_settings(ui),
                            SettingsTab::Convert => self.show_convert_settings(ui),
                        }
                    });
            },
        );

        ui.separator();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(self.settings.dirty, egui::Button::new("Save"))
                .clicked()
            {
                self.save_settings();
            }

            if let Some(error) = &self.settings.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            } else if !self.settings.status.is_empty() {
                ui.label(&self.settings.status);
            }
        });
    }

    fn show_general_settings(&mut self, ui: &mut egui::Ui) {
        setting_text_field(
            ui,
            "Output directory",
            &mut self.settings.draft.output_directory,
            &mut self.settings.dirty,
        );
    }

    fn show_convert_settings(&mut self, ui: &mut egui::Ui) {
        setting_text_field(
            ui,
            "Groundcover output plugin",
            &mut self.settings.draft.groundcover_output,
            &mut self.settings.dirty,
        );
        setting_text_field(
            ui,
            "Deleted refs output plugin",
            &mut self.settings.draft.deleted_output,
            &mut self.settings.dirty,
        );

        ui.add_space(8.0);
        setting_multiline_text(
            ui,
            "Grass ID patterns",
            &mut self.settings.draft.grass_ids,
            &mut self.settings.dirty,
        );
        setting_multiline_text(
            ui,
            "Exclude patterns",
            &mut self.settings.draft.exclude,
            &mut self.settings.dirty,
        );
        setting_multiline_text(
            ui,
            "Ignored plugins",
            &mut self.settings.draft.ignored_plugins,
            &mut self.settings.dirty,
        );

        ui.add_space(8.0);
        setting_checkbox(
            ui,
            "Dry run",
            &mut self.settings.draft.dry_run,
            &mut self.settings.dirty,
        );
        if self.settings.draft.dry_run && self.settings.draft.validate_config {
            self.settings.draft.validate_config = false;
            self.settings.dirty = true;
        }
        setting_checkbox(
            ui,
            "Validate config",
            &mut self.settings.draft.validate_config,
            &mut self.settings.dirty,
        );
        if self.settings.draft.validate_config && self.settings.draft.dry_run {
            self.settings.draft.dry_run = false;
            self.settings.dirty = true;
        }
        setting_checkbox(
            ui,
            "Debug diagnostics",
            &mut self.settings.draft.debug,
            &mut self.settings.dirty,
        );
        setting_checkbox(
            ui,
            "Auto-enable generated plugins",
            &mut self.settings.draft.auto_enable,
            &mut self.settings.dirty,
        );
    }

    pub(super) fn load_settings(&mut self) {
        if self.settings.loaded {
            return;
        }

        self.reload_settings();
    }

    fn reload_settings(&mut self) -> bool {
        self.load_settings_from_disk("Loaded")
    }

    fn discard_settings(&mut self) -> bool {
        self.load_settings_from_disk("Discarded changes and reloaded")
    }

    fn load_settings_from_disk(&mut self, verb: &str) -> bool {
        match groundcover::load_config_for_edit(&GroundcoverArgs::default()) {
            Ok((path, config)) => {
                self.settings.draft = SettingsDraft::from_config(&config);
                self.settings.config_path = Some(path.clone());
                self.settings.loaded = true;
                self.settings.dirty = false;
                self.settings.status = format!("{verb} {}", path.display());
                self.settings.error = None;
                true
            }
            Err(error) => {
                self.settings.loaded = false;
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to load settings: {error}"));
                false
            }
        }
    }

    fn save_settings(&mut self) -> bool {
        let Some(path) = self.settings.config_path.clone() else {
            self.settings.error = Some("No greenmote.toml path is available.".to_owned());
            return false;
        };

        let config = self.settings.draft.to_config();
        match groundcover::save_config_for_edit(&config, &path) {
            Ok(config) => {
                self.settings.draft = SettingsDraft::from_config(&config);
                self.settings.dirty = false;
                self.settings.status = format!("Saved {}", path.display());
                self.settings.error = None;
                true
            }
            Err(error) => {
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to save settings: {error}"));
                false
            }
        }
    }

    fn request_settings_tab(&mut self, tab: SettingsTab) {
        if self.settings.selected_tab == tab {
            return;
        }

        if self.settings.dirty {
            self.settings.pending_action = Some(PendingSettingsAction::SelectTab(tab));
        } else {
            self.settings.selected_tab = tab;
        }
    }

    pub(super) fn perform_pending_settings_action(&mut self) {
        let Some(action) = self.settings.pending_action.take() else {
            return;
        };

        match action {
            PendingSettingsAction::ShowScreen(screen) => self.show_screen(screen),
            PendingSettingsAction::SelectTab(tab) => self.settings.selected_tab = tab,
        }
    }

    pub(super) fn show_pending_settings_prompt(&mut self, ctx: &egui::Context) {
        if self.settings.pending_action.is_none() {
            return;
        }

        let mut save = false;
        let mut discard = false;
        let mut cancel = false;

        egui::Window::new("Unsaved settings")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Settings have unsaved changes.");
                ui.label("Save them before switching views?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    discard = ui.button("Discard").clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });

        if save {
            if self.save_settings() {
                self.perform_pending_settings_action();
            }
        } else if discard {
            if self.discard_settings() {
                self.perform_pending_settings_action();
            }
        } else if cancel {
            self.settings.pending_action = None;
        }
    }
}

impl SettingsDraft {
    fn from_config(config: &GroundcoverConfig) -> Self {
        Self {
            output_directory: path_to_string(&config.output_directory),
            groundcover_output: config.groundcover_output.clone(),
            deleted_output: config.deleted_output.clone(),
            grass_ids: vec_to_lines(&config.grass_ids),
            exclude: vec_to_lines(&config.exclude),
            ignored_plugins: vec_to_lines(&config.ignored_plugins),
            dry_run: config.dry_run,
            validate_config: config.validate_config,
            debug: config.debug,
            auto_enable: config.auto_enable,
        }
    }

    fn to_config(&self) -> GroundcoverConfig {
        let mut config = GroundcoverConfig::default();
        config.output_directory = PathBuf::from(self.output_directory.trim());
        self.groundcover_output
            .trim()
            .clone_into(&mut config.groundcover_output);
        self.deleted_output
            .trim()
            .clone_into(&mut config.deleted_output);
        config.grass_ids = lines_to_vec(&self.grass_ids);
        config.exclude = lines_to_vec(&self.exclude);
        config.ignored_plugins = lines_to_vec(&self.ignored_plugins);
        config.dry_run = self.dry_run;
        config.validate_config = self.validate_config;
        config.debug = self.debug;
        config.auto_enable = self.auto_enable;
        config
    }
}

fn setting_text_field(ui: &mut egui::Ui, label: &str, value: &mut String, dirty: &mut bool) {
    ui.label(label);
    if ui.text_edit_singleline(value).changed() {
        *dirty = true;
    }
}

fn setting_multiline_text(ui: &mut egui::Ui, label: &str, value: &mut String, dirty: &mut bool) {
    ui.label(label);
    let editor = egui::TextEdit::multiline(value)
        .desired_rows(5)
        .desired_width(f32::INFINITY);
    if ui.add(editor).changed() {
        *dirty = true;
    }
}

fn setting_checkbox(ui: &mut egui::Ui, label: &str, value: &mut bool, dirty: &mut bool) {
    if ui.checkbox(value, label).changed() {
        *dirty = true;
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn vec_to_lines(lines: &[String]) -> String {
    lines.join("\n")
}

fn lines_to_vec(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
