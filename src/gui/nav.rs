use eframe::egui;

use super::{GreenmoteApp, Screen};

const NAV_BAR_VERTICAL_PADDING: f32 = 8.0;
const NAV_BUTTON_MIN_WIDTH: f32 = 96.0;

#[derive(Default)]
pub(super) struct NavUiState {
    metrics: Option<NavMetrics>,
}

#[derive(Clone, Copy)]
struct NavEntry {
    label: &'static str,
    screen: Screen,
}

struct NavButton {
    entry: NavEntry,
    size: egui::Vec2,
}

struct NavMetrics {
    style_key: NavStyleKey,
    buttons: Vec<NavButton>,
    width: f32,
    button_height: f32,
}

#[derive(Clone, PartialEq, Eq)]
struct NavStyleKey {
    pixels_per_point: u32,
    interact_width: u32,
    interact_height: u32,
    button_padding_x: u32,
    button_padding_y: u32,
    item_spacing_x: u32,
    button_font_family: egui::FontFamily,
    button_font_size: u32,
    button_frame: bool,
}

pub(super) fn nav_bar_height(ctx: &egui::Context) -> f32 {
    nav_button_height(&ctx.style()) + (NAV_BAR_VERTICAL_PADDING * 2.0)
}

impl GreenmoteApp {
    pub(super) fn show_navigation_bar(&mut self, ui: &mut egui::Ui) {
        const NAV_ENTRIES: &[NavEntry] = &[
            NavEntry {
                label: "Convert",
                screen: Screen::Convert,
            },
            NavEntry {
                label: "Settings",
                screen: Screen::Settings,
            },
        ];

        let current_screen = self.screen;
        let metrics = self.nav.metrics(ui, NAV_ENTRIES);
        let top_padding = ((ui.available_height() - metrics.button_height) / 2.0).max(0.0);
        let mut clicked_screen = None;

        ui.add_space(top_padding);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - metrics.width).max(0.0) / 2.0);

            for button in &metrics.buttons {
                if ui
                    .add_sized(
                        button.size,
                        egui::Button::new(button.entry.label)
                            .selected(current_screen == button.entry.screen),
                    )
                    .clicked()
                {
                    clicked_screen = Some(button.entry.screen);
                }
            }
        });

        if let Some(screen) = clicked_screen {
            self.request_screen(screen);
        }
    }
}

impl NavUiState {
    fn metrics(&mut self, ui: &egui::Ui, entries: &[NavEntry]) -> &NavMetrics {
        let style_key = NavStyleKey::new(ui);
        if self
            .metrics
            .as_ref()
            .is_none_or(|metrics| metrics.style_key != style_key)
        {
            self.metrics = Some(NavMetrics::new(ui, entries, style_key.clone()));
        }

        self.metrics
            .as_ref()
            .expect("navbar metrics were just initialized")
    }
}

impl NavMetrics {
    fn new(ui: &egui::Ui, entries: &[NavEntry], style_key: NavStyleKey) -> Self {
        let buttons = entries
            .iter()
            .map(|entry| NavButton {
                entry: *entry,
                size: nav_button_size(ui, entry.label),
            })
            .collect::<Vec<_>>();
        let width = nav_width(ui, &buttons);
        let button_height = buttons
            .iter()
            .map(|button| button.size.y)
            .fold(0.0, f32::max);

        Self {
            style_key,
            buttons,
            width,
            button_height,
        }
    }
}

impl NavStyleKey {
    fn new(ui: &egui::Ui) -> Self {
        let spacing = ui.spacing();
        let button_font = egui::TextStyle::Button.resolve(ui.style());

        Self {
            pixels_per_point: ui.ctx().pixels_per_point().to_bits(),
            interact_width: spacing.interact_size.x.to_bits(),
            interact_height: spacing.interact_size.y.to_bits(),
            button_padding_x: spacing.button_padding.x.to_bits(),
            button_padding_y: spacing.button_padding.y.to_bits(),
            item_spacing_x: spacing.item_spacing.x.to_bits(),
            button_font_family: button_font.family,
            button_font_size: button_font.size.to_bits(),
            button_frame: ui.visuals().button_frame,
        }
    }
}

fn nav_button_size(ui: &egui::Ui, label: &str) -> egui::Vec2 {
    egui::vec2(
        NAV_BUTTON_MIN_WIDTH.max(nav_button_desired_width(ui, label)),
        nav_button_height(ui.style()),
    )
}

fn nav_button_height(style: &egui::Style) -> f32 {
    let font_id = egui::TextStyle::Button.resolve(style);

    style
        .spacing
        .interact_size
        .y
        .max(font_id.size + (style.spacing.button_padding.y * 2.0))
}

fn nav_button_desired_width(ui: &egui::Ui, label: &str) -> f32 {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font_id, ui.visuals().text_color())
        .size()
        .x;

    let horizontal_padding = if ui.visuals().button_frame {
        ui.spacing().button_padding.x * 2.0
    } else {
        0.0
    };

    horizontal_padding + text_width
}

fn nav_width(ui: &egui::Ui, buttons: &[NavButton]) -> f32 {
    let button_widths = buttons.iter().map(|button| button.size.x).sum::<f32>();
    let spacing_widths = buttons
        .windows(2)
        .map(|_window| ui.spacing().item_spacing.x)
        .sum::<f32>();

    button_widths + spacing_widths
}
