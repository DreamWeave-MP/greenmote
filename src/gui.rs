use std::{
    io,
    sync::{Arc, Mutex, mpsc},
    thread,
};

use eframe::egui;

use crate::groundcover::{self, ConversionEvent, ConversionPhase, GroundcoverArgs};

const MAX_EVENTS_PER_FRAME: usize = 256;

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
    progress: Option<ProgressState>,
    event_receiver: Option<mpsc::Receiver<GuiEvent>>,
}

enum GuiEvent {
    Output(String),
    Progress(ConversionEvent),
    Finished { error: Option<String> },
}

#[derive(Clone)]
struct GuiEventSink {
    sender: Arc<Mutex<mpsc::Sender<GuiEvent>>>,
    context: egui::Context,
}

struct GuiOutput {
    sink: GuiEventSink,
}

struct ProgressState {
    phase: ConversionPhase,
    progress: ProgressKind,
}

enum ProgressKind {
    Indeterminate,
    Counted { current: usize, total: usize },
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
        self.receive_conversion_events(ctx);

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
        if self.convert.progress.is_some() {
            self.show_progress(ui);
        } else {
            ui.label(&self.convert.status);
        }
        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.monospace(self.convert.output.as_str());
            });
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let sink = GuiEventSink::new(sender, ctx.clone());

        self.convert.running = true;
        self.set_status("Converting with default settings...");
        self.convert.progress = None;
        self.convert.event_receiver = Some(receiver);
        self.convert.output.clear();

        thread::spawn(move || {
            let mut stdout = GuiOutput::new(sink.clone());
            let mut stderr = GuiOutput::new(sink.clone());
            let progress_sink = sink.clone();
            let error = groundcover::run_with_output_and_events(
                GroundcoverArgs::default(),
                &mut stdout,
                &mut stderr,
                &move |event| progress_sink.send(GuiEvent::Progress(event)),
            )
            .err()
            .map(|error| error.to_string());

            sink.send(GuiEvent::Finished { error });
        });
    }

    fn receive_conversion_events(&mut self, ctx: &egui::Context) {
        let Some(receiver) = self.convert.event_receiver.take() else {
            return;
        };

        let mut pending_progress = PendingProgress::default();
        let mut processed = 0;

        loop {
            if processed >= MAX_EVENTS_PER_FRAME {
                if self.convert.running {
                    self.convert.event_receiver = Some(receiver);
                }
                self.apply_pending_progress(pending_progress.take());
                ctx.request_repaint();
                return;
            }

            match receiver.try_recv() {
                Ok(GuiEvent::Progress(event)) => {
                    pending_progress.merge(event);
                    processed += 1;
                }
                Ok(event) => {
                    self.apply_pending_progress(pending_progress.take());
                    self.handle_gui_event(event);
                    processed += 1;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if self.convert.running {
                        self.convert.event_receiver = Some(receiver);
                    }
                    self.apply_pending_progress(pending_progress.take());
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.convert.running = false;
                    self.set_status("Conversion worker disconnected.");
                    self.apply_pending_progress(pending_progress.take());
                    return;
                }
            }
        }
    }

    fn handle_gui_event(&mut self, event: GuiEvent) {
        match event {
            GuiEvent::Output(output) => self.convert.output.push_str(&output),
            GuiEvent::Progress(event) => self.handle_progress_event(event),
            GuiEvent::Finished { error } => self.finish_conversion(error),
        }
    }

    fn handle_progress_event(&mut self, event: ConversionEvent) {
        match event {
            ConversionEvent::PhaseStarted(phase) => {
                self.convert.progress = Some(ProgressState {
                    phase,
                    progress: ProgressKind::Indeterminate,
                });
                self.set_status(phase.label());
            }
            ConversionEvent::Progress {
                phase,
                current,
                total,
            } => {
                let current = self.monotonic_current(phase, current);
                self.convert.progress = Some(ProgressState {
                    phase,
                    progress: ProgressKind::Counted { current, total },
                });
                self.set_status(phase.label());
            }
        }
    }

    fn apply_pending_progress(&mut self, event: Option<ConversionEvent>) {
        if let Some(event) = event {
            self.handle_progress_event(event);
        }
    }

    fn monotonic_current(&self, phase: ConversionPhase, current: usize) -> usize {
        let Some(progress) = &self.convert.progress else {
            return current;
        };

        if progress.phase != phase {
            return current;
        }

        match progress.progress {
            ProgressKind::Indeterminate => current,
            ProgressKind::Counted {
                current: previous, ..
            } => current.max(previous),
        }
    }

    fn finish_conversion(&mut self, error: Option<String>) {
        self.convert.running = false;
        self.convert.progress = None;

        if let Some(error) = error {
            append_error(&mut self.convert.output, &error);
            self.set_status(format!("Conversion failed: {error}"));
        } else {
            self.set_status("Conversion finished.");
        }
    }

    fn show_progress(&self, ui: &mut egui::Ui) {
        let Some(progress) = &self.convert.progress else {
            return;
        };

        ui.label(progress.label());
        ui.add(progress.progress_bar());
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.convert.status = status.into();
    }
}

#[derive(Default)]
struct PendingProgress {
    event: Option<ConversionEvent>,
}

impl PendingProgress {
    fn merge(&mut self, event: ConversionEvent) {
        self.event = match (self.event, event) {
            (
                Some(ConversionEvent::Progress {
                    phase: old_phase,
                    current: old_current,
                    total: old_total,
                }),
                ConversionEvent::Progress {
                    phase,
                    current,
                    total,
                },
            ) if old_phase == phase && old_total == total => Some(ConversionEvent::Progress {
                phase,
                current: current.max(old_current),
                total,
            }),
            (_, event) => Some(event),
        };
    }

    fn take(&mut self) -> Option<ConversionEvent> {
        self.event.take()
    }
}

impl GuiEventSink {
    fn new(sender: mpsc::Sender<GuiEvent>, context: egui::Context) -> Self {
        Self {
            sender: Arc::new(Mutex::new(sender)),
            context,
        }
    }

    fn send(&self, event: GuiEvent) {
        if let Ok(sender) = self.sender.lock() {
            let _send_result = sender.send(event);
            self.context.request_repaint();
        }
    }
}

impl GuiOutput {
    fn new(sink: GuiEventSink) -> Self {
        Self { sink }
    }
}

impl io::Write for GuiOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.sink.send(GuiEvent::Output(
            String::from_utf8_lossy(bytes).into_owned(),
        ));
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl ProgressState {
    fn label(&self) -> String {
        match self.progress {
            ProgressKind::Indeterminate => self.phase.label().to_owned(),
            ProgressKind::Counted { current, total } => {
                format!("{} ({current}/{total})", self.phase.label())
            }
        }
    }

    fn progress_bar(&self) -> egui::ProgressBar {
        match self.progress {
            ProgressKind::Indeterminate => egui::ProgressBar::new(0.0)
                .animate(true)
                .desired_width(f32::INFINITY),
            ProgressKind::Counted { current, total } => {
                egui::ProgressBar::new(progress_fraction(current, total))
                    .show_percentage()
                    .desired_width(f32::INFINITY)
            }
        }
    }
}

fn append_error(output: &mut String, error: &str) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str("error:\n");
    output.push_str(error);
    output.push('\n');
}

fn progress_fraction(current: usize, total: usize) -> f32 {
    if total == 0 {
        return 1.0;
    }

    let current = u128::try_from(current.min(total)).unwrap_or(u128::MAX);
    let total = u128::try_from(total).unwrap_or(u128::MAX).max(1);
    let basis_points = u16::try_from((current * 10_000) / total).unwrap_or(10_000);

    f32::from(basis_points) / 10_000.0
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
