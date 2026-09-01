//! `pros-gui` - the window over the target library.
//!
//! # It holds no logic, and the dependency list is what enforces that
//!
//! The same rule as `pros` and as the sibling project's shell: the crates are the product,
//! and this is an interaction surface over them. Every decision it presents - what a
//! registration is, what a missing loader means, whether an answer was slow enough to
//! remark on - is made in `pros-core` or `pros-link` and is reachable from the command line
//! too.
//!
//! # What is testable here, and what is not
//!
//! A window cannot be looked at from a test, and on the machine this was written on it
//! cannot be looked at at all. So the split is deliberate:
//!
//! - [`state`] holds the rules about what may run and what a failure does to what is on
//!   screen. Tested.
//! - [`work`] does the asking on another thread, so a slow target does not stop the window
//!   repainting - a window that has stopped repainting and one that has crashed look the
//!   same. Tested.
//! - [`app`] draws. Only the wording is tested, because the wording has to match what `pros`
//!   says about the same finding.
//!
//! **What has not been established here is that a window appears.** That needs somebody to
//! run it, and saying so is cheaper than implying otherwise.

mod app;
mod journal;
mod listing;
mod state;
mod sweep;
mod tail;
mod work;

/// Opens the window.
fn main() -> eframe::Result<()> {
    // Bound to `main` rather than to the app: `run_native` does not return until the window
    // closes, so this outlives every frame. Dropping it early would stop the writers while the
    // window was still running, which is the one way to lose the log of a session.
    let _logging = oops_log::Logging::new("pros-gui")
        .build(pros_core::build::line_static())
        .init();
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1000.0, 720.0])
        .with_min_inner_size([700.0, 480.0])
        // "Prosperous GUI" rather than "Prosperous" so the window can be picked
        // out by title. Window matching is substring and case-insensitive, and a
        // terminal or editor showing a path under `prosperous/` matches the bare
        // name - which is how the documentation screenshots ended up capturing a
        // terminal instead of this window. A space is enough: no path has one here.
        .with_title("Prosperous GUI");

    // The same logo the readme and the site use, so the taskbar entry is recognisable as
    // this program rather than as whatever the platform picks for an unmarked window.
    //
    // `include_bytes!` resolves relative to *this file*, which is why the path is here and
    // not behind a shared helper: a crate in oops-libs cannot embed a consumer's asset, the
    // same constraint that keeps each application's documentation registry in the
    // application.
    match eframe::icon_data::from_png_bytes(include_bytes!("../../assets/logo.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // Reported and carried on. A window wearing the platform's default icon is worth
        // more than no window, and a silent fallback would leave the reason invisible.
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
