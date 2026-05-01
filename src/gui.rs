use std::{
    io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use eframe::egui;

use crate::groundcover::{
    self, ConversionEvent, ConversionPhase, GroundcoverArgs, GroundcoverConfig,
};

const MAX_EVENTS_PER_FRAME: usize = 256;

struct GreenmoteApp {
    screen: Screen,
    convert: ConvertUiState,
    settings: SettingsUiState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Convert,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
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

struct SettingsUiState {
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
struct SettingsDraft {
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
            settings: SettingsUiState::default(),
        }
    }
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

impl eframe::App for GreenmoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive_conversion_events(ctx);

        egui::SidePanel::left("greenmote_convert_panel")
            .resizable(false)
            .exact_width(ctx.viewport_rect().width() / 3.0)
            .show(ctx, |ui| {
                self.show_sidebar(ui);
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            self.show_active_screen(ui, ctx);
        });
    }
}

impl GreenmoteApp {
    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(12.0);
        ui.vertical_centered_justified(|ui| {
            if ui
                .add(egui::Button::new("Convert").selected(self.screen == Screen::Convert))
                .clicked()
            {
                self.screen = Screen::Convert;
            }

            if ui
                .add(egui::Button::new("Settings").selected(self.screen == Screen::Settings))
                .clicked()
            {
                self.screen = Screen::Settings;
                self.load_settings();
            }
        });
    }

    fn show_active_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.screen {
            Screen::Convert => self.show_convert_screen(ui, ctx),
            Screen::Settings => self.show_settings_screen(ui),
        }
    }

    fn show_convert_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Convert");
        if self.convert.progress.is_some() {
            self.show_progress(ui);
        } else {
            ui.label(&self.convert.status);
        }
        ui.add_space(8.0);
        let can_start = !self.convert.running && !self.settings.dirty;
        if ui
            .add_enabled(can_start, egui::Button::new("Start conversion"))
            .clicked()
        {
            self.start_conversion(ctx);
        }
        if self.settings.dirty {
            ui.label("Save Settings changes before converting.");
        }
        ui.separator();

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.monospace(self.convert.output.as_str());
            });
    }

    fn show_settings_screen(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.horizontal(|ui| {
            ui.selectable_value(
                &mut self.settings.selected_tab,
                SettingsTab::General,
                "General",
            );
            ui.selectable_value(
                &mut self.settings.selected_tab,
                SettingsTab::Convert,
                "Convert",
            );
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

    fn load_settings(&mut self) {
        if self.settings.loaded {
            return;
        }

        match groundcover::load_config_for_edit(&GroundcoverArgs::default()) {
            Ok((path, config)) => {
                self.settings.draft = SettingsDraft::from_config(&config);
                self.settings.config_path = Some(path.clone());
                self.settings.loaded = true;
                self.settings.dirty = false;
                self.settings.status = format!("Loaded {}", path.display());
                self.settings.error = None;
            }
            Err(error) => {
                self.settings.loaded = false;
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to load settings: {error}"));
            }
        }
    }

    fn save_settings(&mut self) {
        let Some(path) = self.settings.config_path.clone() else {
            self.settings.error = Some("No greenmote.toml path is available.".to_owned());
            return;
        };

        let config = self.settings.draft.to_config();
        match groundcover::save_config_for_edit(&config, &path) {
            Ok(config) => {
                self.settings.draft = SettingsDraft::from_config(&config);
                self.settings.dirty = false;
                self.settings.status = format!("Saved {}", path.display());
                self.settings.error = None;
            }
            Err(error) => {
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to save settings: {error}"));
            }
        }
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let sink = GuiEventSink::new(sender, ctx.clone());

        self.convert.running = true;
        self.set_status("Converting with saved settings...");
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
                let current = self.monotonic_current(phase, current, total);
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

    fn monotonic_current(&self, phase: ConversionPhase, current: usize, total: usize) -> usize {
        let Some(progress) = &self.convert.progress else {
            return current;
        };

        if progress.phase != phase {
            return current;
        }

        match progress.progress {
            ProgressKind::Counted {
                current: previous,
                total: previous_total,
            } if previous_total == total => current.max(previous),
            ProgressKind::Indeterminate | ProgressKind::Counted { .. } => current,
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
