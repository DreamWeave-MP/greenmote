use std::{io, sync::mpsc, thread};

use eframe::egui;

use crate::groundcover::{self, GroundcoverArgs};

struct GreenmoteApp {
    screen: Screen,
    convert: ConvertUiState,
}

enum Screen {
    Convert,
}

#[derive(Default)]
struct ConvertUiState {
    running: bool,
    status: String,
    result_receiver: Option<mpsc::Receiver<ConversionResult>>,
}

type ConversionResult = Result<(), String>;

impl Default for GreenmoteApp {
    fn default() -> Self {
        Self {
            screen: Screen::Convert,
            convert: ConvertUiState {
                status: "Ready.".to_owned(),
                ..ConvertUiState::default()
            },
        }
    }
}

impl eframe::App for GreenmoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_conversion_result();

        egui::SidePanel::left("greenmote_convert_panel")
            .resizable(false)
            .exact_width(ctx.viewport_rect().width() / 3.0)
            .show(ctx, |ui| {
                self.show_sidebar(ui, ctx);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_active_screen(ui);
        });
    }
}

impl GreenmoteApp {
    fn show_sidebar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(12.0);
        ui.vertical_centered_justified(|ui| {
            let convert_button =
                ui.add_enabled(!self.convert.running, egui::Button::new("Convert"));
            if convert_button.clicked() {
                self.start_conversion(ctx);
            }
        });
    }

    fn show_active_screen(&self, ui: &mut egui::Ui) {
        match self.screen {
            Screen::Convert => self.show_convert_screen(ui),
        }
    }

    fn show_convert_screen(&self, ui: &mut egui::Ui) {
        ui.heading("Convert");
        ui.label(&self.convert.status);
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let repaint_context = ctx.clone();

        self.convert.running = true;
        "Converting with default settings...".clone_into(&mut self.convert.status);
        self.convert.result_receiver = Some(receiver);

        thread::spawn(move || {
            let result =
                groundcover::run(GroundcoverArgs::default()).map_err(|error| error.to_string());
            let _send_result = sender.send(result);
            repaint_context.request_repaint();
        });
    }

    fn receive_conversion_result(&mut self) {
        let Some(receiver) = self.convert.result_receiver.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(Ok(())) => {
                self.convert.running = false;
                "Conversion finished.".clone_into(&mut self.convert.status);
            }
            Ok(Err(error)) => {
                self.convert.running = false;
                self.convert.status = format!("Conversion failed: {error}");
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.convert.result_receiver = Some(receiver);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.convert.running = false;
                "Conversion worker disconnected.".clone_into(&mut self.convert.status);
            }
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
