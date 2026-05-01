use std::io;

use eframe::egui;

struct GreenmoteApp;

impl eframe::App for GreenmoteApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("greenmote_convert_panel")
            .resizable(false)
            .exact_width(ctx.available_rect().width() / 3.0)
            .show(ctx, |ui| {
                ui.add_space(12.0);
                ui.vertical_centered_justified(|ui| {
                    let _response = ui.button("Convert");
                });
            });

        egui::CentralPanel::default().show(ctx, |_ui| {});
    }
}

/// Runs the `greenmote` graphical user interface.
///
/// # Errors
///
/// Returns an I/O-shaped error when the underlying GUI platform cannot create or run the window.
pub fn run() -> io::Result<()> {
    eframe::run_native(
        "Greenmote",
        eframe::NativeOptions::default(),
        Box::new(|_creation_context| Ok(Box::new(GreenmoteApp))),
    )
    .map_err(|error| io::Error::other(error.to_string()))
}
