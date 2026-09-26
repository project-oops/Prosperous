//! `pros-gui` - the window over the target library.
//!
//! It holds no logic: every decision it presents is made in `pros-core` or `pros-link` and is
//! reachable from `pros` too. A window cannot be inspected from a test, so the testable parts
//! are kept apart from the drawing:
//!
//! - [`state`] holds what may run and what a failure does to what is on screen.
//! - [`work`] asks the target on another thread, so a slow target does not stop repainting.
//! - [`app`] draws; its wording is tested against what `pros` says about the same finding.

mod app;
mod journal;
mod listing;
mod probe;
mod state;
mod sweep;
mod tail;
mod work;

/// Opens the window.
fn main() -> eframe::Result<()> {
    // Held by `main`: `run_native` returns only when the window closes, so the log writers
    // outlive every frame.
    let _logging = oops_log::Logging::new("pros-gui")
        .build(pros_core::build::line_static())
        .init();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 720.0])
        .with_min_inner_size([700.0, 480.0])
        // Not the bare "Prosperous": window matching by title is a case-insensitive substring,
        // and a terminal showing a path under `prosperous/` would match it. No path has a space.
        .with_title("Prosperous GUI");

    // `include_bytes!` resolves relative to this file, so a shared crate cannot embed it.
    match eframe::icon_data::from_png_bytes(include_bytes!("../../assets/logo.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // The window opens with the default icon; the reason is printed, not swallowed.
        Err(e) => eprintln!("prosperous: window icon: {e}"),
    }

    let options = eframe::NativeOptions {
        viewport,
        ..eframe::NativeOptions::default()
    };
    eframe::run_native(
        "prosperous",
        options,
        Box::new(|_cc| Ok(Box::new(app::App::new()))),
    )
}
