use std::{
    io,
    path::{Path, PathBuf},
};

use eframe::egui;

mod convert;
mod nav;
mod run_options;
mod settings;

use convert::ConvertUiState;
use nav::{NavUiState, nav_bar_height};
use run_options::ConvertRunOptions;
use settings::{SettingsTab, SettingsUiState};

struct GreenmoteApp {
    screen: Screen,
    convert: ConvertUiState,
    settings: SettingsUiState,
    nav: NavUiState,
    pending_navigation: Option<PendingNavigation>,
    checked_initial_config: bool,
    config_recovery_error: Option<String>,
    openmw_config_error: Option<String>,
    session_openmw_cfg: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Convert,
    Settings,
}

enum PendingNavigation {
    Screen(Screen),
    SettingsTab(SettingsTab),
    OpenMwConfig(PathBuf),
}

impl Default for GreenmoteApp {
    fn default() -> Self {
        Self {
            screen: Screen::Convert,
            convert: ConvertUiState::ready(),
            settings: SettingsUiState::default(),
            nav: NavUiState::default(),
            pending_navigation: None,
            checked_initial_config: false,
            config_recovery_error: None,
            openmw_config_error: None,
            session_openmw_cfg: None,
        }
    }
}

impl eframe::App for GreenmoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_conversion_events(ctx);
        self.check_initial_config();

        egui::TopBottomPanel::top("greenmote_navigation_bar")
            .resizable(false)
            .exact_height(nav_bar_height(ctx))
            .show(ctx, |ui| {
                self.show_navigation_bar(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_active_screen(ui, ctx);
        });

        self.show_pending_navigation_prompt(ctx);
        self.show_openmw_config_prompt(ctx);
        self.show_config_recovery_prompt(ctx);
    }
}

impl GreenmoteApp {
    fn show_active_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.screen {
            Screen::Convert => self.show_convert_screen(ui, ctx),
            Screen::Settings => self.show_settings_screen(ui),
        }
    }

    fn request_screen(&mut self, screen: Screen) {
        if self.screen == screen {
            return;
        }

        if self.settings.is_dirty() {
            self.queue_pending_navigation(PendingNavigation::Screen(screen));
        } else {
            self.show_screen(screen);
        }
    }

    fn show_screen(&mut self, screen: Screen) {
        self.screen = screen;
        if screen == Screen::Settings && !self.load_settings() {
            self.record_settings_load_failure();
        }
    }

    fn check_initial_config(&mut self) {
        if self.checked_initial_config {
            return;
        }

        self.checked_initial_config = true;
        if !self.load_settings() {
            self.record_settings_load_failure();
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
            PendingNavigation::Screen(screen) => self.show_screen(screen),
            PendingNavigation::SettingsTab(tab) => self.settings.select_tab(tab),
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

        egui::Window::new("Unsaved settings")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Settings have unsaved changes.");
                ui.label("Save them before continuing?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    save = ui.button("Save").clicked();
                    discard = ui.button("Discard").clicked();
                    cancel = ui.button("Cancel").clicked();
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
            self.pending_navigation = None;
        }
    }

    fn show_config_recovery_prompt(&mut self, ctx: &egui::Context) {
        let Some(error) = self.config_recovery_error.clone() else {
            return;
        };

        let mut regenerate = false;
        let mut close = false;

        egui::Window::new("Malformed config")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("greenmote.toml could not be loaded.");
                ui.label("Replace it with defaults before continuing.");
                ui.label("The current file will be moved aside as a .bak file first.");
                ui.add_space(8.0);
                ui.colored_label(ui.visuals().error_fg_color, error);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    regenerate = ui.button("Back up, regenerate, and continue").clicked();
                    close = ui.button("Close").clicked();
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

        let default_config = crate::groundcover::openmw::default_user_config_file();
        let mut use_default = false;
        let mut select_config = false;
        let mut close = false;
        let can_select_config = !self.convert.is_running();

        egui::Window::new("OpenMW config not found")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label("Greenmote could not find or load an OpenMW configuration file.");
                ui.label("Choose a valid OpenMW config path before continuing.");
                ui.add_space(8.0);
                match &default_config {
                    Ok(path) => {
                        ui.label("Default OpenMW user config path:");
                        ui.monospace(path.display().to_string());
                    }
                    Err(path_error) => {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("Could not determine default OpenMW config path: {path_error}"),
                        );
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    use_default = ui
                        .add_enabled(
                            default_config.is_ok() && can_select_config,
                            egui::Button::new("Use default path"),
                        )
                        .clicked();
                    select_config = ui
                        .add_enabled(can_select_config, egui::Button::new("Select OpenMW Config"))
                        .clicked();
                    close = ui.button("Close").clicked();
                });
            });

        if use_default {
            match default_config {
                Ok(path) => {
                    self.apply_selected_openmw_config(&path);
                }
                Err(error) => {
                    self.openmw_config_error = Some(format!(
                        "Failed to determine default OpenMW config path: {error}"
                    ));
                }
            }
        } else if select_config {
            self.request_openmw_config_selection();
        } else if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn request_openmw_config_selection(&mut self) {
        if self.convert.is_running() {
            return;
        }

        let Some(path) = select_openmw_config_file() else {
            return;
        };

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

fn select_openmw_config_file() -> Option<PathBuf> {
    let dialog = rfd::FileDialog::new()
        .set_title("Select OpenMW Config")
        .add_filter("OpenMW config", &["cfg"])
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
    use super::{is_openmw_config_settings_error, openmw_config_error_message};

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
}
