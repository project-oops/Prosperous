//! The probe panel: launching a title and capturing the log around it.

use std::time::Duration;

use pros_core::target;

use super::App;
use super::log::{LogMatch, filter_controls, filtered_rows};
use super::widgets::{section_heading, section_heading_with};
use crate::state::{Job, Section};

impl App {
    /// The probe's steps and lines since the last frame, the way the log's are taken.
    ///
    /// Polled while it runs, not only when a line arrives, so a silent title's end is drawn.
    pub(super) fn take_probe(&mut self, ctx: &egui::Context) {
        let Some(run) = &mut self.probe else {
            return;
        };
        if run.drain(
            &mut self.state.probing.lines,
            &mut self.state.probing.status,
        ) {
            ctx.request_repaint();
        }
        if run.is_running() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        // A probe belongs to the target it launched on, like the log.
        if self.state.target().is_none_or(|now| now.name != run.target) {
            self.probe = None;
        }
    }

    /// An installed title, launched with the log already attached, and what it said.
    ///
    /// The `pros probe` loop without the deploy: close what the title left running, attach to
    /// the log, launch, and follow until it parks, exits or the cap passes (`pros_core::probe`).
    pub(super) fn probe_panel(&mut self, ui: &mut egui::Ui) {
        let Some(target) = self.state.target().cloned() else {
            section_heading(ui, Section::Probe);
            ui.label("no target selected");
            return;
        };
        let running = self
            .probe
            .as_ref()
            .is_some_and(crate::probe::Run::is_running);
        self.probe_toolbar(ui, &target, running);
        self.probe_capture(ui, &target, running);
    }

    /// The probe screen's heading: which title, for how long, and the button that starts it.
    fn probe_toolbar(&mut self, ui: &mut egui::Ui, target: &target::Target, running: bool) {
        let titles = self.state.probing.titles.clone().unwrap_or_default();
        section_heading_with(ui, Section::Probe, |ui| {
            let label = |about: &pros_core::titles::Metadata| match &about.name {
                Some(name) => format!("{}  {name}", about.id),
                None => about.id.clone(),
            };
            let chosen = self
                .state
                .probing
                .id
                .as_ref()
                .and_then(|id| titles.iter().find(|about| &about.id == id))
                .map_or_else(|| "choose a title".to_owned(), label);
            ui.add_enabled_ui(!running, |ui| {
                egui::ComboBox::from_id_salt("probe-title")
                    .selected_text(chosen)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for about in &titles {
                            ui.selectable_value(
                                &mut self.state.probing.id,
                                Some(about.id.clone()),
                                label(about),
                            );
                        }
                    });
            });
            if ui
                .add_enabled(
                    self.state.is_idle() && !running,
                    egui::Button::new("refresh"),
                )
                .on_hover_text("ask the target what is installed again")
                .clicked()
            {
                self.state.probing.titles_for = Some(target.name.clone());
                self.state.begin(Job::Titles(target.clone()));
            }
            ui.label("for");
            ui.add_enabled(
                !running,
                egui::DragValue::new(&mut self.state.probing.seconds)
                    .range(5..=3600)
                    .suffix("s"),
            )
            .on_hover_text(
                "the longest it follows the log - it stops sooner if the title parks or exits",
            );
            if running {
                if ui
                    .button("stop")
                    .on_hover_text(
                        "stop following - the title is left running; the dashboard Close, or \
                         the system screen, ends it",
                    )
                    .clicked()
                    && let Some(run) = &self.probe
                {
                    run.stop();
                }
                ui.spinner();
            } else if ui
                .add_enabled(
                    self.state.probing.id.is_some(),
                    egui::Button::new("launch and capture"),
                )
                .on_hover_text(
                    "close it if it is running, attach to the log, launch it, and keep what it \
                     says until it parks, exits, or the time is up",
                )
                .on_disabled_hover_text("choose a title first")
                .clicked()
                && let Some(id) = self.state.probing.id.clone()
            {
                self.state.probing.lines.clear();
                self.state.probing.status = format!("starting {id}");
                self.probe = Some(crate::probe::Run::start(
                    target,
                    &id,
                    self.state.probing.seconds,
                ));
            }
        });
    }

    /// What the last probe captured, drawn the way the log screen draws its lines.
    fn probe_capture(&mut self, ui: &mut egui::Ui, target: &target::Target, running: bool) {
        let matcher = LogMatch::build(&self.state.probing.filter, self.state.probing.regex);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !running && !self.state.probing.lines.is_empty(),
                    egui::Button::new("clear"),
                )
                .on_hover_text("forget the last capture")
                .clicked()
            {
                self.state.probing.lines.clear();
                self.state.probing.status.clear();
            }
            let suggested = self.probe.as_ref().map_or_else(
                || "probe.log".to_owned(),
                |run| format!("{}-{}.log", target.name, run.id),
            );
            match filter_controls(
                ui,
                &self.state.probing.lines,
                &mut self.state.probing.filter,
                &mut self.state.probing.regex,
                &matcher,
                &suggested,
            ) {
                Some(Ok(said)) => self.state.said = said,
                Some(Err(why)) => self.state.trouble = Some(why),
                None => {}
            }
        });
        // Progress or ending in words: a stopped capture and a waiting one look the same.
        if !self.state.probing.status.is_empty() {
            ui.label(&self.state.probing.status);
        }
        ui.separator();

        if self
            .state
            .probing
            .titles
            .as_ref()
            .is_some_and(Vec::is_empty)
        {
            ui.weak(format!(
                "nothing under {} looks like an installed title",
                pros_core::titles::APPMETA
            ));
            return;
        }
        if self.state.probing.lines.is_empty() {
            ui.weak(if running {
                "waiting for the title to say something"
            } else {
                "choose a title and launch it - its log is captured here, from before it starts"
            });
            return;
        }
        filtered_rows(ui, "probe", &self.state.probing.lines, &matcher, running);
    }
}
