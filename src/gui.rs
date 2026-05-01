use std::{cell::RefCell, io, rc::Rc, sync::mpsc, thread};

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
    output: String,
    result_receiver: Option<mpsc::Receiver<ConversionResult>>,
}

struct ConversionResult {
    output: String,
    error: Option<String>,
}

#[derive(Clone)]
struct SharedOutput {
    bytes: Rc<RefCell<Vec<u8>>>,
}

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
        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.monospace(self.convert.output.as_str());
            });
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let repaint_context = ctx.clone();

        self.convert.running = true;
        self.set_status("Converting with default settings...");
        self.convert.result_receiver = Some(receiver);
        self.convert.output.clear();

        thread::spawn(move || {
            let bytes = Rc::new(RefCell::new(Vec::new()));
            let mut stdout = SharedOutput::new(bytes.clone());
            let mut stderr = SharedOutput::new(bytes.clone());
            let error =
                groundcover::run_with_output(GroundcoverArgs::default(), &mut stdout, &mut stderr)
                    .err()
                    .map(|error| error.to_string());

            let result = ConversionResult {
                output: String::from_utf8_lossy(&bytes.borrow()).into_owned(),
                error,
            };

            let _send_result = sender.send(result);
            repaint_context.request_repaint();
        });
    }

    fn receive_conversion_result(&mut self) {
        let Some(receiver) = self.convert.result_receiver.take() else {
            return;
        };

        match receiver.try_recv() {
            Ok(result) => {
                self.convert.running = false;
                self.convert.output = format_conversion_output(&result);

                if let Some(error) = result.error {
                    self.set_status(format!("Conversion failed: {error}"));
                } else {
                    self.set_status("Conversion finished.");
                }
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
    }
}

impl SharedOutput {
    fn new(bytes: Rc<RefCell<Vec<u8>>>) -> Self {
        Self { bytes }
    }
}

impl io::Write for SharedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.bytes.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn format_conversion_output(result: &ConversionResult) -> String {
    let mut output = result.output.clone();

    if let Some(error) = &result.error {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("error:\n");
        output.push_str(error);
        output.push('\n');
    }

    output
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
