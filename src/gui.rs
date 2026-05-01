use std::{io, sync::mpsc, thread};

use eframe::egui;

use crate::groundcover::{self, GroundcoverArgs};

struct GreenmoteApp {
    screen: Screen,
    convert: ConvertUiState,
    messages: MessageLog,
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

#[derive(Default)]
struct MessageLog {
    text: String,
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
            messages: MessageLog::default(),
        }
    }
}

impl eframe::App for GreenmoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_conversion_result();

        egui::TopBottomPanel::bottom("greenmote_message_log")
            .resizable(false)
            .exact_height(self.messages.panel_height(ctx))
            .show(ctx, |ui| {
                self.messages.show(ui);
            });

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
        self.set_status("Converting with default settings...");
        self.convert.result_receiver = Some(receiver);
        self.messages
            .reset("Started conversion with default settings.");

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
                self.set_status("Conversion finished.");
            }
            Ok(Err(error)) => {
                self.convert.running = false;
                self.set_status(format!("Conversion failed: {error}"));
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.convert.result_receiver = Some(receiver);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.convert.running = false;
                self.set_status("Conversion worker disconnected.");
            }
        }
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.convert.status = status.into();
        self.messages.reset(self.convert.status.as_str());
    }
}

impl MessageLog {
    const LINE_HEIGHT: f32 = 18.0;
    const VERTICAL_PADDING: f32 = 12.0;
    const MIN_HEIGHT: f32 = 28.0;
    const MAX_WINDOW_FRACTION: f32 = 0.1;

    fn reset(&mut self, message: &str) {
        self.text.clear();
        self.text.push_str(message);
    }

    fn panel_height(&self, ctx: &egui::Context) -> f32 {
        let line_count = u16::try_from(self.text.lines().count().max(1)).unwrap_or(u16::MAX);
        let line_count = f32::from(line_count);
        let content_height = line_count.mul_add(Self::LINE_HEIGHT, Self::VERTICAL_PADDING);
        let max_height = ctx.viewport_rect().height() * Self::MAX_WINDOW_FRACTION;

        content_height.min(max_height).max(Self::MIN_HEIGHT)
    }

    fn show(&self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.label(self.text.as_str());
            });
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
