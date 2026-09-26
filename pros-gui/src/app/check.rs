//! The check panel, and what the last check says a target has.

use pros_core::check::Verdict;
use pros_core::manifest::Manifest;
use pros_core::payloads::Boot;

use super::App;
use super::widgets::{section_heading, section_heading_with};
use crate::state::{Job, Section};

impl App {
    /// What the target can do now.
    pub(super) fn check_panel(&mut self, ui: &mut egui::Ui) {
        if self.state.target().is_none() {
            section_heading(ui, Section::Check);
            ui.label("no target selected");
            ui.small("target -> register..., or pick one from the list above");
            return;
        }
        section_heading_with(ui, Section::Check, |ui| {
            if ui
                .add_enabled(self.state.is_idle(), egui::Button::new("check again"))
                .on_hover_text("ask the target what it can do right now")
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                let started = self.state.begin(Job::Check(target));
                debug_assert!(started, "a job started while the button was disabled");
            }
            // Beside the check, because a working chain is the answer to what it finds.
            // Single-entry edits stay on the autoload screen.
            if ui
                .add_enabled(self.state.is_idle(), egui::Button::new("deploy chain..."))
                .on_hover_text(
                    "set this target up from nothing: pick a chain and where it goes, read \
                     what you would end up with, then agree to it. Nothing happens before \
                     you do.",
                )
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                self.state.autoload.setting_up = Some(self.state.autoload.list_at);
            }
        });

        // Before the services table: whether the target survives its next restart matters
        // more than what answers now. Drawn before the report exists too, because nothing
        // measured is itself a finding.
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        // The configurator goes above the findings, since it answers most of them.
        self.configurator(ui, idle);
        self.doctor_panel(ui, idle);
        self.plan_panel(ui, idle, connected);
        ui.add_space(8.0);
        ui.separator();

        let Some(report) = &self.state.report else {
            ui.label("not asked yet - a target's capabilities are not remembered between");
            ui.label("runs, because the entry point does not survive a power cycle");
            return;
        };

        egui::Grid::new("services").striped(true).show(ui, |ui| {
            for finding in &report.findings {
                let (mark, colour) = if finding.reachability.open {
                    ("up", egui::Color32::from_rgb(120, 190, 120))
                } else if finding.service.required {
                    ("DOWN", egui::Color32::from_rgb(220, 120, 120))
                } else {
                    ("--", egui::Color32::GRAY)
                };
                ui.colored_label(colour, mark);
                ui.label(finding.service.name.as_ref());
                ui.label(format!(":{}", finding.service.port));
                // What the service provides, since a port number is not a capability.
                ui.label(finding.service.unlocks.as_ref());
                ui.label(if finding.was_slow() {
                    format!("{}ms", finding.reachability.took.as_millis())
                } else {
                    String::new()
                });
                ui.end_row();
            }
        });

        ui.add_space(8.0);
        let verdict = report.verdict();
        let colour = match verdict {
            Verdict::Ready => egui::Color32::from_rgb(120, 190, 120),
            Verdict::Dimmed { .. } => egui::Color32::from_rgb(210, 190, 120),
            Verdict::Blocked { .. } => egui::Color32::from_rgb(220, 120, 120),
        };
        ui.colored_label(colour, verdict.to_string());
    }

    /// Everything the doctor is allowed to look at, borrowed from what is already known.
    ///
    /// One place builds it, so a chosen plan is built against the same picture that offered
    /// the options.
    pub(super) fn with_known<T>(&self, act: impl FnOnce(&pros_core::doctor::Known<'_>) -> T) -> T {
        // The check screen audits the manager's own list, which the check reads beside its
        // probe.
        self.with_known_of(
            self.state.chain.as_ref(),
            pros_core::recovery::Kind::Manager,
            Some(pros_core::chain::PATH),
            act,
        )
    }

    /// Whether the loader is answering, as far as the last check knows.
    ///
    /// `None` means not asked, which the audit treats as not answering: listing the loader is
    /// offered unless a copy is known to hold the port.
    pub(super) fn loader_is_up(&self) -> Option<bool> {
        let report = self.state.report.as_ref()?;
        let loader = report.about(pros_link::service::LOADER.name.as_ref())?;
        Some(loader.reachability.open)
    }

    /// Which chain this target is meant to be running.
    ///
    /// The registration's answer when it has one (a target set up with etaHEN is not missing
    /// an FTP server); otherwise the first shipped chain. Decided here only, not defaulted in
    /// several places.
    pub(super) fn chain_of_target(&self) -> pros_core::recovery::baseline::Preset {
        self.state
            .target()
            .and_then(|target| target.chain.as_deref())
            .and_then(pros_core::recovery::baseline::named)
            .unwrap_or_else(pros_core::recovery::baseline::first)
    }

    /// The same, about a named list rather than the one the check happened to read.
    pub(super) fn with_known_of<T>(
        &self,
        chain: Option<&pros_core::chain::Chain>,
        kind: pros_core::recovery::Kind,
        list: Option<&str>,
        act: impl FnOnce(&pros_core::doctor::Known<'_>) -> T,
    ) -> T {
        let preset = self.chain_of_target();
        let nothing = Manifest::default();
        let described = self.manifest.as_ref().unwrap_or(&nothing);
        let staged: Vec<String> = described
            .payloads()
            .iter()
            .filter(|one| pros_core::staging::is_staged(one))
            .map(|one| one.name.clone())
            .collect();
        act(&pros_core::doctor::Known {
            report: self.state.report.as_ref(),
            // Passed as it is: not listed is not the same as none.
            there: self.state.payloads.there.as_deref(),
            staged: &staged,
            described,
            chain,
            kind,
            // An autoloader resolves entries against its own list's directory, so a plan has
            // to know which list it is for.
            list,
            preset: &preset,
            known: &self.catalogue,
        })
    }

    /// A payload's description, matched the way everything else matches names.
    pub(super) fn described_as(&self, service: &str) -> Option<pros_core::manifest::Payload> {
        self.manifest
            .as_ref()?
            .payloads()
            .iter()
            .find(|one| {
                pros_core::chain::Chain::parse(&one.name)
                    .position(service)
                    .is_some()
            })
            .cloned()
    }

    /// Whether a service this section needs is answering, explaining in the panel if not.
    ///
    /// Answers `true` when the work can go ahead. Otherwise it draws which service, what it
    /// provides, whether it is staged here and whether a reboot brings it back, and offers the
    /// action that would change it.
    pub(super) fn needs(&mut self, ui: &mut egui::Ui, service: &str) -> bool {
        let Some(report) = &self.state.report else {
            self.unasked(ui, service);
            return false;
        };
        let Some(finding) = report.about(service) else {
            // An unknown service is not reported on.
            return true;
        };
        if finding.reachability.open {
            return true;
        }

        let unlocks = &finding.service.unlocks;
        let boot = self.state.chain.as_ref().map_or(Boot::Unknown, |chain| {
            chain.position(service).map_or(Boot::NotInList, Boot::At)
        });
        let staged = self
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.find(service))
            .filter(|payload| pros_core::staging::is_staged(payload))
            .and_then(pros_core::staging::path_for);

        section_heading(ui, self.state.section);
        ui.colored_label(
            egui::Color32::from_rgb(220, 120, 120),
            format!("{service} is not answering"),
        );
        ui.label(format!("this section needs it to {unlocks}"));
        ui.add_space(4.0);
        match boot {
            Boot::At(at) => {
                ui.small(format!(
                    "in the boot list at {at}, so a reboot brings it back"
                ));
            }
            Boot::NotInList => {
                ui.small("not in the boot list, so a reboot will not bring it back");
            }
            Boot::Unknown => {
                ui.small(
                    "the boot list has not been read, so what a reboot brings back is unknown",
                );
            }
        }
        ui.add_space(8.0);

        let mut send = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    staged.is_some() && self.state.is_idle(),
                    egui::Button::new("run it"),
                )
                .on_hover_text("load it now, through the loader")
                .on_disabled_hover_text("not staged here - the payloads section can fetch it")
                .clicked()
            {
                send.clone_from(&staged);
            }
            if ui.button("go to payloads").clicked() {
                self.state.section = Section::Payloads;
            }
        });
        if let Some(path) = send
            && let Some(target) = self.state.target().cloned()
        {
            self.state
                .begin(Job::Send(target, service.to_owned(), path));
        }
        false
    }

    /// What to say when nothing has asked the target yet.
    fn unasked(&mut self, ui: &mut egui::Ui, service: &str) {
        section_heading(ui, self.state.section);
        if self.state.target().is_none() {
            ui.label("no target selected");
            ui.small("target -> register..., or pick one from the list above");
            return;
        }
        // The check starts on its own when a target is selected, so it is usually running.
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("asking the target about {service}"));
        });
        ui.small("what a target can do is not remembered between runs, because the entry point");
        ui.small("does not survive a power cycle - so it is asked every time");
    }
}
