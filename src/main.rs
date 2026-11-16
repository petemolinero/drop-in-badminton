//! Drop-in Badminton — court rotation for pickup badminton nights.

// No console window behind the app on Windows (release builds).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod engine;
mod hall;
mod model;
mod store;
mod surfaces;
mod theme;
mod update;
mod widgets;

fn main() -> eframe::Result {
    update::remember_exe();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Drop-in Badminton")
            .with_app_id("drop-in-badminton")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_maximized(true),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Drop-in Badminton",
        options,
        Box::new(|cc| Ok(Box::new(app::DropInApp::new(cc)))),
    );
    // After an update, start the new version once this one has saved and closed.
    update::restart_if_requested();
    result
}
