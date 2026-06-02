use std::{
    io,
    path::{Path, PathBuf},
};

use eframe::egui;

mod convert;
mod localization;
mod run_options;
mod settings;

use convert::ConvertUiState;
use localization::{Localizer, UiLanguage, UiText};
use run_options::{ConvertRunOptions, UnclipRunOptions};
use settings::SettingsUiState;

struct GreenmoteApp {
    selected_tab: AppTab,
    convert: ConvertUiState,
    settings: SettingsUiState,
    pending_navigation: Option<PendingNavigation>,
    checked_initial_config: bool,
    config_recovery_error: Option<String>,
    openmw_config_error: Option<String>,
    session_openmw_cfg: Option<PathBuf>,
    localizer: Localizer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppTab {
    Convert,
    Unclip,
    Settings,
}

enum PendingNavigation {
    Tab(AppTab),
    OpenMwConfig(PathBuf),
}

impl Default for GreenmoteApp {
    fn default() -> Self {
        Self {
            selected_tab: AppTab::Convert,
            convert: ConvertUiState::ready(),
            settings: SettingsUiState::default(),
            pending_navigation: None,
            checked_initial_config: false,
            config_recovery_error: None,
            openmw_config_error: None,
            session_openmw_cfg: None,
            localizer: Localizer::default(),
        }
    }
}

impl eframe::App for GreenmoteApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_conversion_events(ctx);
        self.check_initial_config();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::CentralPanel::default().show_inside(ui, |ui| {
            self.show_tab_bar(ui);
            ui.separator();
            self.show_active_screen(ui, &ctx);
        });

        self.show_pending_navigation_prompt(&ctx);
        self.show_openmw_config_prompt(&ctx);
        self.show_config_recovery_prompt(&ctx);
    }
}

impl GreenmoteApp {
    fn show_tab_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::selectable(
                    self.selected_tab == AppTab::Convert,
                    self.localizer.text(UiText::Convert),
                ))
                .clicked()
            {
                self.request_convert();
            }

            if ui
                .add(egui::Button::selectable(
                    self.selected_tab == AppTab::Unclip,
                    self.localizer.text(UiText::Unclip),
                ))
                .clicked()
            {
                self.request_unclip();
            }

            if ui
                .add_enabled(
                    !self.convert.is_running(),
                    egui::Button::selectable(
                        self.selected_tab == AppTab::Settings,
                        self.localizer.text(UiText::Settings),
                    ),
                )
                .clicked()
            {
                self.show_settings();
            }
        });
    }

    fn show_active_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.selected_tab {
            AppTab::Convert => self.show_convert_screen(ui, ctx),
            AppTab::Unclip => self.show_unclip_screen(ui, ctx),
            AppTab::Settings => self.show_settings_screen(ui, ctx),
        }
    }

    fn show_settings(&mut self) {
        if self.selected_tab == AppTab::Settings {
            return;
        }

        self.select_tab(AppTab::Settings);
        if !self.load_settings() {
            self.record_settings_load_failure();
        }
    }

    fn request_convert(&mut self) {
        self.request_tab(AppTab::Convert);
    }

    fn request_unclip(&mut self) {
        self.request_tab(AppTab::Unclip);
    }

    fn request_tab(&mut self, tab: AppTab) {
        if self.selected_tab == tab {
            return;
        }

        if self.selected_tab != AppTab::Settings {
            self.select_tab(tab);
            return;
        }

        self.settings.commit_active_list_edit();

        if self.settings.is_dirty() {
            self.queue_pending_navigation(PendingNavigation::Tab(tab));
        } else {
            self.select_tab(tab);
        }
    }

    fn select_tab(&mut self, tab: AppTab) {
        if self.selected_tab == tab {
            return;
        }

        if self.selected_tab == AppTab::Unclip {
            self.convert.cancel_pending_unclip_write_confirmation();
        }

        self.selected_tab = tab;
    }

    fn check_initial_config(&mut self) {
        if self.checked_initial_config {
            return;
        }

        self.checked_initial_config = true;
        if !self.load_settings() {
            if self.try_load_default_openmw_config_after_initial_failure() {
                return;
            }
            self.record_settings_load_failure();
        }
    }

    fn try_load_default_openmw_config_after_initial_failure(&mut self) -> bool {
        let Some(error) = self.settings_error().map(str::to_owned) else {
            return false;
        };
        if !is_openmw_config_settings_error(&error) {
            return false;
        }

        let Ok(path) = crate::groundcover::openmw::default_user_config_file() else {
            return false;
        };

        if self.load_settings_with_openmw_cfg(&path) {
            self.session_openmw_cfg = Some(path);
            self.openmw_config_error = None;
            self.config_recovery_error = None;
            true
        } else {
            false
        }
    }

    fn record_settings_load_failure(&mut self) {
        let Some(error) = self.settings_error().map(str::to_owned) else {
            return;
        };

        self.openmw_config_error = None;
        self.config_recovery_error = None;
        if is_openmw_config_settings_error(&error) {
            self.openmw_config_error = Some(openmw_config_error_message(&error).to_owned());
        } else {
            self.config_recovery_error = Some(error);
        }
    }

    fn queue_pending_navigation(&mut self, navigation: PendingNavigation) {
        if self.has_pending_navigation_request() {
            return;
        }

        self.pending_navigation = Some(navigation);
    }

    fn has_pending_navigation_request(&self) -> bool {
        self.pending_navigation.is_some()
    }

    fn perform_pending_dirty_navigation(&mut self) {
        let Some(navigation) = self.pending_navigation.take() else {
            return;
        };

        match navigation {
            PendingNavigation::Tab(tab) => self.select_tab(tab),
            PendingNavigation::OpenMwConfig(path) => self.apply_selected_openmw_config(&path),
        }
    }

    fn show_pending_navigation_prompt(&mut self, ctx: &egui::Context) {
        if !self.has_pending_navigation_request() {
            return;
        }

        if !self.settings.is_dirty() {
            self.perform_pending_dirty_navigation();
            return;
        }

        let mut save = false;
        let mut discard = false;
        let mut cancel = false;

        egui::Window::new(self.localizer.text(UiText::UnsavedSettingsTitle))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(self.localizer.text(UiText::UnsavedSettingsMessage));
                ui.label(self.localizer.text(UiText::SaveBeforeContinuing));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = ui.button(self.localizer.text(UiText::Save)).clicked();
                    discard = ui.button(self.localizer.text(UiText::Discard)).clicked();
                    cancel = ui.button(self.localizer.text(UiText::Cancel)).clicked();
                });
            });

        if save {
            if self.save_settings() {
                self.perform_pending_dirty_navigation();
            }
        } else if discard {
            if self.discard_settings() {
                self.perform_pending_dirty_navigation();
            }
        } else if cancel {
            self.cancel_pending_dirty_navigation();
        }
    }

    fn cancel_pending_dirty_navigation(&mut self) {
        self.pending_navigation = None;
    }

    fn show_config_recovery_prompt(&mut self, ctx: &egui::Context) {
        let Some(error) = self.config_recovery_error.clone() else {
            return;
        };

        let mut regenerate = false;
        let mut close = false;

        egui::Window::new(self.localizer.text(UiText::MalformedConfigTitle))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(self.localizer.text(UiText::MalformedConfigMessage));
                ui.label(self.localizer.text(UiText::ReplaceWithDefaults));
                ui.label(self.localizer.text(UiText::BackupBeforeReplacing));
                ui.add_space(8.0);
                ui.colored_label(ui.visuals().error_fg_color, error);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    regenerate = ui
                        .button(self.localizer.text(UiText::BackupRegenerateContinue))
                        .clicked();
                    close = ui.button(self.localizer.text(UiText::Close)).clicked();
                });
            });

        if regenerate {
            if self.regenerate_settings() {
                self.config_recovery_error = None;
            } else {
                self.config_recovery_error = self.settings_error().map(str::to_owned);
            }
        } else if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn show_openmw_config_prompt(&mut self, ctx: &egui::Context) {
        if self.openmw_config_error.is_none() {
            return;
        }

        let mut select_config = false;
        let mut close = false;
        let can_select_config = !self.convert.is_running();

        egui::Window::new(self.localizer.text(UiText::OpenMwConfigNotFoundTitle))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(self.localizer.text(UiText::OpenMwConfigNotFoundMessage));
                ui.label(
                    self.localizer
                        .text(UiText::ChooseOpenMwConfigBeforeContinuing),
                );
                ui.add_space(8.0);
                if let Some(error) = &self.openmw_config_error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    ui.add_space(8.0);
                }
                ui.horizontal(|ui| {
                    select_config = ui
                        .add_enabled(
                            can_select_config,
                            egui::Button::new(self.localizer.text(UiText::SelectOpenMwConfig)),
                        )
                        .clicked();
                    close = ui.button(self.localizer.text(UiText::Close)).clicked();
                });
            });

        if select_config {
            self.request_openmw_config_selection();
        } else if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn request_openmw_config_selection(&mut self) {
        if self.convert.is_running() {
            return;
        }

        let Some(path) = select_openmw_config_file(self.localizer) else {
            return;
        };

        self.request_openmw_config_path(path);
    }

    fn request_openmw_config_path(&mut self, path: PathBuf) {
        if self.convert.is_running() {
            return;
        }

        if self.selected_tab == AppTab::Settings {
            self.settings.commit_active_list_edit();
        }

        if self.settings.is_dirty() {
            self.queue_pending_navigation(PendingNavigation::OpenMwConfig(path));
        } else {
            self.apply_selected_openmw_config(&path);
        }
    }

    fn apply_selected_openmw_config(&mut self, path: &Path) {
        if self.load_settings_with_openmw_cfg(path) {
            self.session_openmw_cfg = Some(path.to_owned());
            self.openmw_config_error = None;
            self.config_recovery_error = None;
        } else {
            if self
                .settings_error()
                .is_some_and(|error| !is_openmw_config_settings_error(error))
            {
                self.session_openmw_cfg = Some(path.to_owned());
            }
            self.record_settings_load_failure();
        }
    }
}

fn language_label(localizer: Localizer, language: UiLanguage) -> &'static str {
    match language {
        UiLanguage::English => localizer.text(UiText::EnglishLanguage),
        UiLanguage::French => localizer.text(UiText::FrenchLanguage),
        UiLanguage::German => localizer.text(UiText::GermanLanguage),
        UiLanguage::Russian => localizer.text(UiText::RussianLanguage),
        UiLanguage::Spanish => localizer.text(UiText::SpanishLanguage),
        UiLanguage::Swedish => localizer.text(UiText::SwedishLanguage),
    }
}

fn select_openmw_config_file(localizer: Localizer) -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new()
        .set_title(localizer.text(UiText::SelectOpenMwConfig))
        .add_filter(localizer.text(UiText::OpenMwConfig), &["cfg"])
        .set_file_name("openmw.cfg");

    let dialog = match std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_owned))
    {
        Some(directory) => dialog.set_directory(directory),
        None => dialog,
    };

    dialog.pick_file()
}

fn is_openmw_config_settings_error(error: &str) -> bool {
    openmw_config_error_message(error).contains("failed to read OpenMW configuration")
        || error.contains("explicit --openmw-cfg path")
        || error.contains("OpenMW root config discovery")
}

fn openmw_config_error_message(error: &str) -> &str {
    error
        .strip_prefix("Failed to load settings: ")
        .unwrap_or(error)
}

/// Runs the `greenmote` graphical user interface.
///
/// # Errors
///
/// Returns an I/O-shaped error when the underlying GUI platform cannot create or run the window.
pub fn run() -> io::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([720.0, 480.0]),
        ..eframe::NativeOptions::default()
    };

    eframe::run_native(
        "Greenmote",
        native_options,
        Box::new(|_creation_context| Ok(Box::new(GreenmoteApp::default()))),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{
        AppTab, GreenmoteApp, PendingNavigation, is_openmw_config_settings_error,
        openmw_config_error_message,
    };

    fn pending_tab(app: &GreenmoteApp) -> Option<AppTab> {
        match &app.pending_navigation {
            Some(PendingNavigation::Tab(tab)) => Some(*tab),
            Some(PendingNavigation::OpenMwConfig(_)) | None => None,
        }
    }

    #[test]
    fn openmw_config_errors_are_not_malformed_greenmote_config_errors() {
        assert!(is_openmw_config_settings_error(
            "Failed to load settings: failed to read OpenMW configuration: OpenMW root config discovery found no openmw.cfg"
        ));
        assert!(is_openmw_config_settings_error(
            "Failed to load settings: explicit --openmw-cfg path missing is neither a file nor a directory containing openmw.cfg"
        ));
    }

    #[test]
    fn toml_errors_remain_malformed_greenmote_config_errors() {
        assert!(!is_openmw_config_settings_error(
            "Failed to load settings: TOML parse error at line 1, column 1"
        ));
    }

    #[test]
    fn openmw_config_dialog_removes_settings_load_wrapper() {
        assert_eq!(
            openmw_config_error_message(
                "Failed to load settings: failed to read OpenMW configuration: missing openmw.cfg"
            ),
            "failed to read OpenMW configuration: missing openmw.cfg"
        );
    }

    #[test]
    fn dirty_settings_convert_tab_navigation_targets_convert_after_resolution() {
        let mut app = GreenmoteApp {
            selected_tab: AppTab::Settings,
            ..GreenmoteApp::default()
        };
        app.settings.set_dirty_for_test(true);

        app.request_convert();

        assert_eq!(app.selected_tab, AppTab::Settings);
        assert_eq!(pending_tab(&app), Some(AppTab::Convert));

        app.settings.set_dirty_for_test(false);
        app.perform_pending_dirty_navigation();

        assert_eq!(app.selected_tab, AppTab::Convert);
        assert!(app.pending_navigation.is_none());
    }

    #[test]
    fn dirty_settings_unclip_tab_navigation_targets_unclip_after_resolution() {
        let mut app = GreenmoteApp {
            selected_tab: AppTab::Settings,
            ..GreenmoteApp::default()
        };
        app.settings.set_dirty_for_test(true);

        app.request_unclip();

        assert_eq!(app.selected_tab, AppTab::Settings);
        assert_eq!(pending_tab(&app), Some(AppTab::Unclip));

        app.settings.set_dirty_for_test(false);
        app.perform_pending_dirty_navigation();

        assert_eq!(app.selected_tab, AppTab::Unclip);
        assert!(app.pending_navigation.is_none());
    }

    #[test]
    fn cancel_dirty_settings_tab_navigation_stays_on_settings_and_clears_target() {
        let mut app = GreenmoteApp {
            selected_tab: AppTab::Settings,
            ..GreenmoteApp::default()
        };
        app.settings.set_dirty_for_test(true);

        app.request_unclip();
        app.cancel_pending_dirty_navigation();

        assert_eq!(app.selected_tab, AppTab::Settings);
        assert!(app.pending_navigation.is_none());
    }
}
