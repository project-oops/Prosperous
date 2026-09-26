//! The autoload panel: the startup list, the manager's settings, and the configurator that
//! builds a chain from a preset.

use pros_core::target;

use super::App;
use super::doctor::{Asked, doctor_action};
use super::payloads::mark_of;
use super::widgets::section_heading;
use crate::state::{Job, Section};

impl App {
    /// What the target loads at startup, and the manager's settings.
    ///
    /// The file written here decides what loads at boot; a wrong one leaves the target without
    /// its file service or loader, and recovery is re-running the entry point by hand. So an
    /// edit produces a diff, and the write happens on a second explicit press with those lines
    /// on screen.
    pub(super) fn autoload_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::Autoload);

        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        ui.horizontal(|ui| self.autoload_toolbar(ui, idle, connected));
        ui.separator();

        self.list_findings(ui, &self.state.list(), idle);
        self.boot_list(ui, connected);
        ui.add_space(10.0);
        // The settings belong to the manager only. Under an autoloader's list they would
        // change a different file from the one on screen, and `AUTOLOAD_ENABLED` under the
        // wrong list can leave the target unable to start its services.
        if self.state.list().autoloader {
            ui.weak("the manager's settings belong to its own list - choose it to see them");
        } else {
            self.settings_rows(ui);
        }
        self.export_panel(ui);
        self.pending_write(ui, idle, connected);
    }

    /// Reading the list again, choosing which list, and exporting it as a preset.
    fn autoload_toolbar(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        if ui
            .add_enabled(idle && connected, egui::Button::new("read"))
            .on_hover_text("re-read the startup list and the manager's settings")
            .on_disabled_hover_text(if connected {
                "wait for what is already running"
            } else {
                "no target selected"
            })
            .clicked()
            && let Some(target) = self.state.target().cloned()
        {
            // Not emptied first: the list stays up while it is re-read, with a notice.
            self.state.log.followed_for = None;
            self.state.begin(Job::ReadAutoload(target));
        }
        // Which list. The manager keeps one at a fixed path; the autoloader that runs
        // before it looks in several places, and its list decides whether the manager runs
        // at all. They are audited by opposite rules.
        let held = self.state.list();
        egui::ComboBox::from_id_salt("which-list")
            .selected_text(&held.label)
            .show_ui(ui, |ui| self.list_choices(ui));
        // Export: reads a working list out as a chain preset, the reverse of deploying one.
        let worth_exporting = self
            .state
            .autoload
            .boot
            .as_ref()
            .is_some_and(|boot| boot.steps.iter().any(|step| !step.is_disabled()));
        if ui
            .add_enabled(worth_exporting, egui::Button::new("export chain..."))
            .on_hover_text(
                "write this list down as a chain preset of your own, so it can be \
                     deployed to another target - or to this one after something breaks it",
            )
            .on_disabled_hover_text(if connected {
                "read a list first - there is nothing to write down"
            } else {
                "no target selected"
            })
            .clicked()
        {
            self.begin_export(&held);
        }
        ui.weak(held.path);
        if !held.editable {
            ui.colored_label(egui::Color32::from_rgb(210, 190, 120), "read only");
        }
    }

    /// The startup lists the chains declare, to choose which one is shown; choosing another
    /// reads it.
    fn list_choices(&mut self, ui: &mut egui::Ui) {
        for (at, one) in self.state.autoload.lists.clone().iter().enumerate() {
            if ui
                .selectable_label(self.state.autoload.list_at == at, &one.label)
                .on_hover_text(format!(
                    "{}
{}",
                    one.path,
                    if one.editable {
                        "editable"
                    } else {
                        "read only - a list on removable storage is \
                         the way back in when the internal one is broken"
                    }
                ))
                .clicked()
                && self.state.autoload.list_at != at
            {
                self.state.autoload.list_at = at;
                self.state.autoload.boot = None;
                self.state.autoload.pending_change = None;
                if let Some(target) = self.state.target().cloned() {
                    self.state.begin(Job::ReadList(target, one.clone()));
                }
            }
        }
    }

    /// Setting a target up from nothing: which list, what would go in it, and a warning.
    ///
    /// It replaces a whole startup list, possibly the removable one kept as the way back in, so
    /// it asks twice: the plan says what will be fetched and sent, then the resulting file goes
    /// through the whole-file review that says what the target will try to run.
    pub(super) fn configurator(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some(at) = self.state.autoload.setting_up else {
            return;
        };
        let chosen = self.state.autoload.lists.get(at).cloned();
        let Some(held) = chosen else {
            self.state.autoload.setting_up = None;
            return;
        };

        ui.add_space(8.0);
        ui.separator();
        ui.colored_label(
            egui::Color32::from_rgb(230, 90, 90),
            "SET UP FROM NOTHING - THIS REPLACES A STARTUP LIST",
        );
        ui.add_space(4.0);

        self.setup_choices(ui, at, &held);
        // What is there now, counted only for the list being shown: this panel makes no
        // requests.
        let showing = self.state.autoload.list_at == at;
        match (showing, self.state.autoload.boot.as_ref()) {
            (true, Some(boot)) if !boot.steps.is_empty() => {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 160, 90),
                    format!(
                        "{} entries are in {} now, and all of them go",
                        boot.steps.len(),
                        held.path
                    ),
                );
            }
            (true, _) => {
                ui.weak(format!("{} is empty or was not read", held.path));
            }
            (false, _) => {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 160, 90),
                    format!(
                        "whatever is in {} now will be replaced - this screen is showing a \
                         different list, so it has not been read",
                        held.path
                    ),
                );
            }
        }
        if !held.editable {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "this list is on removable storage - the one that gets you back in when the \
                 internal one is broken. Setting it up replaces exactly that.",
            );
        }

        ui.add_space(6.0);
        let mut go = false;
        let mut drop_it = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("show me what it would write"))
                .on_hover_text("plan it - nothing happens until you agree to the plan")
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                go = true;
            }
            if ui.button("cancel").clicked() {
                drop_it = true;
            }
        });

        if drop_it {
            self.state.autoload.setting_up = None;
        }
        if go {
            self.plan_a_setup(&held);
        }
    }

    /// The two questions the configurator asks before it will plan anything.
    ///
    /// Which chain, then which list: what the target runs, then where the file goes.
    fn setup_choices(&mut self, ui: &mut egui::Ui, at: usize, held: &pros_core::chain::Held) {
        let (presets, trouble) = pros_core::recovery::baseline::all();
        let mut pick = at;
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("chain:");
            egui::ComboBox::from_id_salt("which-chain")
                .selected_text(&self.state.autoload.preset)
                .show_ui(ui, |ui| {
                    for one in &presets {
                        if ui
                            .selectable_label(one.name == self.state.autoload.preset, &one.name)
                            .on_hover_text(&one.about)
                            .clicked()
                        {
                            chosen = Some(one.name.clone());
                        }
                    }
                });
            ui.label("into:");
            egui::ComboBox::from_id_salt("setting-up")
                .selected_text(&held.label)
                .show_ui(ui, |ui| {
                    for (which, one) in self.state.autoload.lists.clone().iter().enumerate() {
                        if ui
                            .selectable_label(which == at, &one.label)
                            .on_hover_text(&one.path)
                            .clicked()
                        {
                            pick = which;
                        }
                    }
                });
        });
        if let Some(name) = chosen {
            self.state.autoload.preset = name;
        }
        if pick != at {
            self.state.autoload.setting_up = Some(pick);
        }
        // An unreadable chains file is reported, not silently replaced by the shipped presets.
        if let Some(why) = trouble {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), why);
        }
        if let Some(one) = presets
            .iter()
            .find(|one| one.name == self.state.autoload.preset)
        {
            ui.weak(&one.about);
            if !one.result.is_empty() {
                ui.add_space(4.0);
                ui.label("what you end up with:");
                // Printed exactly as the chains file states it.
                ui.colored_label(egui::Color32::from_rgb(150, 190, 220), &one.result);
            }
        }
        if let Some(path) = pros_core::recovery::baseline::path() {
            ui.weak(format!("chains are read from {}", path.display()))
                .on_hover_text(
                    "a file of this shape beside the registry adds chains, or replaces one of \
                     these by using its name - read when this program starts",
                );
        }
    }

    /// Builds the configurator's plan and hands it to the panel that agrees to plans.
    fn plan_a_setup(&mut self, held: &pros_core::chain::Held) {
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        let preset = pros_core::recovery::baseline::named(&self.state.autoload.preset)
            .unwrap_or_else(pros_core::recovery::baseline::first);
        // Recorded on the registration, because later checks and fixes are judged against the
        // chain the target is meant to run. Recorded when the plan is made: it is the decision,
        // whether or not the plan is carried out.
        if let Some(target) = self.state.target().cloned() {
            match target::remember_chain(&target.name, Some(&preset.name)) {
                Ok(_) => {
                    if let Some(one) = self
                        .state
                        .targets
                        .iter_mut()
                        .find(|one| one.name == target.name)
                    {
                        one.chain = Some(preset.name.clone());
                    }
                }
                // Not fatal: the plan still holds, but the next check will not know the chain.
                Err(why) => {
                    self.state.trouble = Some(format!("the chain was not recorded: {why}"));
                }
            }
        }
        // Every list the chain has: a chain that runs the manager has the autoloader's list and
        // the manager's own, and writing one leaves the target half configured. The chosen list
        // goes where it was chosen; any other is written only when it has exactly one possible
        // path (the manager's is compiled in, an autoloader's has several candidates).
        let mut writing = vec![(held.path.clone(), kind)];
        for one in &preset.lists {
            let its_kind = if one.autoloader {
                pros_core::recovery::Kind::Autoloader
            } else {
                pros_core::recovery::Kind::Manager
            };
            if its_kind == kind || one.at.len() != 1 {
                continue;
            }
            let only = &one.at[0];
            if !writing.iter().any(|(path, _)| path == only) {
                writing.push((only.clone(), its_kind));
            }
        }

        let of = preset.clone();
        let planned = writing.clone();
        let (plan, left_out) = self.with_known(move |known| {
            let mut plans = Vec::new();
            let mut missed = Vec::new();
            for (path, kind) in &planned {
                let (one, out) = pros_core::doctor::provision(known, path, *kind, &of);
                plans.push(one);
                missed.extend(out);
            }
            missed.sort_unstable();
            missed.dedup();
            (pros_core::doctor::Plan::all_of(&plans), missed)
        });
        self.state.autoload.setting_up = None;
        // A payload with no route is left out (an entry the loader cannot find fails at every
        // boot), and named here.
        if !left_out.is_empty() {
            self.state.said = format!("left out, with no way to get them: {}", left_out.join("; "));
        }
        self.state.doctor.pending_plan = Some(crate::state::Pending {
            id: format!("set up {}", held.path),
            label: if writing.len() > 1 {
                format!(
                    "{} runs the {} chain - {} lists",
                    held.label,
                    preset.name,
                    writing.len()
                )
            } else {
                format!("{} runs the {} chain", held.label, preset.name)
            },
            plan,
        });
    }

    /// What is wrong with this list, on the screen where it is edited.
    ///
    /// The check screen audits only the manager's own list; this one audits whichever list is
    /// shown, while it is being edited. List checks only, not what is answering now.
    fn list_findings(&mut self, ui: &mut egui::Ui, held: &pros_core::chain::Held, idle: bool) {
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        // Parsed from what is on screen, so an unsaved edit is audited as it is made.
        let shown = self
            .state
            .autoload
            .boot
            .as_ref()
            .map(|boot| pros_core::chain::Chain::parse(&boot.to_text()));
        let findings = self.with_known_of(
            shown.as_ref(),
            kind,
            Some(held.path.as_str()),
            pros_core::doctor::examine_list,
        );
        if findings.is_empty() {
            return;
        }

        let mut asked: Option<crate::state::Pending> = None;
        let mut choose: Option<(String, String)> = None;
        ui.add_space(4.0);
        egui::Grid::new("list-findings")
            .striped(true)
            .num_columns(4)
            .show(ui, |ui| {
                for finding in &findings {
                    let (mark, colour) = mark_of(&finding.verdict, finding.gravity);
                    ui.colored_label(colour, mark);
                    ui.label(&finding.label);
                    ui.label(finding.verdict.describe());
                    match doctor_action(ui, finding, idle) {
                        Some(Asked::Plan(one)) => asked = Some(one),
                        Some(Asked::Chose(id, name)) => choose = Some((id, name)),
                        None => {}
                    }
                    ui.end_row();
                }
            });
        // A list with nothing wrong says so once, below the table.
        if findings.iter().all(|one| !one.verdict.is_unwell()) {
            ui.weak("nothing here says this list leaves you locked out");
        }
        ui.add_space(4.0);

        if let Some((id, name)) = choose
            && let pros_core::doctor::Remedy::Ready(plan) =
                self.with_known(|known| pros_core::doctor::plan_for(known, &name))
        {
            asked = Some(crate::state::Pending {
                id,
                label: format!("{name} is in the startup list"),
                plan,
            });
        }
        if let Some(one) = asked {
            self.state.doctor.pending_plan = Some(one);
        }
    }

    /// The manager's settings, under the list they belong with.
    fn settings_rows(&mut self, ui: &mut egui::Ui) {
        let Some(settings) = self.state.autoload.settings.clone() else {
            return;
        };
        // Drawn from the pending edit when there is one, so a second click undoes the first.
        let pending = self
            .state
            .autoload
            .pending_change
            .clone()
            .filter(|change| change.into == pros_core::autoload::CONFIG);
        let shown = pending.as_ref().map_or_else(
            || settings.clone(),
            |change| pros_core::autoload::Settings::parse(&change.now),
        );
        ui.strong("settings");
        let mut change = None;
        let mut undo = false;
        egui::Grid::new("settings")
            .striped(true)
            .num_columns(2)
            .show(ui, |ui| {
                for (key, value) in shown.all() {
                    // A one-or-zero setting gets a switch; anything else is shown read-only,
                    // so no value of unknown shape is rewritten.
                    if value == "0" || value == "1" {
                        let mut on = value == "1";
                        if ui.checkbox(&mut on, "").changed() {
                            let wanted = if on { "1" } else { "0" };
                            // Applied to what is pending and diffed against the target's copy,
                            // so setting a value back clears the edit.
                            let next = shown.set(key, wanted).map(|edit| edit.now);
                            match next {
                                Some(now) if now.trim() == settings.text().trim() => undo = true,
                                Some(now) => {
                                    change = Some(pros_core::autoload::Change {
                                        was: settings.text().to_owned(),
                                        now,
                                        what: format!("{key} = {wanted}"),
                                        into: pros_core::autoload::CONFIG.to_owned(),
                                    });
                                }
                                None => {}
                            }
                        }
                        let name = if settings.get(key) == Some(value.as_str()) {
                            egui::RichText::new(key)
                        } else {
                            // Changed and not written.
                            egui::RichText::new(key).color(egui::Color32::from_rgb(210, 190, 120))
                        };
                        ui.label(name);
                    } else {
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.label(key);
                            ui.weak(value);
                        });
                    }
                    ui.end_row();
                }
            });
        if undo {
            // Back to what the target has, so there is nothing to write and nothing to review.
            self.state.autoload.pending_change = None;
        } else if let Some(pending) = change {
            self.state.autoload.pending_change = Some(pending);
        }
    }

    /// A change waiting to be written, shown line by line.
    fn pending_write(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        let Some(change) = self.state.autoload.pending_change.clone() else {
            return;
        };
        // A change to a read-only list is dropped rather than offered for writing.
        if change.into == pros_core::chain::PATH && !self.state.list().editable {
            self.state.autoload.pending_change = None;
            return;
        }
        ui.add_space(8.0);
        ui.separator();
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            format!("not written yet: {}", change.what),
        );
        ui.small("every change is marked in the list above, in the position it has now");
        // The change itself is marked in the table above; this shows the whole file as sent.
        ui.collapsing("the file as it will be written", |ui| {
            for line in change.now.lines() {
                ui.monospace(line);
            }
        });
        let (grave, more) = self.write_hazards(ui, &change);
        self.write_buttons(ui, &change, grave, more, idle, connected);
    }

    /// What is wrong with the text about to be written, and what would answer it.
    ///
    /// Audited on what is about to be written, not on what is there.
    ///
    /// Returns whether anything found is grave, and the edits that would put it right.
    fn write_hazards(
        &self,
        ui: &mut egui::Ui,
        change: &pros_core::autoload::Change,
    ) -> (bool, Vec<pros_core::recovery::Fix>) {
        let after = pros_core::chain::Chain::parse(&change.now);
        let hazards = pros_core::recovery::audit(
            &after,
            &self.catalogue,
            self.state.payloads.there.as_deref().unwrap_or_default(),
            pros_core::recovery::Kind::Manager,
            &self.chain_of_target(),
            self.loader_is_up(),
        );
        let grave = pros_core::recovery::is_dangerous(&hazards);
        if grave {
            ui.add_space(6.0);
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "WRITING THIS MAY LEAVE THE TARGET UNREACHABLE AT ITS NEXT RESTART",
            );
            for hazard in hazards
                .iter()
                .filter(|one| one.gravity() == pros_core::recovery::Gravity::Critical)
            {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), hazard.describe());
                ui.weak(hazard.remedy());
            }
        }
        // Repairs are offered alongside the warning, so writing anyway is not the only action.
        let repairs: Vec<pros_core::recovery::Fix> = hazards
            .iter()
            .filter_map(pros_core::recovery::Hazard::fix)
            .collect();
        (grave, if grave { repairs } else { Vec::new() })
    }

    /// The buttons under a pending write, and what was pressed.
    fn write_buttons(
        &mut self,
        ui: &mut egui::Ui,
        change: &pros_core::autoload::Change,
        grave: bool,
        mut more: Vec<pros_core::recovery::Fix>,
        idle: bool,
        connected: bool,
    ) {
        let mut fix_first: Vec<pros_core::recovery::Fix> = Vec::new();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // The write button is named differently when the write is dangerous.
            if !more.is_empty()
                && ui
                    .button(format!("fix these {} first", more.len()))
                    .on_hover_text(
                        "make the edits that answer the findings above, and show the \
                         result here for review - still nothing written",
                    )
                    .clicked()
            {
                fix_first = std::mem::take(&mut more);
            }
            let label = if grave { "write it anyway" } else { "write it" };
            if ui
                .add_enabled(idle && connected, egui::Button::new(label))
                .on_hover_text("send this file to the target, replacing what is there")
                .on_disabled_hover_text("no target selected")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state.begin(Job::WriteAutoload(
                    target,
                    change.into.clone(),
                    change.now.clone(),
                ));
                self.state.autoload.pending_change = None;
            }
            if ui.button("discard").clicked() {
                self.state.autoload.pending_change = None;
                // Re-read, so the screen shows the target's settings rather than the discarded
                // edit.
                if let Some(target) = self.state.target().cloned()
                    && idle
                {
                    self.state.begin(Job::ReadAutoload(target));
                }
            }
        });
        // After the panel: applying borrows the state it was drawn from.
        if !fix_first.is_empty() {
            self.apply_fixes(&fix_first);
        }
    }
}
