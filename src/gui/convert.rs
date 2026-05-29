use std::{ffi::OsString, io, path::Path, process::Command, sync::mpsc, thread};

use eframe::egui;

use crate::groundcover::{self, CancellationToken, ConversionEvent, ConversionPhase};

use super::{ConvertRunOptions, GreenmoteApp};

const MAX_EVENTS_PER_FRAME: usize = 256;
const MIN_WIDGET_SIZE: f32 = 1.0;

#[derive(Default)]
pub(super) struct ConvertUiState {
    running: bool,
    status: String,
    output: String,
    cancelling: bool,
    progress: Option<ProgressState>,
    phase_reached: Option<ConversionPhase>,
    event_receiver: Option<mpsc::Receiver<GuiEvent>>,
    cancellation: Option<CancellationToken>,
    run_options: ConvertRunOptions,
    saved_run_options: ConvertRunOptions,
}

enum GuiEvent {
    Output(String),
    Progress(ConversionEvent),
    Finished {
        error: Option<String>,
        cancelled: bool,
    },
}

#[derive(Clone)]
struct GuiEventSink {
    sender: mpsc::Sender<GuiEvent>,
    context: egui::Context,
}

struct GuiOutput {
    sink: GuiEventSink,
}

struct ProgressState {
    phase: ConversionPhase,
    progress: ProgressKind,
}

struct CancellationNotice {
    status: &'static str,
    output: &'static str,
}

impl CancellationNotice {
    const fn before_output() -> Self {
        Self {
            status: "Conversion cancelled. No generated files or OpenMW config were updated.",
            output: "Conversion cancelled. No generated files or OpenMW config were updated.",
        }
    }

    const fn output_side_effects() -> Self {
        Self {
            status: "Conversion cancelled. Generated files or copied meshes may have been updated.",
            output: "Conversion cancelled. Generated files or copied meshes may have been updated. OpenMW config was not edited unless auto-enable had already started.",
        }
    }

    const fn config_side_effects() -> Self {
        Self {
            status: "Conversion cancelled. Generated files and OpenMW config may have been updated.",
            output: "Conversion cancelled. Generated files and OpenMW config may have been updated. See greenmote.log for the durable cancellation marker if output writing had started.",
        }
    }
}

enum ProgressKind {
    Indeterminate,
    Counted { current: usize, total: usize },
}

#[derive(Default)]
struct PendingProgress {
    event: Option<ConversionEvent>,
}

impl ConvertUiState {
    pub(super) fn ready() -> Self {
        Self {
            status: "Ready.".to_owned(),
            ..Self::default()
        }
    }

    pub(super) fn sync_run_options(&mut self, options: ConvertRunOptions) {
        self.run_options = options;
        self.saved_run_options = options;
    }

    pub(super) fn current_run_options(&self) -> ConvertRunOptions {
        self.run_options
    }

    pub(super) fn is_running(&self) -> bool {
        self.running
    }

    pub(super) fn mark_run_options_saved(&mut self, options: ConvertRunOptions) {
        self.saved_run_options = options;
    }

    pub(super) fn sync_saved_run_options_from_settings(&mut self, options: ConvertRunOptions) {
        let saved = self.saved_run_options;

        if self.run_options.dry_run == saved.dry_run || options.dry_run != saved.dry_run {
            self.run_options.dry_run = options.dry_run;
        }
        if self.run_options.debug == saved.debug || options.debug != saved.debug {
            self.run_options.debug = options.debug;
        }
        if self.run_options.auto_enable == saved.auto_enable
            || options.auto_enable != saved.auto_enable
        {
            self.run_options.auto_enable = options.auto_enable;
        }

        self.saved_run_options = options;
    }

    fn run_options_differ_from_saved(&self) -> bool {
        self.run_options != self.saved_run_options
    }
}

impl GreenmoteApp {
    pub(super) fn show_convert_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Convert");
        if self.convert.progress.is_some() {
            self.show_progress(ui);
        } else {
            ui.label(&self.convert.status);
        }
        ui.add_space(8.0);

        self.show_convert_run_options(ui);

        ui.add_space(8.0);
        let can_start = !self.convert.running
            && !self.settings.is_dirty()
            && self.config_recovery_error.is_none()
            && self.openmw_config_error.is_none();
        if ui
            .add_enabled(can_start, egui::Button::new("Start conversion"))
            .clicked()
        {
            self.start_conversion(ctx);
        }
        if self.settings.is_dirty() {
            ui.label("Save Settings changes before converting.");
        }
        if self.config_recovery_error.is_some() {
            ui.label("Regenerate Settings before converting.");
        }
        if self.openmw_config_error.is_some() {
            ui.label("Choose an OpenMW config before converting.");
        }
        ui.separator();

        egui::TopBottomPanel::bottom("convert_output_actions")
            .resizable(false)
            .show_separator_line(true)
            .show_inside(ui, |ui| {
                self.show_convert_output_actions(ui, ctx);
            });

        let output_size = finite_widget_size(ui.available_size());
        ui.allocate_ui_with_layout(
            output_size,
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                let output_width = finite_widget_extent(ui.available_width());
                let output_height = finite_widget_extent(ui.available_height());
                egui::ScrollArea::vertical()
                    .max_width(output_width)
                    .max_height(output_height)
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(output_width);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(self.convert.output.as_str()).monospace(),
                            )
                            .wrap_mode(egui::TextWrapMode::Wrap)
                            .selectable(false),
                        );
                    });
            },
        );
    }

    fn show_convert_run_options(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Run options").strong());
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!self.convert.running, |ui| {
                    let mut dry_run = self.convert.run_options.dry_run;
                    if ui.checkbox(&mut dry_run, "Dry run").changed() {
                        self.convert.run_options.set_dry_run(dry_run);
                    }

                    let mut debug = self.convert.run_options.debug;
                    if ui.checkbox(&mut debug, "Debug diagnostics").changed() {
                        self.convert.run_options.set_debug(debug);
                    }

                    ui.add_enabled(
                        self.convert.run_options.can_edit_auto_enable(),
                        egui::Checkbox::new(
                            &mut self.convert.run_options.auto_enable,
                            "Auto-enable generated plugins",
                        ),
                    );
                });
            });

            let differs = self.convert.run_options_differ_from_saved();
            ui.horizontal_wrapped(|ui| {
                let can_save = differs
                    && !self.convert.running
                    && !self.settings.is_dirty()
                    && self.config_recovery_error.is_none();
                if ui
                    .add_enabled(can_save, egui::Button::new("Save as defaults"))
                    .clicked()
                {
                    let _saved = self.save_convert_run_options_as_defaults();
                }

                if ui
                    .add_enabled(
                        differs && !self.convert.running,
                        egui::Button::new("Reset from saved"),
                    )
                    .clicked()
                {
                    self.convert.run_options = self.convert.saved_run_options;
                    self.set_status("Reset run options from saved settings.");
                }

                if differs {
                    ui.label("Run options differ from saved defaults.");
                } else {
                    ui.label("Run options match saved defaults.");
                }
            });

            if self.settings.is_dirty() && differs {
                ui.label("Save or discard Settings changes before saving these as defaults.");
            }
        });
    }

    fn show_convert_output_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(ui.spacing().item_spacing.y);
        ui.horizontal_wrapped(|ui| {
            let actions_enabled = !self.convert.running;
            if ui
                .add_enabled(
                    self.convert.running && !self.convert.cancelling,
                    egui::Button::new("Cancel"),
                )
                .clicked()
            {
                self.cancel_conversion();
            }

            if ui
                .add_enabled(
                    actions_enabled && !self.convert.output.is_empty(),
                    egui::Button::new("Clear output"),
                )
                .clicked()
            {
                self.convert.output.clear();
                self.set_status("Output cleared.");
            }

            if ui
                .add_enabled(
                    actions_enabled && !self.convert.output.is_empty(),
                    egui::Button::new("Copy output"),
                )
                .clicked()
            {
                ctx.copy_text(self.convert.output.clone());
                self.set_status("Copied output to clipboard.");
            }

            if ui
                .add_enabled(
                    actions_enabled && self.config_recovery_error.is_none(),
                    egui::Button::new("Open output dir"),
                )
                .clicked()
            {
                let output_directory = self.settings.output_directory().to_path_buf();
                self.open_directory(&output_directory, "output");
            }

            if ui
                .add_enabled(
                    actions_enabled && self.settings.log_path().is_some(),
                    egui::Button::new("Open log"),
                )
                .clicked()
                && let Some(log_path) = self.settings.log_path()
            {
                self.open_file(&log_path, "log");
            }

            if ui.button("Settings").clicked() {
                self.show_settings();
            }
        });
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let sink = GuiEventSink::new(sender, ctx.clone());
        let options = self.convert.run_options;
        let openmw_cfg = self.session_openmw_cfg.clone();
        let cancellation = CancellationToken::default();
        let worker_cancellation = cancellation.clone();

        self.convert.running = true;
        self.convert.cancelling = false;
        self.set_status("Converting with current run options...");
        self.convert.progress = None;
        self.convert.phase_reached = None;
        self.convert.event_receiver = Some(receiver);
        self.convert.cancellation = Some(cancellation);
        self.convert.output.clear();

        thread::spawn(move || {
            let mut stdout = GuiOutput::new(sink.clone());
            let mut stderr = GuiOutput::new(sink.clone());
            let progress_sink = sink.clone();
            let error = match groundcover::load_config_for_edit(openmw_cfg.as_deref(), None) {
                Ok((_path, mut config)) => {
                    options.apply_to_config(&mut config);
                    config.compile_regex_sets().and_then(|()| {
                        groundcover::run_with_config_events_and_cancel(
                            config.openmw_cfg.as_deref(),
                            &config,
                            &mut stdout,
                            &mut stderr,
                            &move |event| progress_sink.send(GuiEvent::Progress(event)),
                            &worker_cancellation,
                        )
                    })
                }
                Err(error) => Err(error),
            }
            .err()
            .map(|error| {
                let cancelled = error.kind() == io::ErrorKind::Interrupted
                    && worker_cancellation.is_cancelled();
                (error.to_string(), cancelled)
            });
            let (error, cancelled) =
                error.map_or((None, false), |(error, cancelled)| (Some(error), cancelled));

            sink.send(GuiEvent::Finished { error, cancelled });
        });
    }

    fn cancel_conversion(&mut self) {
        if let Some(cancellation) = &self.convert.cancellation {
            cancellation.cancel();
            self.convert.cancelling = true;
            self.convert.progress = None;
            self.set_status("Cancelling conversion...");
        }
    }

    pub(super) fn receive_conversion_events(&mut self, ctx: &egui::Context) {
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
                    self.record_reached_phase(event_phase(event));
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
                    self.apply_pending_progress(pending_progress.take());
                    if self.convert.running {
                        self.finish_conversion(
                            Some("Conversion worker disconnected.".to_owned()),
                            false,
                        );
                    }
                    return;
                }
            }
        }
    }

    fn handle_gui_event(&mut self, event: GuiEvent) {
        match event {
            GuiEvent::Output(output) => self.convert.output.push_str(&output),
            GuiEvent::Progress(event) => self.handle_progress_event(event),
            GuiEvent::Finished { error, cancelled } => self.finish_conversion(error, cancelled),
        }
    }

    fn handle_progress_event(&mut self, event: ConversionEvent) {
        self.record_reached_phase(event_phase(event));

        if self.convert.cancelling {
            return;
        }

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

    fn record_reached_phase(&mut self, phase: ConversionPhase) {
        if match self.convert.phase_reached {
            Some(reached) => phase_order(phase) >= phase_order(reached),
            None => true,
        } {
            self.convert.phase_reached = Some(phase);
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

    fn finish_conversion(&mut self, error: Option<String>, cancelled: bool) {
        let cancellation_notice = self.cancellation_notice();
        self.convert.running = false;
        self.convert.cancelling = false;
        self.convert.progress = None;
        self.convert.phase_reached = None;
        self.convert.cancellation = None;

        if cancelled {
            append_notice(&mut self.convert.output, cancellation_notice.output);
            self.set_status(cancellation_notice.status);
        } else if let Some(error) = error {
            append_error(&mut self.convert.output, &error);
            self.set_status(format!("Conversion failed: {error}"));
        } else {
            self.set_status("Conversion finished.");
        }
    }

    fn cancellation_notice(&self) -> CancellationNotice {
        let Some(phase) = self.convert.phase_reached else {
            return CancellationNotice::before_output();
        };

        match phase {
            ConversionPhase::LoadingStaticPlugins
            | ConversionPhase::PlanningStatics
            | ConversionPhase::LoadingCellPlugins
            | ConversionPhase::ScanningCells
            | ConversionPhase::ResolvingMeshes => CancellationNotice::before_output(),
            ConversionPhase::WritingPlugins | ConversionPhase::CopyingMeshes => {
                CancellationNotice::output_side_effects()
            }
            ConversionPhase::AutoEnabling | ConversionPhase::WritingLog => {
                CancellationNotice::config_side_effects()
            }
        }
    }

    fn show_progress(&self, ui: &mut egui::Ui) {
        let Some(progress) = &self.convert.progress else {
            return;
        };

        ui.label(progress.label());
        ui.add(progress.progress_bar(finite_widget_extent(ui.available_width())));
    }

    pub(super) fn set_status(&mut self, status: impl Into<String>) {
        self.convert.status = status.into();
    }

    fn open_directory(&mut self, directory: &Path, label: &str) {
        if !directory.is_dir() {
            self.set_status(format!(
                "Cannot open {label} directory because it does not exist: {}",
                directory.display()
            ));
            return;
        }

        match open_path_native(directory) {
            Ok(()) => self.set_status(format!(
                "Requested opening {label} directory: {}",
                directory.display()
            )),
            Err(error) => self.set_status(format!(
                "Failed to open {label} directory {}: {error}",
                directory.display()
            )),
        }
    }

    fn open_file(&mut self, file: &Path, label: &str) {
        if !file.is_file() {
            self.set_status(format!(
                "Cannot open {label} file because it does not exist: {}",
                file.display()
            ));
            return;
        }

        match open_path_native(file) {
            Ok(()) => self.set_status(format!(
                "Requested opening {label} file: {}",
                file.display()
            )),
            Err(error) => self.set_status(format!(
                "Failed to open {label} file {}: {error}",
                file.display()
            )),
        }
    }
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
        Self { sender, context }
    }

    fn send(&self, event: GuiEvent) {
        let _send_result = self.sender.send(event);
        self.context.request_repaint();
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

    fn progress_bar(&self, width: f32) -> egui::ProgressBar {
        match self.progress {
            ProgressKind::Indeterminate => egui::ProgressBar::new(0.0)
                .animate(true)
                .desired_width(width),
            ProgressKind::Counted { current, total } => {
                egui::ProgressBar::new(progress_fraction(current, total))
                    .show_percentage()
                    .desired_width(width)
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

fn append_notice(output: &mut String, notice: &str) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(notice);
    output.push('\n');
}

const fn event_phase(event: ConversionEvent) -> ConversionPhase {
    match event {
        ConversionEvent::PhaseStarted(phase) | ConversionEvent::Progress { phase, .. } => phase,
    }
}

const fn phase_order(phase: ConversionPhase) -> u8 {
    match phase {
        ConversionPhase::LoadingStaticPlugins => 0,
        ConversionPhase::PlanningStatics => 1,
        ConversionPhase::LoadingCellPlugins => 2,
        ConversionPhase::ScanningCells => 3,
        ConversionPhase::ResolvingMeshes => 4,
        ConversionPhase::WritingPlugins => 5,
        ConversionPhase::CopyingMeshes => 6,
        ConversionPhase::AutoEnabling => 7,
        ConversionPhase::WritingLog => 8,
    }
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

fn finite_widget_size(size: egui::Vec2) -> egui::Vec2 {
    egui::vec2(finite_widget_extent(size.x), finite_widget_extent(size.y))
}

fn finite_widget_extent(extent: f32) -> f32 {
    if extent.is_finite() {
        extent.max(MIN_WIDGET_SIZE)
    } else {
        MIN_WIDGET_SIZE
    }
}

struct OpenCommand {
    program: &'static str,
    args: Vec<OsString>,
}

fn open_path_native(path: &Path) -> io::Result<()> {
    let mut last_error = None;

    for candidate in path_open_commands(path) {
        let mut command = Command::new(candidate.program);
        command.args(&candidate.args);
        match command.spawn() {
            Ok(mut child) => {
                thread::spawn(move || {
                    let _status = child.wait();
                });
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no path opener available")))
}

#[cfg(target_os = "windows")]
fn path_open_commands(path: &Path) -> Vec<OpenCommand> {
    vec![OpenCommand {
        program: "explorer",
        args: vec![path.as_os_str().to_owned()],
    }]
}

#[cfg(target_os = "macos")]
fn path_open_commands(path: &Path) -> Vec<OpenCommand> {
    vec![OpenCommand {
        program: "open",
        args: vec![path.as_os_str().to_owned()],
    }]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn path_open_commands(path: &Path) -> Vec<OpenCommand> {
    let path = path.as_os_str().to_owned();
    vec![
        OpenCommand {
            program: "xdg-open",
            args: vec![path.clone()],
        },
        OpenCommand {
            program: "gio",
            args: vec![OsString::from("open"), path.clone()],
        },
        OpenCommand {
            program: "kde-open5",
            args: vec![path.clone()],
        },
        OpenCommand {
            program: "kde-open",
            args: vec![path],
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, path::Path};

    use super::{ConvertRunOptions, ConvertUiState};

    #[cfg(all(unix, not(target_os = "macos")))]
    use super::path_open_commands;

    #[test]
    fn settings_save_preserves_unrelated_transient_run_options() {
        let saved = ConvertRunOptions::default();
        let transient = ConvertRunOptions {
            dry_run: true,
            debug: false,
            auto_enable: false,
        };
        let mut state = ConvertUiState::ready();
        state.sync_run_options(saved);
        state.run_options = transient;

        state.sync_saved_run_options_from_settings(saved);

        assert_eq!(state.current_run_options(), transient);
    }

    #[test]
    fn settings_save_applies_changed_saved_run_option_fields() {
        let saved = ConvertRunOptions::default();
        let changed = ConvertRunOptions {
            dry_run: false,
            debug: true,
            auto_enable: false,
        };
        let mut state = ConvertUiState::ready();
        state.sync_run_options(saved);
        state.run_options = ConvertRunOptions {
            dry_run: true,
            debug: false,
            auto_enable: false,
        };

        state.sync_saved_run_options_from_settings(changed);

        assert_eq!(
            state.current_run_options(),
            ConvertRunOptions {
                dry_run: true,
                debug: true,
                auto_enable: false,
            }
        );
    }

    #[test]
    fn settings_save_overwrites_transient_field_when_that_default_changes() {
        let saved = ConvertRunOptions::default();
        let changed = ConvertRunOptions {
            dry_run: true,
            debug: false,
            auto_enable: false,
        };
        let mut state = ConvertUiState::ready();
        state.sync_run_options(saved);
        state.run_options = ConvertRunOptions {
            dry_run: false,
            debug: true,
            auto_enable: false,
        };

        state.sync_saved_run_options_from_settings(changed);

        assert_eq!(
            state.current_run_options(),
            ConvertRunOptions {
                dry_run: true,
                debug: true,
                auto_enable: false,
            }
        );
    }

    #[test]
    #[cfg(all(unix, not(target_os = "macos")))]
    fn linux_path_open_commands_use_paths_not_file_urls() {
        let path = Path::new("/tmp/greenmote output/log");
        let commands = path_open_commands(path);

        assert_eq!(commands[0].program, "xdg-open");
        assert_eq!(commands[0].args, vec![path.as_os_str().to_owned()]);
        assert_eq!(commands[1].program, "gio");
        assert_eq!(
            commands[1].args,
            vec![OsString::from("open"), path.as_os_str().to_owned()]
        );
    }
}
