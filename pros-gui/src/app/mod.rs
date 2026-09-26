//! Drawing, and nothing else.
//!
//! This module reads state and draws it. Every decision (what a registration is, what a
//! missing loader means, which files a loader accepts) lives in a crate below this one and is
//! reachable from `pros` too. The rules for changing state live in [`crate::state`], where
//! they can be tested without a window.
//!
//! Immediate mode fits because a check is a table replaced wholesale on every run, not a form
//! edited field by field.

mod autoload;
mod check;
mod confirm;
mod controllers;
mod docs;
mod doctor;
mod export;
mod files;
mod log;
mod menu;
mod payloads;
mod probing;
mod shell;
mod startup;
mod stream;
mod system;
mod widgets;

use std::time::Duration;

use pros_core::manifest::Manifest;
use pros_core::target;

use crate::state::{Job, Section, State};
use crate::work::Worker;
use docs::DOCS;
use system::ProcSort;
use widgets::{section_heading, size};

/// Where the manager keeps the payload files it loads.
///
/// Measured on a target: one folder per payload, with the file inside it.
const PAYLOADS: &str = "/data/pldmgr/payloads";

/// What the separator between two panes costs, with its padding.
const GAP: f32 = 24.0;

/// How often the system panel re-reads the target when auto-refresh is on.
///
/// Each tick is a shell round trip. The same interval as the default of `pros top`.
const SYSTEM_REFRESH: Duration = Duration::from_secs(3);

/// The window.
pub(crate) struct App {
    /// The documentation reader. Holds which page is open and the parsed form of the ones
    /// already looked at, so the markdown is not re-parsed on every frame.
    docs: oops_docs::DocsWindow,
    state: State,
    worker: Worker,
    stamp: String,
    /// What is described, when a manifest has been read.
    manifest: Option<Manifest>,
    /// Which services exist and what each is for: defaults, then this project's own file.
    ///
    /// Read once, at start: it says what a service means, not what a target is doing, so it
    /// does not expire on a power cycle.
    catalogue: pros_core::catalogue::Catalogue,
    /// The log being followed, when one is.
    ///
    /// Beside the worker rather than inside it: the worker runs one job at a time, and a
    /// subscription would block everything else the window can do.
    tail: Option<crate::tail::Tail>,
    /// The probe running, or the last one, when there is one.
    ///
    /// Beside the worker for the same reason as the log: a probe is a launch followed by a
    /// subscription lasting up to its cap.
    probe: Option<crate::probe::Run>,
    /// What each payload's own project has released, as far as anything has asked.
    ///
    /// Read from disk at start and written back as answers arrive, so the next launch does not
    /// ask again and hit the rate limit.
    sources: pros_core::sources::Sources,
    /// Whether the sweep that runs on its own has been started.
    ///
    /// A flag rather than starting it in `new`, so the window opens before any asking starts.
    asked_at_launch: bool,
    /// A sweep of those projects, while one is running.
    ///
    /// Beside the worker because it is deliberately slow (spaced out, waiting out refusals) and
    /// would block the one-job queue for its whole length.
    sweep: Option<crate::sweep::Sweep>,
    /// How the system panel's process list is ordered: a view choice, not a fact about the
    /// target.
    system_sort: ProcSort,
    /// Whether the system panel re-reads the target on its own.
    ///
    /// With it on, the panel re-runs `ReadSystem` when idle and the interval has passed. Off by
    /// default: reading the target is a round trip, not something to do unasked.
    system_auto: bool,
    /// When the panel last asked the target, for the auto-refresh interval.
    system_asked_at: Option<std::time::Instant>,
}

impl App {
    /// Opens with whatever is registered on this machine.
    #[must_use]
    pub(crate) fn new() -> Self {
        // An unreadable registry does not stop the window: it can still register a target.
        let targets = target::load().unwrap_or_default();
        // The manifest in the usual place, or the built-in recommended list, so a fresh install
        // shows what a target ought to be running.
        let manifest = pros_core::manifest::Tracked::Payloads
            .read()
            .ok()
            .or_else(|| {
                let seed = pros_core::manifest::recommended();
                let _ = seed.save();
                Some(seed)
            });

        Self {
            state: State::new(targets),
            worker: Worker::new(),
            stamp: pros_core::build::line(),
            docs: oops_docs::DocsWindow::default(),
            tail: None,
            probe: None,
            sweep: None,
            asked_at_launch: false,
            sources: pros_core::sources::load(),
            manifest,
            // Defaults when no file exists, the normal case (`pros_core::catalogue`).
            catalogue: pros_core::catalogue::load()
                .unwrap_or_else(|_| pros_core::catalogue::Catalogue::builtin()),
            system_sort: ProcSort::default(),
            system_auto: false,
            system_asked_at: None,
        }
    }

    /// The sidebar: which target, watching it, and what to do with it.
    ///
    /// Registering is in the menu and at the bottom of the target list, not a form here.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let chosen = self
            .state
            .target()
            .map_or_else(|| "no target".to_owned(), |target| target.name.clone());
        egui::ComboBox::from_id_salt("target")
            .selected_text(chosen)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for which in 0..self.state.targets.len() {
                    let label = self.state.targets[which].name.clone();
                    ui.selectable_value(&mut self.state.chosen, Some(which), label);
                }
                // At the bottom of the list, where a missing target is noticed.
                ui.separator();
                if ui.button("register...").clicked() {
                    self.state.register.editing = None;
                    self.state.showing.registering = true;
                }
            });

        ui.add_space(8.0);

        for (group, sections) in Section::GROUPS {
            ui.add_space(4.0);
            ui.small(group);
            ui.separator();
            for section in sections {
                ui.selectable_value(&mut self.state.section, *section, section.name());
            }
        }
    }

    /// Clears everything known about the previous target, so no answer is shown under
    /// another target's name.
    fn forget_the_last_target(&mut self) {
        self.state.report = None;
        self.state.chain = None;
        self.state.files.located = None;
        self.state.system = None;
        self.state.autoload.settings = None;
        self.state.autoload.boot = None;
        self.state.payloads.there = None;
        self.state.files.names.clear();
        // Another machine has other titles installed.
        self.state.probing.titles = None;
        self.state.probing.id = None;
    }

    /// Asks the target everything the window will need, as soon as one is selected.
    ///
    /// Four reads, in the order their answers are needed: the check, which qualifies every
    /// panel's advice; the payloads the target holds, so no panel recommends fetching one that
    /// is there; the startup list and settings; and the system report. The first starts now
    /// and the rest queue behind it; a failure stops the rest, so the order is by importance.
    fn survey_on_arrival(&mut self) {
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // A different target clears what is on screen; a re-survey of the same one does not.
        let elsewhere = self.state.checked_for.as_deref() != Some(target.name.as_str());
        if !elsewhere && !self.state.resurvey {
            return;
        }
        if !self.state.is_idle() {
            return;
        }
        self.state.resurvey = false;
        if elsewhere {
            self.forget_the_last_target();
        }
        self.state.checked_for = Some(target.name.clone());
        self.state.begin(Job::Check(target.clone()));
        self.state
            .queue(Job::FindPayloads(target.clone(), PAYLOADS.to_owned()));
        self.state.queue(Job::ReadAutoload(target.clone()));
        // Last, because no other panel depends on it.
        self.state.queue(Job::ReadSystem(target));
    }

    /// Reads the system report when the system screen is opened and none is held.
    fn system_on_arrival(&mut self) {
        if self.state.section != Section::System
            || self.state.system.is_some()
            || !self.state.is_idle()
        {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // Not before the check, which says whether the shell answers at all.
        if self.state.report.is_none() {
            return;
        }
        self.state.begin(Job::ReadSystem(target));
    }

    /// Reads the manager's settings when the autoload screen is opened, then the payload scan,
    /// which the list needs to mark missing entries.
    fn autoload_on_arrival(&mut self) {
        if self.state.section != Section::Autoload
            || !self.state.is_idle()
            || self.state.report.is_none()
        {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        if self.state.autoload.settings.is_none() {
            self.state.begin(Job::ReadAutoload(target));
        } else if self.state.payloads.there.is_none() {
            self.state
                .begin(Job::FindPayloads(target, PAYLOADS.to_owned()));
        }
    }

    /// Starts following the log when somebody opens that screen.
    ///
    /// Tried once per target; a failure leaves the button rather than retrying every frame.
    fn follow_on_arrival(&mut self) {
        if self.state.section != Section::Log || self.tail.is_some() {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        if self.state.log.followed_for.as_deref() == Some(target.name.as_str()) {
            return;
        }
        self.state.log.followed_for = Some(target.name.clone());
        match crate::tail::Tail::start(&target.name, &target.link()) {
            Ok(tail) => {
                self.state.log.lines.clear();
                self.tail = Some(tail);
            }
            // A log service that is not loaded is a normal state the check already reports.
            Err(why) => self.state.trouble = Some(why),
        }
    }

    /// Lists what is installed when the probe screen is opened, so it can offer titles.
    ///
    /// Asked once per target, like the log; the refresh button asks again.
    fn titles_on_arrival(&mut self) {
        if self.state.section != Section::Probe
            || self.state.probing.titles.is_some()
            || !self.state.is_idle()
        {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        if self.state.probing.titles_for.as_deref() == Some(target.name.as_str()) {
            return;
        }
        self.state.probing.titles_for = Some(target.name.clone());
        self.state.begin(Job::Titles(target));
    }

    /// Asks the target which of this section's candidate directories it has.
    ///
    /// Only for sections with more than one candidate and no standard among them. Asked once
    /// per section per target.
    fn locate_on_arrival(&mut self) {
        let candidates = self.state.section.candidates();
        let answered = self
            .state
            .files
            .located
            .as_ref()
            .is_some_and(|(asked, _)| *asked == self.state.section);
        if candidates.is_empty() || answered || !self.state.is_idle() {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // Not before the check, which says whether the file service is up.
        if self.state.report.is_none() {
            return;
        }
        self.state.begin(Job::Locate(target, candidates));
    }

    /// A screen whose first answer is still on its way.
    ///
    /// A sentence instead of the panel with greyed controls over empty rows. A re-read of a
    /// screen that already has content never gets here
    /// ([`crate::state::State::still_arriving`]).
    fn arriving(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, self.state.section);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            // Which question, not just loading: answers arrive in sequence.
            ui.label(
                self.state
                    .waiting
                    .as_ref()
                    .map_or_else(|| "waiting its turn".to_owned(), |one| one.job.describe()),
            );
        });
        let queued = self.state.queued();
        if queued > 0 {
            ui.add_space(4.0);
            ui.weak(format!("{queued} more to ask after this one"));
        }
        ui.add_space(6.0);
        ui.weak("this screen opens as soon as its answer arrives - later refreshes leave it up");
    }

    /// Draws whichever section the sidebar has selected.
    fn section(&mut self, ui: &mut egui::Ui) {
        // First gate: a section names the service it needs, and the last check says whether
        // it answers. Nothing here probes for itself.
        if let Some(needed) = self.state.section.requires()
            && !self.needs(ui, needed)
        {
            return;
        }
        // Second gate: a screen whose first answer is queued but not arrived says so, rather
        // than reading as never asked. One place, so every screen behaves the same.
        if self.state.still_arriving(self.state.section) {
            self.arriving(ui);
            return;
        }
        // A screen with content stays up while it is re-read, with a note so the old reading
        // is not taken for the new one.
        if self.state.re_reading(self.state.section) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("reading again - what is below is from a moment ago");
            });
        }
        match self.state.section {
            Section::Check => self.check_panel(ui),
            Section::Stream => self.stream_panel(ui),
            Section::Autoload => self.autoload_panel(ui),
            Section::System => self.system_panel(ui),
            Section::Controllers => self.controllers_panel(ui),
            Section::Payloads => {
                // Settled like every two-sided section, so the target pane starts at the
                // payload directory.
                self.settle(Section::Payloads);
                self.payloads_body(ui);
            }
            Section::Log => self.log_panel(ui),
            Section::Probe => self.probe_panel(ui),
            Section::Shell => self.shell_panel(ui),
            // One view; these sections differ only in where each side starts.
            section @ (Section::Packages
            | Section::Titles
            | Section::Saves
            | Section::Cheats
            | Section::Filesystem) => {
                self.settle(section);
                self.sync_body(ui);
            }
        }
    }

    /// What is happening, and what went wrong.
    ///
    /// Waiting is shown with its own clock, so a working window does not look hung.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        // The activity record opens above the status line rather than replacing it.
        if self.state.journal.open {
            self.activity(ui);
            ui.separator();
        }
        ui.horizontal(|ui| {
            let troubles = self.state.journal.troubles();
            let count = self.state.journal.all().len();
            let arrow = if self.state.journal.open { "v" } else { ">" };
            // Both counts on the closed bar.
            let label = if troubles > 0 {
                format!("{arrow} activity  ({count}, {troubles} failed)")
            } else {
                format!("{arrow} activity  ({count})")
            };
            if ui
                .selectable_label(self.state.journal.open, label)
                .on_hover_text("everything this program has done this session")
                .clicked()
            {
                self.state.journal.open = !self.state.journal.open;
            }
            ui.separator();

            if let Some(waiting) = &self.state.waiting {
                ui.spinner();
                ui.label(format!(
                    "{} … {:.1}s",
                    waiting.job.describe(),
                    waiting.elapsed().as_secs_f32()
                ));
                // Stops a long copy after the file in flight, keeping the account of what
                // was copied.
                if ui
                    .small_button("stop")
                    .on_hover_text("finish the file in flight, then stop and say what was left")
                    .clicked()
                {
                    self.worker.stop();
                }
                // The queue is shown, so every queued job is visible and clearable.
                let waiting_turn = self.state.queued();
                if waiting_turn > 0 {
                    ui.weak(format!("{waiting_turn} queued"));
                    if ui
                        .small_button("clear queue")
                        .on_hover_text(
                            "forget what has not started - what is running now is not touched",
                        )
                        .clicked()
                    {
                        let dropped = self.state.drop_queued();
                        self.state.said = format!("{dropped} were dropped before starting");
                    }
                }
                // What is going across right now, when there is one.
                if let Some(progress) = &self.state.progress {
                    ui.weak(format!(
                        "{} files, {} - {}",
                        progress.files,
                        size(progress.bytes),
                        progress.current
                    ));
                }
            } else if let Some(trouble) = &self.state.trouble {
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), trouble);
            } else {
                ui.weak("idle");
            }
        });
    }

    /// Everything this program has done this session, newest first.
    ///
    /// The record keeps them in order; this shows them reversed, because the last one is
    /// usually what the panel is opened for.
    fn activity(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("activity");
            if ui
                .small_button("clear")
                .on_hover_text("forget what has finished - anything still running stays")
                .clicked()
            {
                self.state.journal.clear();
            }
            ui.weak("this program's own actions - the target's log is under diagnose");
        });

        egui::ScrollArea::vertical()
            .id_salt("activity")
            .max_height(180.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("activity-rows")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for entry in self.state.journal.all().iter().rev() {
                            let colour = match &entry.ending {
                                crate::journal::Ending::Failed(_) => {
                                    egui::Color32::from_rgb(220, 120, 120)
                                }
                                crate::journal::Ending::Refused(_) => {
                                    egui::Color32::from_rgb(210, 190, 120)
                                }
                                crate::journal::Ending::Running => {
                                    egui::Color32::from_rgb(140, 180, 220)
                                }
                                _ => egui::Color32::GRAY,
                            };
                            ui.colored_label(colour, entry.ending.word());
                            ui.label(&entry.what);
                            ui.weak(entry.target.as_deref().unwrap_or(""));
                            ui.weak(format!("{:.1}s", entry.elapsed().as_secs_f32()));
                            ui.weak(entry.ending.said().unwrap_or(""));
                            ui.end_row();
                        }
                    });
                if self.state.journal.all().is_empty() {
                    ui.weak("nothing yet this session");
                }
            });
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.take_answers(ctx);
        self.take_probe(ctx);
        self.reread_disturbed();
        // A plan's list edits wait for its transfers (`finish_deferred_edits`).
        self.finish_deferred_edits();
        // Keep repainting while something runs, so the clock in the status bar advances.
        if self.state.waiting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // And while a stream runs, because its counters update on another thread.
        if self.state.stream.watching.counts().status.is_watching() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // Input runs from here, not from the panel that draws it (`drive_pads`).
        self.drive_pads(ctx);
        // A held key produces no events, so the window repaints to keep sending it.
        if self.state.controllers.pads.filled() > 0 {
            ctx.request_repaint();
        }

        self.survey_on_arrival();
        self.locate_on_arrival();
        self.system_on_arrival();
        self.autoload_on_arrival();
        self.follow_on_arrival();
        self.titles_on_arrival();
        self.draw(ctx);

        // Unconditionally, after everything that could have begun a job this frame.
        if let Some(job) = self.state.pending.take() {
            self.worker.start(job);
            ctx.request_repaint();
        }
    }
}

impl App {
    /// Takes in what has arrived since the last frame: the worker's answer, where the target
    /// said to go, a new release, the sweep's answers and the log's lines.
    fn take_answers(&mut self, ctx: &egui::Context) {
        match self.worker.collect() {
            // Progress ends nothing: it says how far, and the status bar shows it.
            Some(crate::work::Update::Progress(progress)) => self.state.progress = Some(progress),
            Some(crate::work::Update::Finished(done)) => {
                self.state.progress = None;
                self.state.finish(done);
            }
            None => {}
        }
        // Somewhere the target told us to go, once it had been asked.
        if let Some(path) = self.state.files.go_to.take() {
            self.state.files.library_path = path;
            self.browse();
        }
        if let Some((payload, found)) = self.state.doctor.relisted.take() {
            self.take_relisted(payload, found);
        }
        // Once, on the first frame rather than in `new`, so the window opens before the slow
        // sweep starts.
        if !self.asked_at_launch {
            self.asked_at_launch = true;
            self.check_sources(false);
        }
        // Answers from the projects, as they come. This must follow the start above, so the
        // frame that starts a sweep also requests the repaint that drains it.
        if self.sweep.is_some() {
            self.take_sweep_answers();
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        // Lines since the last frame; a repaint only when something came.
        if let Some(tail) = &mut self.tail {
            if tail.drain(&mut self.state.log.lines) {
                ctx.request_repaint();
            }
            // A tail belongs to the target it was opened against, and closes when that changes.
            if self
                .state
                .target()
                .is_none_or(|now| now.name != tail.target)
            {
                self.tail = None;
            }
        }
    }

    /// Reads again whatever the last job reports it disturbed.
    fn reread_disturbed(&mut self) {
        for what in std::mem::take(&mut self.state.disturbed) {
            match what {
                crate::state::Disturbs::Here => self.read_local(),
                crate::state::Disturbs::There => {
                    // Everything cached about the target is a claim from before this job.
                    self.state.files.seen.clear();
                    self.browse();
                }
                // Re-surveyed, with the panel left on screen and marked as being re-read.
                crate::state::Disturbs::Report | crate::state::Disturbs::Autoload => {
                    self.state.resurvey = true;
                }
            }
        }
    }

    /// The window itself: the menus and windows, the status bar, the sidebar and the section.
    fn draw(&mut self, ctx: &egui::Context) {
        self.take_dropped(ctx);
        self.menu_bar(ctx);
        self.register_dialog(ctx);
        self.about_window(ctx);
        self.docs.show(ctx, DOCS);
        egui::TopBottomPanel::bottom("build")
            .show_separator_line(false)
            .show(ctx, |ui| ui.small(&self.stamp));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("sidebar")
            .default_width(190.0)
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            // Not gated on a target: most sections are about this machine too. Wrapped in a
            // scroll area only where the section does not scroll itself, since a nested one
            // gets unlimited height and never scrolls.
            if self.state.section.scrolls_itself() {
                self.section(ui);
            } else {
                // Both directions, so a table wider than the window gets a bar.
                egui::ScrollArea::both()
                    .id_salt(self.state.section.name())
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.section(ui));
            }
        });
    }
}

#[cfg(test)]
mod glyph_tests {
    /// Every file of this module, by name, so a failure says where.
    const SOURCES: &[(&str, &str)] = &[
        ("mod.rs", include_str!("mod.rs")),
        ("autoload.rs", include_str!("autoload.rs")),
        ("check.rs", include_str!("check.rs")),
        ("confirm.rs", include_str!("confirm.rs")),
        ("controllers.rs", include_str!("controllers.rs")),
        ("docs.rs", include_str!("docs.rs")),
        ("doctor.rs", include_str!("doctor.rs")),
        ("export.rs", include_str!("export.rs")),
        ("files.rs", include_str!("files.rs")),
        ("log.rs", include_str!("log.rs")),
        ("menu.rs", include_str!("menu.rs")),
        ("payloads.rs", include_str!("payloads.rs")),
        ("probing.rs", include_str!("probing.rs")),
        ("shell.rs", include_str!("shell.rs")),
        ("startup.rs", include_str!("startup.rs")),
        ("stream.rs", include_str!("stream.rs")),
        ("system.rs", include_str!("system.rs")),
        ("widgets.rs", include_str!("widgets.rs")),
    ];

    /// The source holds no escaped glyph the default font cannot draw (arrows, triangles).
    #[test]
    fn the_window_draws_nothing_a_font_might_not_have() {
        for (name, source) in SOURCES {
            for (number, line) in source.lines().enumerate() {
                // Such glyphs are written in the escape form.
                assert!(
                    !line.contains(concat!("\\", "u{2")),
                    "app/{name}:{} draws a glyph the font may not have: {}",
                    number + 1,
                    line.trim()
                );
            }
        }
    }
}
