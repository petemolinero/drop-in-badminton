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

fn main() {
    update::remember_exe();
    // Release builds on Windows have no console, so a crash would otherwise vanish
    // without a trace: log it and say so in a message box.
    std::panic::set_hook(Box::new(|info| {
        let place = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let what = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".into());
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        fail(&format!("crashed in thread '{thread}'{place}: {what}"));
    }));
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Drop-in Badminton")
            .with_app_id("drop-in-badminton")
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1000.0, 640.0])
            .with_maximized(true)
            .with_icon(std::sync::Arc::new(egui::IconData {
                rgba: include_bytes!("../assets/icon/app.rgba").to_vec(),
                width: 256,
                height: 256,
            })),
        ..Default::default()
    };
    let result = eframe::run_native(
        "Drop-in Badminton",
        options,
        Box::new(|cc| Ok(Box::new(app::DropInApp::new(cc)))),
    );
    if let Err(err) = result {
        fail(&format!("couldn't start: {err}"));
        std::process::exit(1);
    }
    // After an update, start the new version once this one has saved and closed.
    update::restart_if_requested();
}

/// Report a fatal problem: stderr, `crash.log` in the data folder, and on Windows a
/// message box (unless `DROP_IN_BADMINTON_NO_DIALOG` is set, for automated runs).
fn fail(what: &str) {
    use std::io::Write;
    let line = format!("{} v{} ({}): {what}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"), std::env::consts::OS);
    eprintln!("drop-in-badminton: {line}");
    let dir = model::data_dir();
    let log = dir.join("crash.log");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
        let _ = writeln!(f, "{line}");
    }
    #[cfg(windows)]
    if std::env::var_os("DROP_IN_BADMINTON_NO_DIALOG").is_none() {
        message_box(&format!(
            "Drop-in Badminton ran into a problem and has to close.\n\n{what}\n\nThis was saved to:\n{}",
            log.display()
        ));
    }
}

#[cfg(windows)]
fn message_box(text: &str) {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(hwnd: *mut std::ffi::c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
    }
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    const MB_ICONERROR: u32 = 0x10;
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
    unsafe { MessageBoxW(std::ptr::null_mut(), wide(text).as_ptr(), wide("Drop-in Badminton").as_ptr(), MB_ICONERROR) };
}
