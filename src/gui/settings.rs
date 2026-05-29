use std::path::{Path, PathBuf};

use eframe::egui;

use crate::groundcover::{self, GroundcoverConfig, openmw::ConvertOutputDirectorySource};

use super::{ConvertRunOptions, GreenmoteApp, PendingNavigation};

const SETTINGS_LIST_VISIBLE_ROWS: usize = 6;
const SETTINGS_LIST_FALLBACK_WIDTH: f32 = 560.0;
const SETTINGS_LIST_CONTROLS_WIDTH: f32 = 160.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsTab {
    General,
    Convert,
}

#[allow(clippy::struct_excessive_bools)]
pub(super) struct SettingsUiState {
    selected_tab: SettingsTab,
    draft: SettingsDraft,
    config_path: Option<PathBuf>,
    loaded: bool,
    dirty: bool,
    status: String,
    error: Option<String>,
    selected_list_item: Option<SettingsListItem>,
    editing_list_item: Option<SettingsListItem>,
    queued_edit_list_item: Option<SettingsListItem>,
    inline_edit_text: String,
    inline_edit_original_text: String,
    focus_inline_edit: bool,
    add_popup: Option<SettingsListKind>,
    add_text: String,
    focus_add_text: bool,
    grass_ids_viewport_start: usize,
    exclude_viewport_start: usize,
    ignored_plugins_viewport_start: usize,
}

#[derive(Default)]
#[allow(clippy::struct_excessive_bools)]
pub(super) struct SettingsDraft {
    output_directory: PathBuf,
    output_directory_source: ConvertOutputDirectorySource,
    grass_ids: Vec<String>,
    exclude: Vec<String>,
    ignored_plugins: Vec<String>,
    dry_run: bool,
    debug: bool,
    auto_enable: bool,
    unclip: crate::unclip::config::PersistedUnclipConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsListKind {
    GrassIds,
    Exclude,
    IgnoredPlugins,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SettingsListItem {
    kind: SettingsListKind,
    index: usize,
}

struct EditableListControl<'a> {
    selected_item: &'a mut Option<SettingsListItem>,
    editing_item: &'a mut Option<SettingsListItem>,
    queued_edit_item: &'a mut Option<SettingsListItem>,
    inline_edit_text: &'a mut String,
    inline_edit_original_text: &'a mut String,
    focus_inline_edit: &'a mut bool,
    add_popup: &'a mut Option<SettingsListKind>,
    add_text: &'a mut String,
    focus_add_text: &'a mut bool,
    dirty: &'a mut bool,
    viewport_start: &'a mut usize,
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
            selected_list_item: None,
            editing_list_item: None,
            queued_edit_list_item: None,
            inline_edit_text: String::new(),
            inline_edit_original_text: String::new(),
            focus_inline_edit: false,
            add_popup: None,
            add_text: String::new(),
            focus_add_text: false,
            grass_ids_viewport_start: 0,
            exclude_viewport_start: 0,
            ignored_plugins_viewport_start: 0,
        }
    }
}

impl SettingsUiState {
    pub(super) fn is_dirty(&self) -> bool {
        self.dirty || self.active_list_edit_changed()
    }

    pub(super) fn select_tab(&mut self, tab: SettingsTab) {
        self.selected_tab = tab;
    }

    pub(super) fn run_options(&self) -> ConvertRunOptions {
        self.draft.run_options()
    }

    pub(super) fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    pub(super) fn output_directory(&self) -> &Path {
        self.draft.output_directory.as_path()
    }

    pub(super) fn log_directory(&self) -> Option<PathBuf> {
        self.config_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_owned)
    }

    pub(super) fn log_path(&self) -> Option<PathBuf> {
        self.log_directory()
            .map(|directory| directory.join(crate::groundcover::LOG_NAME))
    }

    pub(super) fn replace_saved_config(
        &mut self,
        path: PathBuf,
        config: &GroundcoverConfig,
        status: String,
    ) {
        self.draft = SettingsDraft::from_config(config);
        self.config_path = Some(path);
        self.loaded = true;
        self.dirty = false;
        self.status = status;
        self.error = None;
        self.clear_list_interaction();
    }

    pub(super) fn commit_active_list_edit(&mut self) {
        let Some(item) = self.editing_list_item else {
            return;
        };

        let items = self.draft.items_mut(item.kind);
        if item.index >= items.len() {
            self.editing_list_item = None;
            self.inline_edit_text.clear();
            self.inline_edit_original_text.clear();
            self.focus_inline_edit = false;
            return;
        }

        match list_item_from_text(&self.inline_edit_text) {
            Some(new_item) if new_item != items[item.index] => {
                items[item.index] = new_item;
                self.dirty = true;
            }
            None => {
                items.remove(item.index);
                self.selected_list_item = if items.is_empty() {
                    None
                } else {
                    Some(SettingsListItem {
                        kind: item.kind,
                        index: item.index.min(items.len() - 1),
                    })
                };
                self.dirty = true;
            }
            Some(_) => {}
        }

        let items_len = self.draft.items_mut(item.kind).len();
        if let Some(selected_index) = self.selected_list_item.and_then(|selected| {
            (selected.kind == item.kind && selected.index < items_len).then_some(selected.index)
        }) {
            ensure_settings_list_item_visible(
                self.viewport_start_mut(item.kind),
                selected_index,
                items_len,
                SETTINGS_LIST_VISIBLE_ROWS,
            );
        } else {
            let viewport_start = self.viewport_start_mut(item.kind);
            *viewport_start = clamp_settings_list_viewport_start(
                *viewport_start,
                items_len,
                SETTINGS_LIST_VISIBLE_ROWS,
            );
        }

        self.editing_list_item = None;
        self.queued_edit_list_item = None;
        self.inline_edit_text.clear();
        self.inline_edit_original_text.clear();
        self.focus_inline_edit = false;
    }

    fn active_list_edit_changed(&self) -> bool {
        let Some(_item) = self.editing_list_item else {
            return false;
        };

        self.inline_edit_text != self.inline_edit_original_text
    }

    fn clear_list_interaction(&mut self) {
        self.selected_list_item = None;
        self.editing_list_item = None;
        self.queued_edit_list_item = None;
        self.inline_edit_text.clear();
        self.inline_edit_original_text.clear();
        self.focus_inline_edit = false;
        self.add_popup = None;
        self.add_text.clear();
        self.focus_add_text = false;
        self.grass_ids_viewport_start = 0;
        self.exclude_viewport_start = 0;
        self.ignored_plugins_viewport_start = 0;
    }
}

impl GreenmoteApp {
    pub(super) fn show_settings_screen(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Settings");
        ui.horizontal(|ui| {
            if ui
                .selectable_label(
                    self.settings.selected_tab == SettingsTab::General,
                    "General",
                )
                .clicked()
            {
                self.request_settings_tab(SettingsTab::General);
            }
            if ui
                .selectable_label(
                    self.settings.selected_tab == SettingsTab::Convert,
                    "Convert",
                )
                .clicked()
            {
                self.request_settings_tab(SettingsTab::Convert);
            }
        });
        ui.separator();

        egui::TopBottomPanel::bottom("settings_footer")
            .resizable(false)
            .show_separator_line(true)
            .show_inside(ui, |ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(self.settings.is_dirty(), egui::Button::new("Save"))
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
            });

        let available = ui.available_size();

        ui.allocate_ui_with_layout(available, egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .scroll_bar_visibility(
                    egui::containers::scroll_area::ScrollBarVisibility::AlwaysHidden,
                )
                .show(ui, |ui| {
                    ui.set_min_width(560.0);
                    match self.settings.selected_tab {
                        SettingsTab::General => self.show_general_settings(ui),
                        SettingsTab::Convert => self.show_convert_settings(ui),
                    }
                });
        });

        self.show_add_list_item_popup(ctx);
    }

    fn show_general_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("OpenMW config");
        ui.add_space(4.0);

        egui::Frame::group(ui.style())
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| match &self.session_openmw_cfg {
                Some(path) => {
                    ui.monospace(path.display().to_string());
                }
                None => {
                    ui.label("Using OpenMW autodetection.");
                }
            });

        ui.add_space(6.0);

        let can_select_config = !self.convert.is_running();
        if ui
            .add_enabled(can_select_config, egui::Button::new("Select OpenMW Config"))
            .clicked()
        {
            self.request_openmw_config_selection();
        }
        if !can_select_config {
            ui.label("OpenMW config cannot be changed while conversion is running.");
        }

        ui.add_space(8.0);

        self.show_output_directory_settings(ui);
    }

    fn show_convert_settings(&mut self, ui: &mut egui::Ui) {
        let mut list_control = EditableListControl {
            selected_item: &mut self.settings.selected_list_item,
            editing_item: &mut self.settings.editing_list_item,
            queued_edit_item: &mut self.settings.queued_edit_list_item,
            inline_edit_text: &mut self.settings.inline_edit_text,
            inline_edit_original_text: &mut self.settings.inline_edit_original_text,
            focus_inline_edit: &mut self.settings.focus_inline_edit,
            add_popup: &mut self.settings.add_popup,
            add_text: &mut self.settings.add_text,
            focus_add_text: &mut self.settings.focus_add_text,
            dirty: &mut self.settings.dirty,
            viewport_start: &mut self.settings.grass_ids_viewport_start,
        };

        setting_editable_list(
            ui,
            "Grass ID patterns",
            "No grass ID patterns configured.",
            SettingsListKind::GrassIds,
            &mut self.settings.draft.grass_ids,
            &mut list_control,
        );
        list_control.viewport_start = &mut self.settings.exclude_viewport_start;
        setting_editable_list(
            ui,
            "Exclude patterns",
            "No exclude patterns configured.",
            SettingsListKind::Exclude,
            &mut self.settings.draft.exclude,
            &mut list_control,
        );
        list_control.viewport_start = &mut self.settings.ignored_plugins_viewport_start;
        setting_editable_list(
            ui,
            "Ignored plugins",
            "No ignored plugins configured.",
            SettingsListKind::IgnoredPlugins,
            &mut self.settings.draft.ignored_plugins,
            &mut list_control,
        );

        ui.add_space(8.0);
        ui.label("Run options are configured on the Convert screen.");
    }

    fn show_add_list_item_popup(&mut self, ctx: &egui::Context) {
        let Some(kind) = self.settings.add_popup else {
            return;
        };

        let mut add = false;
        let mut cancel = false;

        egui::Window::new(kind.add_window_title())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(kind.add_prompt());
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.settings.add_text).desired_width(320.0),
                );
                if self.settings.focus_add_text {
                    response.request_focus();
                    self.settings.focus_add_text = false;
                }

                let pressed_enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
                let pressed_escape = ui.input(|input| input.key_pressed(egui::Key::Escape));
                let can_add = list_item_from_text(&self.settings.add_text).is_some();

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    add = ui.add_enabled(can_add, egui::Button::new("Add")).clicked();
                    cancel = ui.button("Cancel").clicked();
                });

                add |= can_add && response.has_focus() && pressed_enter;
                cancel |= response.has_focus() && pressed_escape;
            });

        if add {
            self.add_settings_list_item(kind);
        } else if cancel {
            self.settings.add_popup = None;
            self.settings.add_text.clear();
            self.settings.focus_add_text = false;
        }
    }

    fn add_settings_list_item(&mut self, kind: SettingsListKind) {
        let Some(item) = list_item_from_text(&self.settings.add_text) else {
            return;
        };

        let items_len = {
            let items = self.settings.draft.items_mut(kind);
            items.push(item);
            items.len()
        };
        let index = items_len - 1;
        self.settings.selected_list_item = Some(SettingsListItem { kind, index });
        ensure_settings_list_item_visible(
            self.settings.viewport_start_mut(kind),
            index,
            items_len,
            SETTINGS_LIST_VISIBLE_ROWS,
        );
        self.settings.editing_list_item = None;
        self.settings.inline_edit_text.clear();
        self.settings.inline_edit_original_text.clear();
        self.settings.focus_inline_edit = false;
        self.settings.add_popup = None;
        self.settings.add_text.clear();
        self.settings.focus_add_text = false;
        self.settings.dirty = true;
    }

    fn show_output_directory_settings(&mut self, ui: &mut egui::Ui) {
        ui.label("Convert output directory");
        ui.add_space(4.0);

        match self.settings.draft.output_directory_source {
            ConvertOutputDirectorySource::OpenMwDataLocal => {
                output_path_frame(ui, &self.settings.draft.output_directory);
                ui.label("From the selected OpenMW configuration's data-local setting.");
            }
            ConvertOutputDirectorySource::CliOverride => {
                output_path_frame(ui, &self.settings.draft.output_directory);
                ui.label("Overridden for this conversion run.");
            }
            ConvertOutputDirectorySource::WorkingDirectoryFallback => {
                output_path_frame(ui, &self.settings.draft.output_directory);
                ui.label(
                    "OpenMW has no data-local setting, so Greenmote will write to the current working directory shown above. OpenMW can only load the generated files if this folder is configured as data-local or data=.",
                );
            }
        }
    }

    pub(super) fn load_settings(&mut self) -> bool {
        if self.settings.loaded {
            return true;
        }

        self.reload_settings()
    }

    pub(super) fn settings_error(&self) -> Option<&str> {
        self.settings.error.as_deref()
    }

    fn reload_settings(&mut self) -> bool {
        self.load_settings_from_disk("Loaded")
    }

    pub(super) fn load_settings_with_openmw_cfg(&mut self, openmw_cfg: &Path) -> bool {
        match groundcover::load_config_for_edit(Some(openmw_cfg), None) {
            Ok((path, config)) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Loaded {}", path.display()),
                );
                self.convert
                    .sync_run_options(ConvertRunOptions::from_config(&config));
                true
            }
            Err(error) => {
                self.settings.loaded = false;
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to load settings: {error}"));
                false
            }
        }
    }

    pub(super) fn regenerate_settings(&mut self) -> bool {
        match groundcover::regenerate_config_for_edit(self.session_openmw_cfg.as_deref(), None) {
            Ok((path, config)) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Regenerated {}", path.display()),
                );
                self.convert
                    .sync_run_options(ConvertRunOptions::from_config(&config));
                true
            }
            Err(error) => {
                self.settings.loaded = false;
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to regenerate settings: {error}"));
                false
            }
        }
    }

    pub(super) fn discard_settings(&mut self) -> bool {
        self.load_settings_from_disk("Discarded changes and reloaded")
    }

    fn load_settings_from_disk(&mut self, verb: &str) -> bool {
        match groundcover::load_config_for_edit(self.session_openmw_cfg.as_deref(), None) {
            Ok((path, config)) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("{verb} {}", path.display()),
                );
                self.convert
                    .sync_run_options(ConvertRunOptions::from_config(&config));
                true
            }
            Err(error) => {
                self.settings.loaded = false;
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to load settings: {error}"));
                false
            }
        }
    }

    pub(super) fn save_settings(&mut self) -> bool {
        self.settings.commit_active_list_edit();

        let Some(path) = self.settings.config_path.clone() else {
            self.settings.error = Some("No greenmote.toml path is available.".to_owned());
            return false;
        };

        let config = self.settings.draft.to_config();
        match groundcover::save_config_for_edit(&config, &path) {
            Ok(config) => {
                self.settings.replace_saved_config(
                    path.clone(),
                    &config,
                    format!("Saved {}", path.display()),
                );
                self.convert
                    .sync_saved_run_options_from_settings(ConvertRunOptions::from_config(&config));
                true
            }
            Err(error) => {
                self.settings.status.clear();
                self.settings.error = Some(format!("Failed to save settings: {error}"));
                false
            }
        }
    }

    fn request_settings_tab(&mut self, tab: SettingsTab) {
        if self.settings.selected_tab == tab {
            return;
        }

        self.settings.commit_active_list_edit();

        if self.settings.dirty {
            if self.has_pending_navigation_request() {
                return;
            }

            self.queue_pending_navigation(PendingNavigation::SettingsTab(tab));
        } else {
            self.settings.select_tab(tab);
        }
    }
}

impl SettingsDraft {
    fn from_config(config: &GroundcoverConfig) -> Self {
        let run_options = ConvertRunOptions::from_config(config);
        Self {
            output_directory: config.output_directory.clone(),
            output_directory_source: config.output_directory_source.clone(),
            grass_ids: config.grass_ids.clone(),
            exclude: config.exclude.clone(),
            ignored_plugins: config.ignored_plugins.clone(),
            dry_run: run_options.dry_run,
            debug: run_options.debug,
            auto_enable: run_options.auto_enable,
            unclip: config.unclip.clone(),
        }
    }

    fn to_config(&self) -> GroundcoverConfig {
        let mut config = GroundcoverConfig::default();
        config.output_directory.clone_from(&self.output_directory);
        config.output_directory_source = self.output_directory_source.clone();
        config.grass_ids.clone_from(&self.grass_ids);
        config.exclude.clone_from(&self.exclude);
        config.ignored_plugins.clone_from(&self.ignored_plugins);
        config.dry_run = self.dry_run;
        config.debug = self.debug;
        config.auto_enable = self.auto_enable;
        config.unclip = self.unclip.clone();
        config
    }

    fn run_options(&self) -> ConvertRunOptions {
        ConvertRunOptions {
            dry_run: self.dry_run,
            debug: self.debug,
            auto_enable: self.auto_enable,
        }
    }

    fn items_mut(&mut self, kind: SettingsListKind) -> &mut Vec<String> {
        match kind {
            SettingsListKind::GrassIds => &mut self.grass_ids,
            SettingsListKind::Exclude => &mut self.exclude,
            SettingsListKind::IgnoredPlugins => &mut self.ignored_plugins,
        }
    }
}

impl SettingsUiState {
    fn viewport_start_mut(&mut self, kind: SettingsListKind) -> &mut usize {
        match kind {
            SettingsListKind::GrassIds => &mut self.grass_ids_viewport_start,
            SettingsListKind::Exclude => &mut self.exclude_viewport_start,
            SettingsListKind::IgnoredPlugins => &mut self.ignored_plugins_viewport_start,
        }
    }
}

fn setting_editable_list(
    ui: &mut egui::Ui,
    label: &str,
    empty_message: &str,
    kind: SettingsListKind,
    items: &mut Vec<String>,
    control: &mut EditableListControl<'_>,
) {
    ui.label(label);
    let inner_margin = settings_list_frame_inner_margin();
    let content_width = settings_list_content_width(ui.available_width(), inner_margin);
    egui::Frame::group(ui.style())
        .inner_margin(inner_margin)
        .show(ui, |ui| {
            ui.set_width(content_width);
            *control.viewport_start = clamp_settings_list_viewport_start(
                *control.viewport_start,
                items.len(),
                SETTINGS_LIST_VISIBLE_ROWS,
            );

            if items.is_empty() {
                ui.weak(empty_message);
            } else {
                show_list_items(ui, kind, items, control, content_width);
            }

            let is_long = items.len() > SETTINGS_LIST_VISIBLE_ROWS;
            if is_long {
                let start = *control.viewport_start + 1;
                let end = (*control.viewport_start + SETTINGS_LIST_VISIBLE_ROWS).min(items.len());
                ui.weak(format!("Showing {start}-{end} of {}", items.len()));
            }

            ui.allocate_ui_with_layout(
                egui::vec2(content_width, ui.spacing().interact_size.y),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let leading_space = (content_width - SETTINGS_LIST_CONTROLS_WIDTH).max(0.0);
                    ui.add_space(leading_space);

                    let can_move_up = *control.viewport_start > 0;
                    if ui
                        .add_enabled(can_move_up, egui::Button::new("Up"))
                        .on_hover_text("Show previous items")
                        .clicked()
                    {
                        *control.viewport_start -= 1;
                    }

                    let can_move_down =
                        *control.viewport_start + SETTINGS_LIST_VISIBLE_ROWS < items.len();
                    if ui
                        .add_enabled(can_move_down, egui::Button::new("Down"))
                        .on_hover_text("Show next items")
                        .clicked()
                    {
                        *control.viewport_start += 1;
                    }

                    ui.add_space(8.0);

                    let can_remove = control
                        .selected_item
                        .is_some_and(|item| item.kind == kind && item.index < items.len());
                    if ui.add_enabled(can_remove, egui::Button::new("-")).clicked() {
                        remove_selected_list_item(
                            kind,
                            items,
                            control.selected_item,
                            control.editing_item,
                        );
                        control.inline_edit_text.clear();
                        control.inline_edit_original_text.clear();
                        *control.focus_inline_edit = false;
                        *control.queued_edit_item = None;
                        *control.dirty = true;
                        if let Some(selected) = (*control.selected_item).and_then(|item| {
                            (item.kind == kind && item.index < items.len()).then_some(item.index)
                        }) {
                            ensure_settings_list_item_visible(
                                control.viewport_start,
                                selected,
                                items.len(),
                                SETTINGS_LIST_VISIBLE_ROWS,
                            );
                        } else {
                            *control.viewport_start = clamp_settings_list_viewport_start(
                                *control.viewport_start,
                                items.len(),
                                SETTINGS_LIST_VISIBLE_ROWS,
                            );
                        }
                    }

                    if ui.small_button("+").clicked() {
                        if *control.add_popup != Some(kind) {
                            control.add_text.clear();
                        }
                        *control.add_popup = Some(kind);
                        *control.focus_add_text = true;
                    }
                },
            );
        });
    ui.add_space(6.0);
}

fn show_list_items(
    ui: &mut egui::Ui,
    kind: SettingsListKind,
    items: &mut Vec<String>,
    control: &mut EditableListControl<'_>,
    row_width: f32,
) {
    let mut index = *control.viewport_start;
    while index < items.len() && index < *control.viewport_start + SETTINGS_LIST_VISIBLE_ROWS {
        let item = SettingsListItem { kind, index };
        if control.editing_item.is_none() && *control.queued_edit_item == Some(item) {
            *control.selected_item = Some(item);
            *control.editing_item = Some(item);
            *control.queued_edit_item = None;
            control.inline_edit_text.clone_from(&items[index]);
            control.inline_edit_original_text.clone_from(&items[index]);
            *control.focus_inline_edit = true;
        }

        if *control.editing_item == Some(item) {
            if show_inline_list_editor(ui, items, item, control, row_width) {
                *control.viewport_start = clamp_settings_list_viewport_start(
                    *control.viewport_start,
                    items.len(),
                    SETTINGS_LIST_VISIBLE_ROWS,
                );
                if let Some(selected) = (*control.selected_item).and_then(|item| {
                    (item.kind == kind && item.index < items.len()).then_some(item.index)
                }) {
                    ensure_settings_list_item_visible(
                        control.viewport_start,
                        selected,
                        items.len(),
                        SETTINGS_LIST_VISIBLE_ROWS,
                    );
                }
                continue;
            }
        } else {
            let selected = *control.selected_item == Some(item);
            let response = selectable_list_row(ui, row_width, selected, items[index].as_str())
                .on_hover_text(items[index].as_str());
            if response.double_clicked() {
                *control.selected_item = Some(item);
                if control.editing_item.is_none() {
                    *control.editing_item = Some(item);
                    control.inline_edit_text.clone_from(&items[index]);
                    control.inline_edit_original_text.clone_from(&items[index]);
                    *control.focus_inline_edit = true;
                } else {
                    *control.queued_edit_item = Some(item);
                }
            } else if response.clicked() {
                *control.selected_item = Some(item);
                if control.editing_item.is_none() {
                    control.inline_edit_text.clear();
                    control.inline_edit_original_text.clear();
                    *control.focus_inline_edit = false;
                }
            }
        }
        index += 1;
    }
}

fn clamp_settings_list_viewport_start(
    viewport_start: usize,
    item_count: usize,
    visible_rows: usize,
) -> usize {
    if visible_rows == 0 || item_count <= visible_rows {
        0
    } else {
        viewport_start.min(item_count - visible_rows)
    }
}

fn ensure_settings_list_item_visible(
    viewport_start: &mut usize,
    item_index: usize,
    item_count: usize,
    visible_rows: usize,
) {
    *viewport_start = clamp_settings_list_viewport_start(*viewport_start, item_count, visible_rows);

    if visible_rows == 0 || item_count == 0 || item_index >= item_count {
        return;
    }

    if item_index < *viewport_start {
        *viewport_start = item_index;
    } else if item_index >= *viewport_start + visible_rows {
        *viewport_start = item_index + 1 - visible_rows;
    }

    *viewport_start = clamp_settings_list_viewport_start(*viewport_start, item_count, visible_rows);
}

fn show_inline_list_editor(
    ui: &mut egui::Ui,
    items: &mut Vec<String>,
    item: SettingsListItem,
    control: &mut EditableListControl<'_>,
    editor_width: f32,
) -> bool {
    let response = ui.add(
        egui::TextEdit::singleline(control.inline_edit_text)
            .desired_width(editor_width)
            .clip_text(false),
    );
    if *control.focus_inline_edit {
        response.request_focus();
        *control.focus_inline_edit = false;
    }

    let pressed_enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
    let pressed_escape = ui.input(|input| input.key_pressed(egui::Key::Escape));

    if response.has_focus() && pressed_escape {
        *control.editing_item = None;
        control.inline_edit_text.clear();
        control.inline_edit_original_text.clear();
        *control.focus_inline_edit = false;
        return false;
    }

    if response.lost_focus() || response.has_focus() && pressed_enter {
        commit_inline_list_edit(
            items,
            item,
            control.selected_item,
            control.editing_item,
            control.inline_edit_text,
            control.dirty,
        );
        control.inline_edit_original_text.clear();
        return true;
    }

    false
}

fn commit_inline_list_edit(
    items: &mut Vec<String>,
    item: SettingsListItem,
    selected_item: &mut Option<SettingsListItem>,
    editing_item: &mut Option<SettingsListItem>,
    inline_edit_text: &mut String,
    dirty: &mut bool,
) {
    let index = item.index;
    if index >= items.len() {
        *editing_item = None;
        inline_edit_text.clear();
        return;
    }

    match list_item_from_text(inline_edit_text) {
        Some(new_item) if new_item != items[index] => {
            items[index] = new_item;
            *dirty = true;
        }
        None => {
            items.remove(index);
            *selected_item = if items.is_empty() {
                None
            } else {
                Some(SettingsListItem {
                    kind: item.kind,
                    index: index.min(items.len() - 1),
                })
            };
            *dirty = true;
        }
        Some(_) => {}
    }

    *editing_item = None;
    inline_edit_text.clear();
}

fn remove_selected_list_item(
    kind: SettingsListKind,
    items: &mut Vec<String>,
    selected_item: &mut Option<SettingsListItem>,
    editing_item: &mut Option<SettingsListItem>,
) {
    let Some(selected) = *selected_item else {
        return;
    };
    if selected.kind != kind || selected.index >= items.len() {
        return;
    }

    items.remove(selected.index);
    *editing_item = None;
    *selected_item = if items.is_empty() {
        None
    } else {
        Some(SettingsListItem {
            kind,
            index: selected.index.min(items.len() - 1),
        })
    };
}

fn list_item_from_text(text: &str) -> Option<String> {
    (!text.trim().is_empty()).then(|| text.to_owned())
}

fn finite_settings_list_width(width: f32) -> f32 {
    if width.is_finite() {
        width.max(1.0)
    } else {
        SETTINGS_LIST_FALLBACK_WIDTH
    }
}

fn settings_list_frame_inner_margin() -> egui::Margin {
    egui::Margin::symmetric(8, 6)
}

fn settings_list_content_width(outer_width: f32, inner_margin: egui::Margin) -> f32 {
    let horizontal_margin = f32::from(inner_margin.left + inner_margin.right);
    finite_settings_list_width(outer_width - horizontal_margin)
}

fn selectable_list_row(
    ui: &mut egui::Ui,
    width: f32,
    selected: bool,
    text: &str,
) -> egui::Response {
    let desired_size = egui::vec2(width, ui.spacing().interact_size.y);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let visible_frame = selected
            || response.hovered()
            || response.is_pointer_button_down_on()
            || response.has_focus();
        if visible_frame {
            let stroke = visuals.bg_stroke;
            ui.painter().rect(
                rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                stroke,
                egui::StrokeKind::Outside,
            );
        }

        let text_pos = egui::pos2(rect.left() + ui.spacing().button_padding.x, rect.center().y);
        let text_clip_rect = rect.shrink2(egui::vec2(ui.spacing().button_padding.x, 0.0));
        ui.painter().with_clip_rect(text_clip_rect).text(
            text_pos,
            egui::Align2::LEFT_CENTER,
            text,
            egui::TextStyle::Button.resolve(ui.style()),
            visuals.text_color(),
        );
    }

    response
}

impl SettingsListKind {
    fn add_window_title(self) -> &'static str {
        match self {
            Self::GrassIds => "Add grass ID pattern",
            Self::Exclude => "Add exclude pattern",
            Self::IgnoredPlugins => "Add ignored plugin",
        }
    }

    fn add_prompt(self) -> &'static str {
        match self {
            Self::GrassIds => "Grass ID pattern",
            Self::Exclude => "Exclude pattern",
            Self::IgnoredPlugins => "Ignored plugin",
        }
    }
}

fn output_path_frame(ui: &mut egui::Ui, path: &Path) {
    egui::Frame::group(ui.style())
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.monospace(path.display().to_string());
        });
}

#[cfg(test)]
mod tests {
    use super::{
        SettingsListItem, SettingsListKind, SettingsUiState, clamp_settings_list_viewport_start,
        commit_inline_list_edit, ensure_settings_list_item_visible, list_item_from_text,
        remove_selected_list_item,
    };

    #[test]
    fn list_item_from_text_preserves_text_and_rejects_empty_text() {
        assert_eq!(
            list_item_from_text("  grass_*  ").as_deref(),
            Some("  grass_*  ")
        );
        assert_eq!(list_item_from_text("   "), None);
    }

    #[test]
    fn remove_selected_list_item_keeps_selection_on_remaining_item() {
        let mut items = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let mut selected = Some(SettingsListItem {
            kind: SettingsListKind::Exclude,
            index: 1,
        });
        let mut editing = selected;

        remove_selected_list_item(
            SettingsListKind::Exclude,
            &mut items,
            &mut selected,
            &mut editing,
        );

        assert_eq!(items, ["a", "c"]);
        assert_eq!(
            selected,
            Some(SettingsListItem {
                kind: SettingsListKind::Exclude,
                index: 1,
            })
        );
        assert_eq!(editing, None);
    }

    #[test]
    fn empty_inline_edit_removes_item_and_updates_selection() {
        let mut items = vec!["a".to_owned(), "b".to_owned()];
        let edited = SettingsListItem {
            kind: SettingsListKind::GrassIds,
            index: 0,
        };
        let mut selected = Some(edited);
        let mut editing = Some(edited);
        let mut text = "  ".to_owned();
        let mut dirty = false;

        commit_inline_list_edit(
            &mut items,
            edited,
            &mut selected,
            &mut editing,
            &mut text,
            &mut dirty,
        );

        assert_eq!(items, ["b"]);
        assert_eq!(
            selected,
            Some(SettingsListItem {
                kind: SettingsListKind::GrassIds,
                index: 0,
            })
        );
        assert_eq!(editing, None);
        assert!(text.is_empty());
        assert!(dirty);
    }

    #[test]
    fn active_inline_edit_counts_as_dirty_before_commit() {
        let mut settings = SettingsUiState {
            editing_list_item: Some(SettingsListItem {
                kind: SettingsListKind::IgnoredPlugins,
                index: 0,
            }),
            inline_edit_original_text: "Old.esp".to_owned(),
            inline_edit_text: "New.esp".to_owned(),
            ..SettingsUiState::default()
        };

        assert!(settings.is_dirty());

        settings.inline_edit_text = "Old.esp".to_owned();

        assert!(!settings.is_dirty());
    }

    #[test]
    fn list_viewport_clamps_to_valid_start() {
        assert_eq!(clamp_settings_list_viewport_start(4, 3, 6), 0);
        assert_eq!(clamp_settings_list_viewport_start(99, 10, 6), 4);
        assert_eq!(clamp_settings_list_viewport_start(2, 10, 6), 2);
    }

    #[test]
    fn list_viewport_ensure_visible_preserves_absolute_indices() {
        let mut viewport_start = 0;

        ensure_settings_list_item_visible(&mut viewport_start, 8, 10, 6);
        assert_eq!(viewport_start, 3);

        ensure_settings_list_item_visible(&mut viewport_start, 2, 10, 6);
        assert_eq!(viewport_start, 2);

        ensure_settings_list_item_visible(&mut viewport_start, 5, 10, 6);
        assert_eq!(viewport_start, 2);
    }
}
