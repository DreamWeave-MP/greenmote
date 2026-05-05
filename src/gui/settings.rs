use std::path::{Path, PathBuf};

use eframe::egui;

use crate::groundcover::{self, GroundcoverConfig};

use super::{ConvertRunOptions, GreenmoteApp, PendingNavigation};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsTab {
    General,
    Convert,
}

pub(super) struct SettingsUiState {
    selected_tab: SettingsTab,
    draft: SettingsDraft,
    config_path: Option<PathBuf>,
    loaded: bool,
    dirty: bool,
    status: String,
    error: Option<String>,
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct SettingsDraft {
    output_directory: String,
    grass_ids: String,
    exclude: String,
    ignored_plugins: String,
    dry_run: bool,
    debug: bool,
    auto_enable: bool,
    unclip: crate::unclip::config::PersistedUnclipConfig,
}

impl Default for SettingsUiState {
    fn default() -> Self {
        Self {
            selected_tab: SettingsTab::General,
            draft: SettingsDraft::from_config(&GroundcoverConfig::default()),
            config_path: None,
            loaded: false,
            dirty: false,
            status: String::new(),
            error: None,
        }
    }
}

impl SettingsUiState {
    pub(super) fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub(super) fn select_tab(&mut self, tab: SettingsTab) {
        self.selected_tab = tab;
    }

    pub(super) fn run_options(&self) -> ConvertRunOptions {
        self.draft.run_options()
    }

    pub(super) fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    pub(super) fn output_directory(&self) -> PathBuf {
        PathBuf::from(self.draft.output_directory.trim())
    }

    pub(super) fn log_directory(&self) -> Option<PathBuf> {
        self.config_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
    }

    pub(super) fn log_path(&self) -> Option<PathBuf> {
        self.log_directory()
            .map(|directory| directory.join(crate::groundcover::LOG_NAME))
    }

    pub(super) fn replace_saved_config(
        &mut self,
        path: PathBuf,
        config: &GroundcoverConfig,
        status: String,
    ) {
        self.draft = SettingsDraft::from_config(config);
        self.config_path = Some(path);
        self.loaded = true;
        self.dirty = false;
        self.status = status;
        self.error = None;
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

        egui::TopBottomPanel::bottom("settings_footer")
            .resizable(false)
            .show_separator_line(true)
            .show_inside(ui, |ui| {
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
            });

        let available = ui.available_size();

        ui.allocate_ui_with_layout(available, egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(560.0);
                    match self.settings.selected_tab {
                        SettingsTab::General => self.show_general_settings(ui),
                        SettingsTab::Convert => self.show_convert_settings(ui),
                    }
                });
        });
    }

    fn show_general_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("OpenMW config");
        ui.add_space(4.0);

        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| match &self.session_openmw_cfg {
                Some(path) => {
                    ui.monospace(path.display().to_string());
                }
                None => {
                    ui.label("Using OpenMW autodetection.");
                }
            });

        ui.add_space(6.0);

        let can_select_config = !self.convert.is_running();
        if ui
            .add_enabled(can_select_config, egui::Button::new("Select OpenMW Config"))
            .clicked()
        {
            self.request_openmw_config_selection();
        }
        if !can_select_config {
            ui.label("OpenMW config cannot be changed while conversion is running.");
        }

        ui.add_space(8.0);

        setting_text_field(
            ui,
            "Output directory",
            &mut self.settings.draft.output_directory,
            &mut self.settings.dirty,
        );
    }

    fn show_convert_settings(&mut self, ui: &mut egui::Ui) {
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
        ui.label("Run options are configured on the Convert screen.");
    }

    pub(super) fn load_settings(&mut self) -> bool {
        if self.settings.loaded {
            return true;
        }

        self.reload_settings()
    }

    pub(super) fn settings_error(&self) -> Option<&str> {
        self.settings.error.as_deref()
    }

    fn reload_settings(&mut self) -> bool {
        self.load_settings_from_disk("Loaded")
    }

    pub(super) fn load_settings_with_openmw_cfg(&mut self, openmw_cfg: &Path) -> bool {
        match groundcover::load_config_for_edit(Some(openmw_cfg), None) {
            Ok((path, config)) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Loaded {}", path.display()),
                );
                self.convert
                    .sync_run_options(ConvertRunOptions::from_config(&config));
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

    pub(super) fn regenerate_settings(&mut self) -> bool {
        match groundcover::regenerate_config_for_edit(self.session_openmw_cfg.as_deref(), None) {
            Ok((path, config)) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Regenerated {}", path.display()),
                );
                self.convert
                    .sync_run_options(ConvertRunOptions::from_config(&config));
                true
            }
            Err(error) => {
                self.settings.loaded = false;
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to regenerate settings: {error}"));
                false
            }
        }
    }

    pub(super) fn discard_settings(&mut self) -> bool {
        self.load_settings_from_disk("Discarded changes and reloaded")
    }

    fn load_settings_from_disk(&mut self, verb: &str) -> bool {
        match groundcover::load_config_for_edit(self.session_openmw_cfg.as_deref(), None) {
            Ok((path, config)) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("{verb} {}", path.display()),
                );
                self.convert
                    .sync_run_options(ConvertRunOptions::from_config(&config));
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

    pub(super) fn save_settings(&mut self) -> bool {
        let Some(path) = self.settings.config_path.clone() else {
            self.settings.error = Some("No greenmote.toml path is available.".to_owned());
            return false;
        };

        let config = self.settings.draft.to_config();
        match groundcover::save_config_for_edit(&config, &path) {
            Ok(config) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Saved {}", path.display()),
                );
                self.convert
                    .sync_saved_run_options_from_settings(ConvertRunOptions::from_config(&config));
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
            if self.has_pending_navigation_request() {
                return;
            }

            self.queue_pending_navigation(PendingNavigation::SettingsTab(tab));
        } else {
            self.settings.select_tab(tab);
        }
    }
}

impl SettingsDraft {
    fn from_config(config: &GroundcoverConfig) -> Self {
        let run_options = ConvertRunOptions::from_config(config);
        Self {
            output_directory: path_to_string(&config.output_directory),
            grass_ids: vec_to_lines(&config.grass_ids),
            exclude: vec_to_lines(&config.exclude),
            ignored_plugins: vec_to_lines(&config.ignored_plugins),
            dry_run: run_options.dry_run,
            debug: run_options.debug,
            auto_enable: run_options.auto_enable,
            unclip: config.unclip.clone(),
        }
    }

    fn to_config(&self) -> GroundcoverConfig {
        let mut config = GroundcoverConfig::default();
        config.output_directory = PathBuf::from(self.output_directory.trim());
        config.grass_ids = lines_to_vec(&self.grass_ids);
        config.exclude = lines_to_vec(&self.exclude);
        config.ignored_plugins = lines_to_vec(&self.ignored_plugins);
        config.dry_run = self.dry_run;
        config.debug = self.debug;
        config.auto_enable = self.auto_enable;
        config.unclip = self.unclip.clone();
        config
    }

    fn run_options(&self) -> ConvertRunOptions {
        ConvertRunOptions {
            dry_run: self.dry_run,
            debug: self.debug,
            auto_enable: self.auto_enable,
        }
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
