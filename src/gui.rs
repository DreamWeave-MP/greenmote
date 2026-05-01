use std::io;

use eframe::egui;

mod convert;
mod nav;
mod settings;

use convert::ConvertUiState;
use nav::{NavUiState, nav_bar_height};
use settings::{PendingSettingsAction, SettingsUiState};

struct GreenmoteApp {
    screen: Screen,
    convert: ConvertUiState,
    settings: SettingsUiState,
    nav: NavUiState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Convert,
    Settings,
}

impl Default for GreenmoteApp {
    fn default() -> Self {
        Self {
            screen: Screen::Convert,
            convert: ConvertUiState::ready(),
            settings: SettingsUiState::default(),
            nav: NavUiState::default(),
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

        self.show_pending_settings_prompt(ctx);
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

        if self.settings.dirty {
            self.settings.pending_action = Some(PendingSettingsAction::ShowScreen(screen));
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
