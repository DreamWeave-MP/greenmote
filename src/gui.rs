use std::io;

use eframe::egui;

mod convert;
mod nav;
mod settings;

use convert::ConvertUiState;
use nav::{NavUiState, nav_bar_height};
use settings::{SettingsTab, SettingsUiState};

struct GreenmoteApp {
    screen: Screen,
    convert: ConvertUiState,
    settings: SettingsUiState,
    nav: NavUiState,
    pending_navigation: Option<PendingNavigation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Convert,
    Settings,
}

#[derive(Clone, Copy)]
enum PendingNavigation {
    Screen(Screen),
    SettingsTab(SettingsTab),
}

impl Default for GreenmoteApp {
    fn default() -> Self {
        Self {
            screen: Screen::Convert,
            convert: ConvertUiState::ready(),
            settings: SettingsUiState::default(),
            nav: NavUiState::default(),
            pending_navigation: None,
        }
    }
}

impl eframe::App for GreenmoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_conversion_events(ctx);

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
        if screen == Screen::Settings {
            self.load_settings();
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
