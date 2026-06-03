// SPDX-License-Identifier: GPL-3.0-only

use std::{ffi::OsString, io, path::Path, process::Command, sync::mpsc, thread};

use eframe::egui;

use crate::{
    groundcover::{self, CancellationToken, ConversionEvent, ConversionPhase},
    unclip::{self, UnclipArgs},
};

use super::{ConvertRunOptions, GreenmoteApp, UiText, UnclipRunOptions};

#[cfg(test)]
use super::UnclipTargetRunOption;

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
    active_worker: Option<WorkerKind>,
    event_receiver: Option<mpsc::Receiver<GuiEvent>>,
    cancellation: Option<CancellationToken>,
    run_options: ConvertRunOptions,
    saved_run_options: ConvertRunOptions,
    unclip: UnclipUiState,
}

enum GuiEvent {
    Output(String),
    Progress(ConversionEvent),
    UnclipTargetStatus {
        index: usize,
        status: UnclipTargetStatus,
    },
    Finished {
        worker: WorkerKind,
        error: Option<String>,
        cancelled: bool,
    },
}

#[derive(Clone, Copy)]
enum WorkerKind {
    Convert,
    Unclip,
}

#[derive(Default)]
struct UnclipUiState {
    run_options: UnclipRunOptions,
    selected_target: Option<usize>,
    pending_target: String,
    pending_write_confirmation: bool,
    target_statuses: Vec<UnclipTargetStatus>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum UnclipTargetStatus {
    #[default]
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct UnclipBatchSummary {
    succeeded: usize,
    failed: usize,
    skipped: usize,
    cancelled: usize,
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

    pub(super) fn sync_unclip_run_options(&mut self, options: UnclipRunOptions) {
        self.unclip.run_options = options;
        self.unclip.selected_target = None;
        self.unclip.pending_target.clear();
        self.unclip.pending_write_confirmation = false;
        self.reset_unclip_target_statuses();
    }

    pub(super) fn sync_loaded_openmw_config_status(&mut self, openmw_cfg: Option<&Path>) {
        if let Some(path) = openmw_cfg {
            self.status = loaded_openmw_config_status(path);
        }
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

    pub(super) fn sync_unclip_write_from_settings(&mut self, write: bool) {
        self.unclip.run_options.write = write;
        self.unclip.pending_write_confirmation = false;
    }

    pub(super) fn cancel_pending_unclip_write_confirmation(&mut self) {
        self.unclip.pending_write_confirmation = false;
    }

    #[cfg(test)]
    pub(super) fn set_unclip_write_for_test(&mut self, write: bool) {
        self.unclip.run_options.write = write;
    }

    #[cfg(test)]
    pub(super) fn unclip_write_for_test(&self) -> bool {
        self.unclip.run_options.write
    }

    #[cfg(test)]
    pub(super) fn set_pending_unclip_write_confirmation_for_test(&mut self, pending: bool) {
        self.unclip.pending_write_confirmation = pending;
    }

    #[cfg(test)]
    pub(super) fn pending_unclip_write_confirmation_for_test(&self) -> bool {
        self.unclip.pending_write_confirmation
    }

    fn selected_unclip_target(&self) -> Option<usize> {
        let selected = self.unclip.selected_target?;
        (selected < self.unclip.run_options.targets.len()).then_some(selected)
    }

    fn set_selected_unclip_target(&mut self, selected: Option<usize>) {
        self.unclip.selected_target =
            selected.filter(|index| *index < self.unclip.run_options.targets.len());
    }

    fn add_unclip_target(&mut self, target: impl Into<String>) -> bool {
        let added = self.unclip.run_options.add_target(target);
        if added {
            self.unclip.selected_target = self.unclip.run_options.targets.len().checked_sub(1);
            self.unclip
                .target_statuses
                .push(UnclipTargetStatus::Pending);
        }

        added
    }

    fn add_unclip_target_with_output(
        &mut self,
        target: impl Into<String>,
        output_plugin: Option<String>,
    ) -> bool {
        let added = self
            .unclip
            .run_options
            .add_target_with_output(target, output_plugin);
        if added {
            self.unclip.selected_target = self.unclip.run_options.targets.len().checked_sub(1);
            self.unclip
                .target_statuses
                .push(UnclipTargetStatus::Pending);
        }

        added
    }

    fn set_selected_unclip_output(&mut self, output_plugin: Option<String>) -> bool {
        let Some(selected) = self.selected_unclip_target() else {
            return false;
        };

        self.unclip
            .run_options
            .set_target_output(selected, output_plugin)
    }

    fn remove_selected_unclip_target(&mut self) -> bool {
        let Some(selected) = self.selected_unclip_target() else {
            return false;
        };

        let removed = self.unclip.run_options.remove_target(selected);
        if removed {
            self.unclip.target_statuses.remove(selected);
            let len = self.unclip.run_options.targets.len();
            self.unclip.selected_target = if len == 0 {
                None
            } else {
                Some(selected.min(len - 1))
            };
        }

        removed
    }

    fn clear_unclip_targets(&mut self) {
        self.unclip.run_options.clear_targets();
        self.unclip.selected_target = None;
        self.unclip.target_statuses.clear();
    }

    fn reset_unclip_target_statuses(&mut self) {
        self.unclip.target_statuses =
            vec![UnclipTargetStatus::Pending; self.unclip.run_options.targets.len()];
    }

    fn run_options_differ_from_saved(&self) -> bool {
        self.run_options != self.saved_run_options
    }
}

fn loaded_openmw_config_status(path: &Path) -> String {
    format!("Loaded OpenMW config: {}", path.display())
}

impl GreenmoteApp {
    pub(super) fn show_convert_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Keep the controls row at its natural height. The output area below owns
        // the remaining vertical space so dry-run results stay visible.
        self.show_convert_heading(ui);
        self.show_convert_panel(ui);
        self.show_convert_action_row(ui, ctx);
        ui.separator();

        self.show_run_output(ui, ctx);
    }

    pub(super) fn show_unclip_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.show_unclip_heading(ui);
        self.show_unclip_panel(ui);
        self.show_unclip_action_row(ui, ctx);
        ui.separator();

        self.show_run_output(ui, ctx);
        self.show_unclip_write_confirmation(ctx);
    }

    fn show_run_output(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::Panel::bottom("convert_output_actions")
            .resizable(false)
            .show_separator_line(true)
            .show_inside(ui, |ui| {
                self.show_convert_output_actions(ui, ctx);
            });

        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(finite_widget_extent(ui.available_width()));
                ui.add(
                    egui::Label::new(egui::RichText::new(self.convert.output.as_str()).monospace())
                        .wrap_mode(egui::TextWrapMode::Wrap)
                        .selectable(false),
                );
            });
    }

    fn show_convert_panel(&mut self, ui: &mut egui::Ui) {
        self.show_convert_run_options(ui);
    }

    fn show_convert_heading(&self, ui: &mut egui::Ui) {
        ui.heading(self.localizer.text(UiText::Convert));
    }

    fn show_unclip_heading(&self, ui: &mut egui::Ui) {
        ui.heading(self.localizer.text(UiText::Unclip));
    }

    fn show_unclip_panel(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(finite_widget_extent(ui.available_width()));
            ui.label(egui::RichText::new(self.localizer.text(UiText::RunOptions)).strong());
            ui.add_enabled_ui(!self.convert.running, |ui| {
                self.show_unclip_target_list(ui);

                ui.checkbox(
                    &mut self.convert.unclip.run_options.write,
                    self.localizer.text(UiText::WriteChangesToPlugin),
                );
            });
        });
    }

    fn show_unclip_target_list(&mut self, ui: &mut egui::Ui) {
        ui.label(self.localizer.text(UiText::TargetPlugins));

        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(finite_widget_extent(ui.available_width()));
            if self.convert.unclip.run_options.targets.is_empty() {
                ui.label(egui::RichText::new(self.localizer.text(UiText::EmptyTargetList)).weak());
            } else {
                egui::ScrollArea::vertical()
                    .max_height(120.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let selected_target = self.convert.selected_unclip_target();
                        let mut next_selected = selected_target;
                        for (index, target) in
                            self.convert.unclip.run_options.targets.iter().enumerate()
                        {
                            let status = self
                                .convert
                                .unclip
                                .target_statuses
                                .get(index)
                                .copied()
                                .unwrap_or(UnclipTargetStatus::Pending);
                            if ui
                                .selectable_label(
                                    selected_target == Some(index),
                                    format!(
                                        "[{}] {}",
                                        self.localizer.text(status.text_key()),
                                        target.label()
                                    ),
                                )
                                .clicked()
                            {
                                next_selected = Some(index);
                            }
                        }
                        self.convert.set_selected_unclip_target(next_selected);
                    });
            }
        });

        ui.horizontal_wrapped(|ui| {
            if ui.button(self.localizer.text(UiText::AddFiles)).clicked()
                && let Some(paths) = select_plugin_files(self.localizer)
            {
                for path in paths {
                    let output_plugin = select_unclip_output_plugin(self.localizer, &path)
                        .filter(|output_path| output_path != &path)
                        .map(|path| path.display().to_string());
                    self.convert
                        .add_unclip_target_with_output(path.display().to_string(), output_plugin);
                }
            }

            let can_set_output = self.convert.selected_unclip_target().is_some();
            if ui
                .add_enabled(
                    can_set_output,
                    egui::Button::new(self.localizer.text(UiText::SetUnclipOutputPlugin)),
                )
                .clicked()
                && let Some(selected) = self.convert.selected_unclip_target()
                && let Some(target) = self.convert.unclip.run_options.targets.get(selected)
            {
                let input_plugin = target.plugin.clone();
                let output_plugin = target.output_plugin.clone();
                let default_path = output_plugin.as_deref().unwrap_or(&input_plugin);
                if let Some(output_path) =
                    select_unclip_output_plugin(self.localizer, Path::new(default_path))
                {
                    self.convert
                        .set_selected_unclip_output(Some(output_path.display().to_string()));
                }
            }

            let can_clear_output = self
                .convert
                .selected_unclip_target()
                .and_then(|selected| self.convert.unclip.run_options.targets.get(selected))
                .is_some_and(|target| target.output_plugin.is_some());
            if ui
                .add_enabled(
                    can_clear_output,
                    egui::Button::new(self.localizer.text(UiText::ClearUnclipOutputPlugin)),
                )
                .clicked()
            {
                self.convert.set_selected_unclip_output(None);
            }

            let can_remove = self.convert.selected_unclip_target().is_some();
            if ui
                .add_enabled(
                    can_remove,
                    egui::Button::new(self.localizer.text(UiText::RemoveSelectedTarget)),
                )
                .clicked()
            {
                self.convert.remove_selected_unclip_target();
            }

            let can_clear = !self.convert.unclip.run_options.targets.is_empty();
            if ui
                .add_enabled(
                    can_clear,
                    egui::Button::new(self.localizer.text(UiText::ClearTargets)),
                )
                .clicked()
            {
                self.convert.clear_unclip_targets();
            }
        });

        ui.horizontal_wrapped(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.convert.unclip.pending_target)
                    .hint_text(self.localizer.text(UiText::TargetPathEntry))
                    .desired_width(finite_widget_extent(ui.available_width() - 112.0)),
            );
            let submitted =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let clicked = ui
                .button(self.localizer.text(UiText::AddTargetPath))
                .clicked();
            if clicked || submitted {
                let target = self.convert.unclip.pending_target.clone();
                if self.convert.add_unclip_target(target) {
                    self.convert.unclip.pending_target.clear();
                }
            }
        });
    }

    fn show_convert_action_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            self.show_start_conversion_button(ui, ctx);
            self.show_action_row_status(ui);
        });
    }

    fn show_unclip_action_row(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            self.show_unclip_action_button(ui, ctx);
            self.show_action_row_status(ui);
        });
    }

    fn show_start_conversion_button(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> egui::Response {
        let can_start = self.can_start_worker();
        let response = ui.add_enabled(
            can_start,
            egui::Button::new(self.localizer.text(UiText::StartConversion)),
        );
        if response.clicked() {
            self.start_conversion(ctx);
        }
        response
    }

    fn show_unclip_action_button(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
    ) -> egui::Response {
        let can_start = self.can_start_worker();
        let label = if self.convert.unclip.run_options.write {
            self.localizer.text(UiText::WriteChanges)
        } else {
            self.localizer.text(UiText::InspectPlugin)
        };

        let response = ui.add_enabled(can_start, egui::Button::new(label));
        if response.clicked() {
            self.request_unclip_run(ctx);
        }
        response
    }

    fn show_action_row_status(&self, ui: &mut egui::Ui) {
        if let Some(progress) = &self.convert.progress {
            ui.add(
                progress
                    .progress_bar(finite_widget_extent(ui.available_width()))
                    .text(progress.label()),
            );
        } else {
            ui.add(egui::Label::new(&self.convert.status).wrap_mode(egui::TextWrapMode::Truncate));
        }
    }

    fn show_convert_run_options(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.label(egui::RichText::new(self.localizer.text(UiText::RunOptions)).strong());
            ui.horizontal_wrapped(|ui| {
                ui.add_enabled_ui(!self.convert.running, |ui| {
                    let mut dry_run = self.convert.run_options.dry_run;
                    if ui
                        .checkbox(&mut dry_run, self.localizer.text(UiText::DryRun))
                        .changed()
                    {
                        self.convert.run_options.set_dry_run(dry_run);
                    }

                    let mut debug = self.convert.run_options.debug;
                    if ui
                        .checkbox(&mut debug, self.localizer.text(UiText::DebugDiagnostics))
                        .changed()
                    {
                        self.convert.run_options.set_debug(debug);
                    }

                    ui.add_enabled(
                        self.convert.run_options.can_edit_auto_enable(),
                        egui::Checkbox::new(
                            &mut self.convert.run_options.auto_enable,
                            self.localizer.text(UiText::AutoEnableGeneratedPlugins),
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
                    .add_enabled(
                        can_save,
                        egui::Button::new(self.localizer.text(UiText::SaveAsDefaults)),
                    )
                    .clicked()
                {
                    let _saved = self.save_convert_run_options_as_defaults();
                }

                if ui
                    .add_enabled(
                        differs && !self.convert.running,
                        egui::Button::new(self.localizer.text(UiText::ResetFromSaved)),
                    )
                    .clicked()
                {
                    self.convert.run_options = self.convert.saved_run_options;
                    self.set_status("Reset run options from saved settings.");
                }

                if differs {
                    ui.label(self.localizer.text(UiText::RunOptionsDiffer));
                } else {
                    ui.label(self.localizer.text(UiText::RunOptionsMatch));
                }
            });

            if self.settings.is_dirty() && differs {
                ui.label(
                    self.localizer
                        .text(UiText::SaveOrDiscardSettingsBeforeDefaults),
                );
            }
        });
    }

    fn can_start_worker(&self) -> bool {
        !self.has_active_worker_state()
            && !self.settings.is_dirty()
            && self.config_recovery_error.is_none()
            && self.openmw_config_error.is_none()
    }

    fn has_active_worker_state(&self) -> bool {
        self.convert.running
            || self.convert.active_worker.is_some()
            || self.convert.event_receiver.is_some()
            || self.convert.cancellation.is_some()
    }

    fn validate_worker_start(&self) -> Result<(), String> {
        if self.has_active_worker_state() {
            return Err("Wait for the current run to finish.".to_owned());
        }
        if self.settings.is_dirty() {
            return Err("Save Settings changes before starting a run.".to_owned());
        }
        if self.config_recovery_error.is_some() {
            return Err("Regenerate Settings before starting a run.".to_owned());
        }
        if self.openmw_config_error.is_some() {
            return Err("Choose an OpenMW config before starting a run.".to_owned());
        }

        Ok(())
    }

    fn request_unclip_run(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.validate_unclip_run() {
            self.set_status(error);
            return;
        }

        if self.convert.unclip.run_options.write {
            self.convert.unclip.pending_write_confirmation = true;
        } else {
            self.start_unclip(ctx);
        }
    }

    fn validate_unclip_run(&self) -> Result<(), String> {
        self.validate_worker_start()?;
        self.convert.unclip.run_options.to_args_list()?;
        if self.convert.unclip.run_options.write
            && self.settings.unclip_write_action_names().is_empty()
        {
            return Err(
                "Unclip write mode is blocked because no write actions are enabled in Settings."
                    .to_owned(),
            );
        }

        Ok(())
    }

    fn validate_unclip_write_confirmation(&self) -> Result<(), String> {
        self.validate_unclip_run()?;
        if !self.convert.unclip.run_options.write {
            return Err("Unclip write mode is no longer enabled.".to_owned());
        }

        Ok(())
    }

    fn show_unclip_write_confirmation(&mut self, ctx: &egui::Context) {
        if !self.convert.unclip.pending_write_confirmation {
            return;
        }

        if let Err(error) = self.validate_unclip_write_confirmation() {
            self.convert.unclip.pending_write_confirmation = false;
            self.set_status(format!("Unclip write confirmation dismissed: {error}"));
            return;
        }

        let targets = self
            .convert
            .unclip
            .run_options
            .targets
            .iter()
            .filter(|target| !target.plugin.trim().is_empty())
            .map(|target| target.label())
            .collect::<Vec<_>>();
        let actions = self.settings.unclip_write_action_names().join(", ");
        let mut confirm = false;
        let mut cancel = false;

        egui::Window::new(self.localizer.text(UiText::ConfirmUnclipWriteTitle))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(self.localizer.text(UiText::ConfirmUnclipWriteMessage));
                ui.label(self.localizer.unclip_target_count(targets.len()));
                for target in targets.iter().take(5) {
                    ui.label(format!("- {target}"));
                }
                if targets.len() > 5 {
                    ui.label(format!(
                        "- {}",
                        self.localizer.unclip_target_overflow(targets.len() - 5)
                    ));
                }
                ui.label(format!(
                    "{} {actions}",
                    self.localizer.text(UiText::EnabledWriteActions)
                ));
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    confirm = ui
                        .button(self.localizer.text(UiText::WriteChanges))
                        .clicked();
                    cancel = ui.button(self.localizer.text(UiText::Cancel)).clicked();
                });
            });

        if confirm {
            self.confirm_unclip_write(ctx);
        } else if cancel {
            self.convert.unclip.pending_write_confirmation = false;
            self.set_status("Unclip write cancelled.");
        }
    }

    fn confirm_unclip_write(&mut self, ctx: &egui::Context) {
        self.convert.unclip.pending_write_confirmation = false;
        if let Err(error) = self.validate_unclip_write_confirmation() {
            self.set_status(format!("Unclip write blocked: {error}"));
            return;
        }

        self.start_unclip(ctx);
    }

    fn show_convert_output_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(ui.spacing().item_spacing.y);
        ui.horizontal_wrapped(|ui| {
            let actions_enabled = !self.convert.running;
            if ui
                .add_enabled(
                    self.convert.running && !self.convert.cancelling,
                    egui::Button::new(self.localizer.text(UiText::Cancel)),
                )
                .clicked()
            {
                self.cancel_worker();
            }

            if ui
                .add_enabled(
                    actions_enabled && !self.convert.output.is_empty(),
                    egui::Button::new(self.localizer.text(UiText::ClearOutput)),
                )
                .clicked()
            {
                self.convert.output.clear();
                self.set_status("Output cleared.");
            }

            if ui
                .add_enabled(
                    actions_enabled && !self.convert.output.is_empty(),
                    egui::Button::new(self.localizer.text(UiText::CopyOutput)),
                )
                .clicked()
            {
                ctx.copy_text(self.convert.output.clone());
                self.set_status("Copied output to clipboard.");
            }

            if ui
                .add_enabled(
                    actions_enabled && self.config_recovery_error.is_none(),
                    egui::Button::new(self.localizer.text(UiText::OpenOutputDir)),
                )
                .clicked()
            {
                let output_directory = self.settings.output_directory().to_path_buf();
                self.open_directory(&output_directory, "output");
            }

            if ui
                .add_enabled(
                    actions_enabled && self.settings.log_path().is_some(),
                    egui::Button::new(self.localizer.text(UiText::OpenLog)),
                )
                .clicked()
                && let Some(log_path) = self.settings.log_path()
            {
                self.open_file(&log_path, "log");
            }
        });
    }

    fn start_conversion(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.validate_worker_start() {
            self.convert.unclip.pending_write_confirmation = false;
            self.set_status(format!("Conversion blocked: {error}"));
            return;
        }

        let (sender, receiver) = mpsc::channel();
        let sink = GuiEventSink::new(sender, ctx.clone());
        let options = self.convert.run_options;
        let openmw_cfg = self.session_openmw_cfg.clone();
        let cancellation = CancellationToken::default();
        let worker_cancellation = cancellation.clone();

        self.convert.running = true;
        self.convert.unclip.pending_write_confirmation = false;
        self.convert.cancelling = false;
        self.set_status("Converting with current run options...");
        self.convert.progress = None;
        self.convert.phase_reached = None;
        self.convert.active_worker = Some(WorkerKind::Convert);
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

            sink.send(GuiEvent::Finished {
                worker: WorkerKind::Convert,
                error,
                cancelled,
            });
        });
    }

    fn start_unclip(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.validate_unclip_run() {
            self.convert.unclip.pending_write_confirmation = false;
            self.set_status(format!("Unclip blocked: {error}"));
            return;
        }

        let args_list = match self.convert.unclip.run_options.to_args_list() {
            Ok(args) => args,
            Err(error) => {
                self.set_status(error);
                return;
            }
        };
        let (sender, receiver) = mpsc::channel();
        let sink = GuiEventSink::new(sender, ctx.clone());
        let openmw_cfg = self.session_openmw_cfg.clone();
        let cancellation = CancellationToken::default();
        let worker_cancellation = cancellation.clone();

        self.convert.running = true;
        self.convert.unclip.pending_write_confirmation = false;
        self.convert.cancelling = false;
        self.convert.progress = None;
        self.convert.phase_reached = None;
        self.convert.active_worker = Some(WorkerKind::Unclip);
        self.convert.event_receiver = Some(receiver);
        self.convert.cancellation = Some(cancellation);
        self.convert.output.clear();
        self.convert.reset_unclip_target_statuses();
        if self.convert.unclip.run_options.write {
            self.set_status(self.localizer.text(UiText::WritingUnclipBatch));
        } else {
            self.set_status(self.localizer.text(UiText::InspectingUnclipBatch));
        }

        thread::spawn(move || {
            let mut stdout = GuiOutput::new(sink.clone());
            let error = run_unclip_batch(
                openmw_cfg.as_deref(),
                &args_list,
                args_list
                    .first()
                    .is_some_and(|args| args.write.unwrap_or(false)),
                &mut stdout,
                &worker_cancellation,
                |index, status| {
                    sink.send(GuiEvent::UnclipTargetStatus { index, status });
                },
                |openmw_cfg, args, stdout, cancellation| {
                    unclip::run_with_output_and_cancel(openmw_cfg, None, args, stdout, cancellation)
                },
            );
            let (error, cancelled) = match error {
                Ok(error) => (error, false),
                Err(error) => {
                    let cancelled = error.kind() == io::ErrorKind::Interrupted
                        && worker_cancellation.is_cancelled();
                    (Some(error.to_string()), cancelled)
                }
            };
            sink.send(GuiEvent::Finished {
                worker: WorkerKind::Unclip,
                error,
                cancelled,
            });
        });
    }

    fn cancel_worker(&mut self) {
        if let Some(cancellation) = &self.convert.cancellation {
            cancellation.cancel();
            self.convert.cancelling = true;
            self.convert.progress = None;
            let label = match self.convert.active_worker.unwrap_or(WorkerKind::Convert) {
                WorkerKind::Convert => "conversion",
                WorkerKind::Unclip => "Unclip",
            };
            self.set_status(format!("Cancelling {label}..."));
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
                        let worker = self.convert.active_worker.unwrap_or(WorkerKind::Convert);
                        let label = match worker {
                            WorkerKind::Convert => "Conversion",
                            WorkerKind::Unclip => "Unclip",
                        };
                        self.finish_worker(
                            worker,
                            Some(format!("{label} worker disconnected.")),
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
            GuiEvent::UnclipTargetStatus { index, status } => {
                if let Some(target_status) = self.convert.unclip.target_statuses.get_mut(index) {
                    *target_status = status;
                }
            }
            GuiEvent::Finished {
                worker,
                error,
                cancelled,
            } => self.finish_worker(worker, error, cancelled),
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

    fn finish_worker(&mut self, worker: WorkerKind, error: Option<String>, cancelled: bool) {
        match worker {
            WorkerKind::Convert => self.finish_conversion(error, cancelled),
            WorkerKind::Unclip => self.finish_unclip(error, cancelled),
        }
    }

    fn finish_conversion(&mut self, error: Option<String>, cancelled: bool) {
        let cancellation_notice = self.cancellation_notice();
        self.convert.running = false;
        self.convert.cancelling = false;
        self.convert.progress = None;
        self.convert.phase_reached = None;
        self.convert.active_worker = None;
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

    fn finish_unclip(&mut self, error: Option<String>, cancelled: bool) {
        self.convert.running = false;
        self.convert.cancelling = false;
        self.convert.progress = None;
        self.convert.phase_reached = None;
        self.convert.active_worker = None;
        self.convert.cancellation = None;

        let label = if self.convert.unclip.run_options.write {
            UiText::UnclipWrite
        } else {
            UiText::UnclipInspection
        };

        if cancelled {
            if let Some(error) = error {
                append_error(&mut self.convert.output, &error);
            }
            self.set_status(self.unclip_cancelled_finished_status(label));
        } else if let Some(error) = error {
            self.set_status(self.unclip_finished_error_status(label, &error));
        } else {
            self.set_status(self.unclip_finished_status(label));
        }
    }

    fn unclip_finished_status(&self, label: UiText) -> String {
        let summary = summarize_unclip_statuses(&self.convert.unclip.target_statuses);
        self.localizer.unclip_finished_status(
            self.localizer.text(label),
            summary.succeeded,
            summary.failed,
            summary.skipped,
            summary.cancelled,
        )
    }

    fn unclip_finished_error_status(&self, label: UiText, error: &str) -> String {
        let summary = summarize_unclip_statuses(&self.convert.unclip.target_statuses);
        self.localizer.unclip_finished_error_status(
            self.localizer.text(label),
            summary.succeeded,
            summary.failed,
            summary.skipped,
            summary.cancelled,
            error,
        )
    }

    fn unclip_cancelled_finished_status(&self, label: UiText) -> String {
        let summary = summarize_unclip_statuses(&self.convert.unclip.target_statuses);
        self.localizer.unclip_cancelled_finished_status(
            self.localizer.text(label),
            summary.succeeded,
            summary.failed,
            summary.skipped,
            summary.cancelled,
        )
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

impl UnclipTargetStatus {
    const fn text_key(self) -> UiText {
        match self {
            Self::Pending => UiText::UnclipTargetPending,
            Self::Running => UiText::UnclipTargetRunning,
            Self::Succeeded => UiText::UnclipTargetSucceeded,
            Self::Failed => UiText::UnclipTargetFailed,
            Self::Skipped => UiText::UnclipTargetSkipped,
            Self::Cancelled => UiText::UnclipTargetCancelled,
        }
    }
}

fn run_unclip_batch<R, S>(
    openmw_cfg: Option<&Path>,
    args_list: &[UnclipArgs],
    write: bool,
    stdout: &mut dyn io::Write,
    cancellation: &CancellationToken,
    mut set_status: S,
    mut runner: R,
) -> io::Result<Option<String>>
where
    R: FnMut(Option<&Path>, &UnclipArgs, &mut dyn io::Write, &CancellationToken) -> io::Result<()>,
    S: FnMut(usize, UnclipTargetStatus),
{
    let mut summary = UnclipBatchSummary::default();
    let mut first_error = None;

    for (index, args) in args_list.iter().enumerate() {
        if cancellation.is_cancelled() {
            for (skipped_index, skipped_args) in args_list.iter().enumerate().skip(index) {
                let skipped = unclip_target_label(skipped_args);
                summary.skipped += 1;
                set_status(skipped_index, UnclipTargetStatus::Skipped);
                writeln!(stdout, "=== Unclip: {skipped} ===").ok();
                writeln!(
                    stdout,
                    "Unclip target skipped after cancellation: {skipped}\n"
                )
                .ok();
            }
            write_unclip_batch_summary(stdout, summary, true);
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "unclip cancelled",
            ));
        }
        let target = unclip_target_label(args);
        writeln!(stdout, "=== Unclip: {target} ===").ok();
        set_status(index, UnclipTargetStatus::Running);

        match runner(openmw_cfg, args, stdout, cancellation) {
            Ok(()) => {
                summary.succeeded += 1;
                set_status(index, UnclipTargetStatus::Succeeded);
                writeln!(stdout, "Unclip target succeeded: {target}\n").ok();
            }
            Err(error) => {
                if error.kind() == io::ErrorKind::Interrupted && cancellation.is_cancelled() {
                    summary.cancelled += 1;
                    set_status(index, UnclipTargetStatus::Cancelled);
                    writeln!(stdout, "Unclip target cancelled: {target}").ok();
                    writeln!(stdout, "error:").ok();
                    writeln!(stdout, "{error}\n").ok();
                    for (skipped_index, skipped_args) in
                        args_list.iter().enumerate().skip(index + 1)
                    {
                        let skipped = unclip_target_label(skipped_args);
                        summary.skipped += 1;
                        set_status(skipped_index, UnclipTargetStatus::Skipped);
                        writeln!(stdout, "=== Unclip: {skipped} ===").ok();
                        writeln!(
                            stdout,
                            "Unclip target skipped after cancellation: {skipped}\n"
                        )
                        .ok();
                    }
                    write_unclip_batch_summary(stdout, summary, true);
                    return Err(error);
                }
                let error = error.to_string();
                summary.failed += 1;
                set_status(index, UnclipTargetStatus::Failed);
                writeln!(stdout, "Unclip target failed: {target}").ok();
                writeln!(stdout, "error:").ok();
                writeln!(stdout, "{error}\n").ok();
                if write {
                    first_error = Some(error);
                    for (skipped_index, skipped_args) in
                        args_list.iter().enumerate().skip(index + 1)
                    {
                        let skipped = unclip_target_label(skipped_args);
                        summary.skipped += 1;
                        set_status(skipped_index, UnclipTargetStatus::Skipped);
                        writeln!(stdout, "=== Unclip: {skipped} ===").ok();
                        writeln!(
                            stdout,
                            "Unclip target skipped after previous write failure: {skipped}\n"
                        )
                        .ok();
                    }
                    break;
                }
            }
        }
    }

    write_unclip_batch_summary(stdout, summary, false);

    Ok(first_error)
}

fn write_unclip_batch_summary(
    stdout: &mut dyn io::Write,
    summary: UnclipBatchSummary,
    cancelled: bool,
) {
    if cancelled {
        writeln!(
            stdout,
            "Unclip batch summary: {} succeeded, {} failed, {} skipped, {} cancelled.",
            summary.succeeded, summary.failed, summary.skipped, summary.cancelled
        )
        .ok();
        writeln!(stdout, "Unclip batch cancelled.").ok();
    } else {
        writeln!(
            stdout,
            "Unclip batch summary: {} succeeded, {} failed, {} skipped.",
            summary.succeeded, summary.failed, summary.skipped
        )
        .ok();
    }
}

fn unclip_target_label(args: &UnclipArgs) -> String {
    let input = args.plugin.as_ref().map_or_else(
        || "(no target plugin)".to_owned(),
        |plugin| plugin.display().to_string(),
    );

    args.output_plugin.as_ref().map_or(input.clone(), |output| {
        format!("{input} -> {}", output.display())
    })
}

fn summarize_unclip_statuses(statuses: &[UnclipTargetStatus]) -> UnclipBatchSummary {
    statuses
        .iter()
        .fold(UnclipBatchSummary::default(), |mut summary, status| {
            match status {
                UnclipTargetStatus::Succeeded => summary.succeeded += 1,
                UnclipTargetStatus::Failed => summary.failed += 1,
                UnclipTargetStatus::Skipped => summary.skipped += 1,
                UnclipTargetStatus::Cancelled => summary.cancelled += 1,
                UnclipTargetStatus::Pending | UnclipTargetStatus::Running => {}
            }
            summary
        })
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

fn select_plugin_files(localizer: super::Localizer) -> Option<Vec<std::path::PathBuf>> {
    rfd::FileDialog::new()
        .set_title(localizer.text(UiText::SelectUnclipTargetPlugins))
        .add_filter(
            localizer.text(UiText::OpenMwPlugins),
            &["omwaddon", "esp", "esm"],
        )
        .pick_files()
}

fn select_unclip_output_plugin(
    localizer: super::Localizer,
    input_path: &Path,
) -> Option<std::path::PathBuf> {
    let mut dialog =
        rfd::FileDialog::new().set_title(localizer.text(UiText::SelectUnclipOutputPlugin));
    if let Some(parent) = input_path.parent() {
        dialog = dialog.set_directory(parent);
    }
    if let Some(file_name) = input_path.file_name() {
        dialog = dialog.set_file_name(file_name.to_string_lossy().to_string());
    }
    dialog
        .add_filter(
            localizer.text(UiText::OpenMwPlugins),
            &["omwaddon", "esp", "esm"],
        )
        .save_file()
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
    use std::{ffi::OsString, io, path::Path};

    use super::{
        ConvertRunOptions, ConvertUiState, UnclipRunOptions, UnclipTargetRunOption,
        UnclipTargetStatus, egui, loaded_openmw_config_status, run_unclip_batch,
        unclip_target_label,
    };
    use crate::{
        groundcover::CancellationToken,
        gui::{AppTab, GreenmoteApp, UiLanguage},
        unclip::UnclipArgs,
    };

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
    fn unclip_target_state_add_remove_and_clear_updates_selection() {
        let mut state = ConvertUiState::ready();

        assert!(state.add_unclip_target("first.omwaddon"));
        assert!(state.add_unclip_target("second.omwaddon"));
        assert_eq!(state.selected_unclip_target(), Some(1));

        assert!(state.remove_selected_unclip_target());
        assert_eq!(state.unclip.run_options.targets, [target("first.omwaddon")]);
        assert_eq!(state.selected_unclip_target(), Some(0));

        state.clear_unclip_targets();
        assert!(state.unclip.run_options.targets.is_empty());
        assert_eq!(state.selected_unclip_target(), None);
    }

    #[test]
    fn loaded_openmw_config_status_includes_path() {
        let path = Path::new("/tmp/openmw.cfg");

        assert_eq!(
            loaded_openmw_config_status(path),
            "Loaded OpenMW config: /tmp/openmw.cfg"
        );
    }

    #[test]
    fn pending_unclip_write_confirmation_cannot_start_after_worker_becomes_active() {
        let mut app = GreenmoteApp::default();
        app.convert.unclip.run_options = UnclipRunOptions {
            targets: vec![target("target.omwaddon")],
            write: true,
        };
        app.convert.unclip.pending_write_confirmation = true;
        app.convert.running = true;

        app.confirm_unclip_write(&egui::Context::default());

        assert!(!app.convert.unclip.pending_write_confirmation);
        assert!(app.convert.active_worker.is_none());
        assert!(app.convert.event_receiver.is_none());
        assert!(app.convert.cancellation.is_none());
        assert_eq!(
            app.convert.status,
            "Unclip write blocked: Wait for the current run to finish."
        );
    }

    #[test]
    fn no_unclip_write_actions_blocks_write_run() {
        let mut app = GreenmoteApp::default();
        app.settings.clear_unclip_write_actions_for_test();
        app.convert.unclip.run_options = UnclipRunOptions {
            targets: vec![target("target.omwaddon")],
            write: true,
        };

        let error = app.validate_unclip_run().unwrap_err();

        assert_eq!(
            error,
            "Unclip write mode is blocked because no write actions are enabled in Settings."
        );
    }

    #[test]
    fn settings_sync_updates_unclip_write_without_forcing_false() {
        let mut state = ConvertUiState::ready();
        state.unclip.run_options = UnclipRunOptions {
            targets: vec![target("target.omwaddon")],
            write: false,
        };
        state.unclip.pending_write_confirmation = true;

        state.sync_unclip_write_from_settings(true);

        assert!(state.unclip.run_options.write);
        assert_eq!(
            state.unclip.run_options.targets,
            [target("target.omwaddon")]
        );
        assert!(!state.unclip.pending_write_confirmation);
    }

    #[test]
    fn finish_unclip_cancelled_status_includes_batch_counts() {
        let mut app = GreenmoteApp::default();
        app.localizer.set_language(UiLanguage::French);
        app.convert.unclip.run_options.write = true;
        app.convert.unclip.target_statuses =
            vec![UnclipTargetStatus::Cancelled, UnclipTargetStatus::Skipped];

        app.finish_unclip(None, true);

        assert_eq!(
            app.convert.status,
            "Unclip annulé. Écriture Unclip terminée : 0 réussis, 0 échoués, 1 ignorés, 1 annulés."
        );
        assert!(!app.convert.status.contains("Unclip cancelled"));
    }

    #[test]
    fn unclip_validation_accepts_multiple_usable_targets_for_batch_runs() {
        let mut app = GreenmoteApp::default();
        app.convert.unclip.run_options = UnclipRunOptions {
            targets: vec![target("first.omwaddon"), target("second.omwaddon")],
            write: false,
        };

        app.validate_unclip_run().unwrap();
    }

    #[test]
    fn unclip_batch_runs_targets_in_order() {
        let args = test_unclip_args(["first.omwaddon", "second.omwaddon"], false);
        let mut output = Vec::new();
        let mut order = Vec::new();
        let mut statuses = Vec::new();
        let cancellation = CancellationToken::default();

        let error = run_unclip_batch(
            None,
            &args,
            false,
            &mut output,
            &cancellation,
            |index, status| statuses.push((index, status)),
            |_openmw_cfg, args, _stdout, _cancellation| {
                order.push(args.plugin.as_ref().unwrap().display().to_string());
                Ok(())
            },
        );

        assert_eq!(error.unwrap(), None);
        assert_eq!(order, ["first.omwaddon", "second.omwaddon"]);
        assert_eq!(
            statuses,
            [
                (0, UnclipTargetStatus::Running),
                (0, UnclipTargetStatus::Succeeded),
                (1, UnclipTargetStatus::Running),
                (1, UnclipTargetStatus::Succeeded),
            ]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("=== Unclip: first.omwaddon ==="));
        assert!(output.contains("Unclip batch summary: 2 succeeded, 0 failed, 0 skipped."));
    }

    #[test]
    fn unclip_target_label_shows_output_override_only_when_present() {
        let mut args = test_unclip_args(["input.omwaddon"], true).remove(0);

        assert_eq!(unclip_target_label(&args), "input.omwaddon");

        args.output_plugin = Some("output.omwaddon".into());

        assert_eq!(
            unclip_target_label(&args),
            "input.omwaddon -> output.omwaddon"
        );
    }

    #[test]
    fn unclip_batch_output_uses_output_override_labels() {
        let mut args = test_unclip_args(["input.omwaddon"], true);
        args[0].output_plugin = Some("output.omwaddon".into());
        let mut output = Vec::new();
        let cancellation = CancellationToken::default();

        run_unclip_batch(
            None,
            &args,
            true,
            &mut output,
            &cancellation,
            |_index, _status| {},
            |_openmw_cfg, _args, _stdout, _cancellation| Ok(()),
        )
        .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("=== Unclip: input.omwaddon -> output.omwaddon ==="));
        assert!(output.contains("Unclip target succeeded: input.omwaddon -> output.omwaddon"));
    }

    #[test]
    fn unclip_inspect_batch_continues_after_failure() {
        let args = test_unclip_args(
            ["first.omwaddon", "second.omwaddon", "third.omwaddon"],
            false,
        );
        let mut output = Vec::new();
        let mut order = Vec::new();
        let mut statuses = Vec::new();
        let cancellation = CancellationToken::default();

        let error = run_unclip_batch(
            None,
            &args,
            false,
            &mut output,
            &cancellation,
            |index, status| statuses.push((index, status)),
            |_openmw_cfg, args, _stdout, _cancellation| {
                let target = args.plugin.as_ref().unwrap().display().to_string();
                order.push(target.clone());
                if target == "second.omwaddon" {
                    Err(std::io::Error::other("simulated failure"))
                } else {
                    Ok(())
                }
            },
        );

        assert_eq!(error.unwrap(), None);
        assert_eq!(
            order,
            ["first.omwaddon", "second.omwaddon", "third.omwaddon"]
        );
        assert!(statuses.contains(&(1, UnclipTargetStatus::Failed)));
        assert!(statuses.contains(&(2, UnclipTargetStatus::Succeeded)));
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Unclip target failed: second.omwaddon"));
        assert!(output.contains("Unclip batch summary: 2 succeeded, 1 failed, 0 skipped."));
    }

    #[test]
    fn unclip_write_batch_stops_after_first_failure_and_skips_rest() {
        let args = test_unclip_args(
            ["first.omwaddon", "second.omwaddon", "third.omwaddon"],
            true,
        );
        let mut output = Vec::new();
        let mut order = Vec::new();
        let mut statuses = Vec::new();
        let cancellation = CancellationToken::default();

        let error = run_unclip_batch(
            None,
            &args,
            true,
            &mut output,
            &cancellation,
            |index, status| statuses.push((index, status)),
            |_openmw_cfg, args, _stdout, _cancellation| {
                let target = args.plugin.as_ref().unwrap().display().to_string();
                order.push(target.clone());
                if target == "second.omwaddon" {
                    Err(std::io::Error::other("simulated failure"))
                } else {
                    Ok(())
                }
            },
        );

        assert_eq!(error.unwrap().as_deref(), Some("simulated failure"));
        assert_eq!(order, ["first.omwaddon", "second.omwaddon"]);
        assert!(statuses.contains(&(1, UnclipTargetStatus::Failed)));
        assert!(statuses.contains(&(2, UnclipTargetStatus::Skipped)));
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("Unclip target skipped after previous write failure: third.omwaddon")
        );
        assert!(output.contains("Unclip batch summary: 1 succeeded, 1 failed, 1 skipped."));
    }

    #[test]
    fn unclip_batch_cancellation_before_next_target_skips_remaining_and_interrupts() {
        let args = test_unclip_args(["first.omwaddon", "second.omwaddon"], false);
        let mut output = Vec::new();
        let mut order = Vec::new();
        let mut statuses = Vec::new();
        let cancellation = CancellationToken::default();

        let error = run_unclip_batch(
            None,
            &args,
            false,
            &mut output,
            &cancellation,
            |index, status| statuses.push((index, status)),
            |_openmw_cfg, args, _stdout, cancellation| {
                order.push(args.plugin.as_ref().unwrap().display().to_string());
                cancellation.cancel();
                Ok(())
            },
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(order, ["first.omwaddon"]);
        assert_eq!(
            statuses,
            [
                (0, UnclipTargetStatus::Running),
                (0, UnclipTargetStatus::Succeeded),
                (1, UnclipTargetStatus::Skipped),
            ]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Unclip target skipped after cancellation: second.omwaddon"));
        assert!(output.contains("Unclip batch cancelled."));
    }

    #[test]
    fn unclip_batch_current_target_cancellation_marks_cancelled_and_skips_rest() {
        let args = test_unclip_args(["first.omwaddon", "second.omwaddon"], false);
        let mut output = Vec::new();
        let mut statuses = Vec::new();
        let cancellation = CancellationToken::default();

        let error = run_unclip_batch(
            None,
            &args,
            false,
            &mut output,
            &cancellation,
            |index, status| statuses.push((index, status)),
            |_openmw_cfg, _args, _stdout, cancellation| {
                cancellation.cancel();
                Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "unclip cancelled",
                ))
            },
        )
        .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(
            statuses,
            [
                (0, UnclipTargetStatus::Running),
                (0, UnclipTargetStatus::Cancelled),
                (1, UnclipTargetStatus::Skipped),
            ]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Unclip target cancelled: first.omwaddon"));
        assert!(output.contains("Unclip target skipped after cancellation: second.omwaddon"));
        assert!(
            output.contains("Unclip batch summary: 0 succeeded, 0 failed, 1 skipped, 1 cancelled.")
        );
    }

    #[test]
    fn unclip_single_target_batch_succeeds() {
        let args = test_unclip_args(["single.omwaddon"], false);
        let mut output = Vec::new();
        let mut statuses = Vec::new();
        let cancellation = CancellationToken::default();

        let error = run_unclip_batch(
            None,
            &args,
            false,
            &mut output,
            &cancellation,
            |index, status| statuses.push((index, status)),
            |_openmw_cfg, _args, _stdout, _cancellation| Ok(()),
        );

        assert_eq!(error.unwrap(), None);
        assert_eq!(
            statuses,
            [
                (0, UnclipTargetStatus::Running),
                (0, UnclipTargetStatus::Succeeded),
            ]
        );
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Unclip batch summary: 1 succeeded, 0 failed, 0 skipped."));
    }

    fn test_unclip_args<const N: usize>(targets: [&str; N], write: bool) -> Vec<UnclipArgs> {
        targets
            .into_iter()
            .map(|target| UnclipArgs {
                plugin: Some(target.into()),
                output_plugin: None,
                meshgenerator_ini: None,
                ignore_meshgenerator_ini: true,
                instances: None,
                verbose: None,
                structured: Some(false),
                write: Some(write),
                write_actions: Vec::new(),
                origin_epsilon: None,
                relocation_step: None,
                relocation_steps: None,
                orientation_epsilon: None,
                include_grass_ids: Vec::new(),
                exclude_grass_ids: Vec::new(),
                include_occluder_ids: Vec::new(),
                exclude_occluder_ids: Vec::new(),
            })
            .collect()
    }

    fn target(plugin: &str) -> UnclipTargetRunOption {
        UnclipTargetRunOption::new(plugin, None)
    }

    #[test]
    fn unclip_validation_rejects_blank_target_list() {
        let mut app = GreenmoteApp::default();
        app.convert.unclip.run_options = UnclipRunOptions {
            targets: vec![target(" ")],
            write: false,
        };

        let error = app.validate_unclip_run().unwrap_err();

        assert_eq!(error, "Choose a target plugin before running Unclip.");
    }

    #[test]
    fn leaving_unclip_cancels_pending_write_confirmation() {
        let mut app = GreenmoteApp {
            selected_tab: AppTab::Unclip,
            ..GreenmoteApp::default()
        };
        app.convert.unclip.pending_write_confirmation = true;

        app.request_convert();

        assert_eq!(app.selected_tab, AppTab::Convert);
        assert!(!app.convert.unclip.pending_write_confirmation);
    }

    #[test]
    fn leaving_unclip_for_settings_cancels_pending_write_confirmation() {
        let mut app = GreenmoteApp {
            selected_tab: AppTab::Unclip,
            ..GreenmoteApp::default()
        };
        app.convert.unclip.pending_write_confirmation = true;

        app.show_settings();

        assert_eq!(app.selected_tab, AppTab::Settings);
        assert!(!app.convert.unclip.pending_write_confirmation);
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
