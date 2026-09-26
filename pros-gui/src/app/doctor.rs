//! The doctor's findings, the plan answering one, and carrying that plan out.

use pros_core::payloads::There;

use super::payloads::mark_of;
use super::widgets::headings;
use super::{App, PAYLOADS};
use crate::state::{Job, Section};

/// What pressing something on one of the doctor's rows asked for.
pub(super) enum Asked {
    /// Show this plan, for somebody to agree to or not.
    Plan(crate::state::Pending),
    /// One of several routes was picked, so build that one's plan.
    Chose(String, String),
}

/// What one row offers, given what its check found.
///
/// The offer differs per verdict.
pub(super) fn doctor_action(
    ui: &mut egui::Ui,
    finding: &pros_core::doctor::Finding,
    idle: bool,
) -> Option<Asked> {
    use pros_core::doctor::{Remedy, Verdict};

    let mut picked: Option<String> = None;
    match &finding.verdict {
        Verdict::Unwell {
            remedy: Remedy::Ready(plan),
            ..
        } if !plan.is_settled() => {
            if ui
                .add_enabled(idle, egui::Button::new("fix...").min_size(FIX))
                .on_hover_text("show every step that would take - nothing happens yet")
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                return Some(Asked::Plan(crate::state::Pending {
                    id: finding.id.clone(),
                    label: finding.label.clone(),
                    plan: plan.clone(),
                }));
            }
        }
        // Several would do, so this asks: how a target boots is the user's choice.
        Verdict::Unwell {
            remedy: Remedy::Choose { between, why },
            ..
        } => {
            ui.menu_button("choose...", |ui| {
                ui.weak(why.as_str());
                ui.separator();
                for (name, unlocks) in between {
                    if ui
                        .button(name.as_str())
                        .on_hover_text(unlocks.as_str())
                        .clicked()
                    {
                        picked = Some(name.clone());
                        ui.close_menu();
                    }
                }
            });
        }
        Verdict::Unwell {
            remedy: Remedy::Beyond(said),
            ..
        } => {
            ui.weak("nothing from here").on_hover_text(said.as_str());
        }
        // Every step is already done and the check still fails, so there is nothing to
        // offer.
        Verdict::Unwell {
            remedy: Remedy::Ready(_),
            ..
        } => {
            ui.weak("already done").on_hover_text(
                "every step of this remedy is already the case and the check still \
                 fails, so this is not what is wrong",
            );
        }
        Verdict::Well(_) | Verdict::Unknown(_) | Verdict::Aside(_) => {
            ui.label("");
        }
    }
    picked.map(|name| Asked::Chose(finding.id.clone(), name))
}

/// What to say after a plan has been handed to the queue.
///
/// Counts what was started, not what worked: nothing has finished yet at this point, and the
/// result is checked and reported separately at the end.
fn summarise(queued: usize, edited: usize, could_not: &[String]) -> String {
    let mut said = match (queued, edited) {
        (0, 0) => "nothing to do".to_owned(),
        (0, _) => format!("{edited} list edits ready - review and save them"),
        (_, 0) => format!("{queued} steps started"),
        // The edits wait on the transfers, so they are not called ready.
        _ => format!(
            "{queued} steps started - the {edited} list edits follow once those land, and open              for review"
        ),
    };
    if !could_not.is_empty() {
        said.push_str(" - not done: ");
        said.push_str(&could_not.join(", "));
    }
    said
}

/// What carrying out one step of a plan came to, for the summary.
enum Carried {
    /// A job was queued.
    Queued,
    /// A list edit was held back until the transfers land.
    Edited,
    /// Nothing new: the edit was already held.
    Nothing,
    /// It could not be done, and why.
    CouldNot(String),
}

/// The target's health in a few words, and the colour it is drawn in.
fn health_word(light: pros_core::doctor::Health) -> (&'static str, egui::Color32) {
    use pros_core::doctor::Health;
    match light {
        Health::Well => ("all well", egui::Color32::from_rgb(120, 190, 120)),
        Health::Unknown => ("not measured", egui::Color32::GRAY),
        Health::Warning => (
            "something is missing",
            egui::Color32::from_rgb(210, 190, 120),
        ),
        Health::Unwell => (
            "THIS TARGET MAY NOT COME BACK AFTER A RESTART",
            egui::Color32::from_rgb(230, 90, 90),
        ),
    }
}

/// Fix all: one combined plan, through the same confirmation as a single fix; answers it
/// when asked for.
fn fix_all_button(
    ui: &mut egui::Ui,
    findings: &[pros_core::doctor::Finding],
    idle: bool,
) -> Option<crate::state::Pending> {
    use pros_core::doctor::{Remedy, Verdict};
    let together: Vec<pros_core::doctor::Plan> = findings
        .iter()
        .filter_map(|finding| match &finding.verdict {
            Verdict::Unwell {
                remedy: Remedy::Ready(plan),
                ..
            } if !plan.is_settled() => Some(plan.clone()),
            _ => None,
        })
        .collect();
    if together.len() <= 1 {
        return None;
    }
    let plan = pros_core::doctor::Plan::all_of(&together);
    let steps = plan.outstanding().len();
    ui.add_enabled(
        idle,
        egui::Button::new(format!("fix all {}", together.len())),
    )
    .on_hover_text(format!(
        "show what answering all of them takes - {steps} steps, and nothing                          happens yet"
    ))
    .on_disabled_hover_text("wait for what is already running")
    .clicked()
    .then(|| crate::state::Pending {
        id: "everything".to_owned(),
        label: format!("all {} of these are answered", together.len()),
        plan,
    })
}

/// Every finding with its one action; answers a plan asked for, or a payload chosen.
fn findings_table(
    ui: &mut egui::Ui,
    findings: &[pros_core::doctor::Finding],
    idle: bool,
) -> (Option<crate::state::Pending>, Option<(String, String)>) {
    let mut asked = None;
    let mut choose = None;
    egui::Grid::new("doctor")
        .striped(true)
        .num_columns(4)
        .show(ui, |ui| {
            headings(ui, &["", "check", "what was found", ""]);
            for finding in findings {
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
    (asked, choose)
}

/// One width for the check screen's fix buttons, so the notes beside them line up.
const FIX: egui::Vec2 = egui::vec2(74.0, 0.0);

impl App {
    /// Every check, worst first, each with the one action that answers it.
    ///
    /// Each action is a whole plan (fetch, send, list), not one step of it.
    pub(super) fn doctor_panel(&mut self, ui: &mut egui::Ui, idle: bool) {
        use pros_core::doctor::{Remedy, health};

        let findings = self.with_known(pros_core::doctor::examine);
        self.verify_if_due(&findings, idle);

        let (word, colour) = health_word(health(&findings));
        let mut asked: Option<crate::state::Pending> = None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.colored_label(colour, word);
            asked = fix_all_button(ui, &findings, idle);
        });

        let (from_table, choose) = findings_table(ui, &findings, idle);
        if from_table.is_some() {
            asked = from_table;
        }

        // After the grid: both of these borrow what it was drawn from.
        if let Some((id, name)) = choose
            && let Remedy::Ready(plan) =
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

    /// The plan, in full, and the only place in this program where one is agreed to.
    ///
    /// A plan reaches the job queue only through this button, after every step is drawn out,
    /// including the steps already done.
    pub(super) fn plan_panel(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        let Some(pending) = self.state.doctor.pending_plan.clone() else {
            return;
        };
        ui.add_space(8.0);
        ui.separator();
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            format!("to make sure {}:", pending.label),
        );
        ui.weak(&pending.plan.because);
        ui.add_space(4.0);

        for (at, one) in pending.plan.moves.iter().enumerate() {
            let step = format!("{}. {}", at + 1, one.step.describe());
            if one.already {
                ui.weak(format!("{step}  - already done, so it is skipped"));
            } else {
                ui.label(step);
            }
        }

        if pending.plan.rewrites_the_list() {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(210, 190, 120),
                "the startup list is not written here: the edit is prepared and opened for \
                 review on the startup list screen, where the whole file is shown and saved",
            );
        }

        ui.add_space(6.0);
        let outstanding = pending.plan.outstanding().len();
        let mut go = false;
        let mut drop_it = false;
        ui.horizontal(|ui| {
            let can = idle && (connected || !pending.plan.touches_the_target());
            if ui
                .add_enabled(can, egui::Button::new(format!("do these {outstanding}")))
                .on_hover_text("carry out the steps above, in order")
                .on_disabled_hover_text(if connected {
                    "wait for what is already running"
                } else {
                    "no target selected"
                })
                .clicked()
            {
                go = true;
            }
            if ui.button("not now").clicked() {
                drop_it = true;
            }
        });

        if drop_it {
            self.state.doctor.pending_plan = None;
        }
        if go {
            self.carry_out(&pending);
        }
    }

    /// Turns an agreed plan into queued jobs, in order.
    ///
    /// List edits are applied but not written: the startup list screen is the one place a list
    /// is written, with the whole file shown first. The transfers put the files the edits name
    /// in place.
    fn carry_out(&mut self, pending: &crate::state::Pending) {
        let Some(target) = self.state.target().cloned() else {
            self.state.trouble = Some("no target selected".to_owned());
            return;
        };
        let mut queued = 0_usize;
        let mut edited = 0_usize;
        let mut could_not: Vec<String> = Vec::new();

        for one in &pending.plan.moves {
            if one.already {
                continue;
            }
            match self.carry_step(&target, &one.step) {
                Carried::Queued => queued += 1,
                Carried::Edited => edited += 1,
                Carried::Nothing => {}
                Carried::CouldNot(why) => could_not.push(why),
            }
        }

        self.state.doctor.pending_plan = None;
        // With deferred list edits, the payloads are re-listed first: adding an entry needs
        // the payload on internal storage, judged by a listing taken after the sends.
        if queued > 0 && !self.state.doctor.after_transfers.is_empty() {
            self.state
                .queue(Job::FindPayloads(target.clone(), PAYLOADS.to_owned()));
        }
        if queued > 0 && self.state.doctor.after_transfers.is_empty() {
            // Re-checked only when no list edit is pending: until the list is saved the
            // finding would still fail.
            self.state.queue(Job::Check(target));
            self.state.doctor.fixing = Some(pending.id.clone());
        }
        self.state.said = summarise(queued, edited, &could_not);
        // Nothing was queued, so nothing is going to land and prompt this later.
        if queued == 0 {
            self.finish_deferred_edits();
        }
    }

    /// Queues one step of an agreed plan, or holds it back with the other list work.
    fn carry_step(
        &mut self,
        target: &pros_core::target::Target,
        step: &pros_core::doctor::Step,
    ) -> Carried {
        use pros_core::doctor::Step;

        match step {
            Step::Fetch { payload } => {
                if let Some(described) = self.described_as(payload) {
                    self.state.queue(Job::Fetch(Box::new(described), None));
                    Carried::Queued
                } else {
                    Carried::CouldNot(format!("{payload} has no description to fetch from"))
                }
            }
            Step::Bring { payload, from } => {
                if let Some(mut into) = pros_core::manifest::staging() {
                    into.push(from.rsplit('/').next().unwrap_or(payload.as_str()));
                    self.state
                        .queue(Job::Pull(target.clone(), from.clone(), into));
                    Carried::Queued
                } else {
                    Carried::CouldNot("there is nowhere on this machine to stage it".to_owned())
                }
            }
            // `Job::Install`, never `Job::Send`: `Send` runs the ELF in memory and writes
            // nothing to the disk.
            Step::Send { payload, to } => self.send_step(target, payload, to),
            Step::Run { path } => {
                self.state
                    .queue(Job::RunThere(target.clone(), path.clone()));
                Carried::Queued
            }
            // Held back with the other list work: its entries name files the sends put in
            // place.
            Step::Rebuild { into, entries } => {
                // Each list once, though several findings can name the same file.
                if self
                    .state
                    .doctor
                    .rebuild
                    .iter()
                    .any(|(kept, _)| kept == into)
                {
                    Carried::Nothing
                } else {
                    self.state
                        .doctor
                        .rebuild
                        .push((into.clone(), entries.clone()));
                    Carried::Edited
                }
            }
            // Held back until the files are where the list will say they are
            // (`DoctorState::after_transfers`).
            Step::List(fix) => {
                self.state.doctor.after_transfers.push(fix.clone());
                Carried::Edited
            }
            // Turns autoload on so the deployed list is read. Queued like a transfer: it
            // writes the settings file, not the list, and is a no-op when already on.
            Step::Enable { into: _ } => {
                self.state.queue(Job::EnableAutoload(target.clone()));
                Carried::Queued
            }
            // Puts a file the chain carries back, verbatim. Queued like a transfer: it
            // writes beside the list, not the list.
            Step::Place { into, content } => {
                self.state.queue(Job::PlaceFile(
                    target.clone(),
                    into.clone(),
                    content.clone(),
                ));
                Carried::Queued
            }
        }
    }

    /// Queues installing a described payload to where the plan says it goes.
    fn send_step(
        &mut self,
        target: &pros_core::target::Target,
        payload: &str,
        to: &str,
    ) -> Carried {
        match self.described_as(payload) {
            Some(described) => match pros_core::staging::path_for(&described) {
                Some(path) => {
                    self.state.queue(Job::Install(
                        target.clone(),
                        Box::new(described),
                        path,
                        to.to_owned(),
                    ));
                    Carried::Queued
                }
                None => Carried::CouldNot(format!("{payload} has no filename to stage under")),
            },
            None => Carried::CouldNot(format!(
                "{payload} is not described, so it cannot be laid out"
            )),
        }
    }

    /// Makes the list edits a plan agreed, now that its transfers have landed.
    ///
    /// Only when the queue is clear and nothing failed: an entry for a file that never arrived
    /// fails at every boot.
    pub(super) fn finish_deferred_edits(&mut self) {
        if self.state.doctor.after_transfers.is_empty() && self.state.doctor.rebuild.is_empty() {
            return;
        }
        if !self.state.is_idle() || self.state.queued() > 0 {
            return;
        }
        let waiting = std::mem::take(&mut self.state.doctor.after_transfers);
        if let Some(why) = self.state.trouble.clone() {
            let dropped = waiting.len() + self.state.doctor.rebuild.len();
            self.state.doctor.rebuild.clear();
            self.state.trouble = Some(format!(
                "{why}\nso the startup list was left alone - {dropped} edits were not made"
            ));
            return;
        }
        // One file at a time, in plan order: the review panel shows one whole file, so the
        // next waits until this one is saved.
        if !self.state.doctor.rebuild.is_empty() {
            let (into, entries) = self.state.doctor.rebuild.remove(0);
            let left = self.state.doctor.rebuild.len();
            self.prepare_rebuild(&into, &entries);
            if left > 0 {
                self.state.said = format!(
                    "{} - {left} more list{} to review after this one is saved",
                    self.state.said,
                    if left == 1 { "" } else { "s" }
                );
            }
            return;
        }
        self.apply_fixes(&waiting);
    }

    /// Puts a whole new startup list up for the same review every write goes through.
    ///
    /// Prepared, never written: the review panel writes it once it has been read.
    fn prepare_rebuild(&mut self, into: &str, entries: &[String]) {
        let text = entries
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let now = if text.is_empty() {
            text
        } else {
            format!("{text}\n")
        };
        let boot = pros_core::boot::Boot::parse(&now);
        // The screen follows the list being written, so the file reviewed is the file saved.
        if let Some(at) = self
            .state
            .autoload
            .lists
            .iter()
            .position(|one| one.path == into)
        {
            self.state.autoload.list_at = at;
        }
        self.state.autoload.boot = Some(boot);
        self.state.autoload.boot_at = None;
        self.state.autoload.pending_change = Some(pros_core::autoload::Change {
            // What it replaces, so the review draws a diff showing which lines go.
            was: self
                .state
                .autoload
                .boot
                .as_ref()
                .map(pros_core::boot::Boot::to_text)
                .unwrap_or_default(),
            now: now.clone(),
            what: format!("set {into} up from nothing - {} entries", entries.len()),
            into: into.to_owned(),
        });
        self.state.section = Section::Autoload;
        self.state.said = format!(
            "{} entries ready for {into} - read the whole file below, then save it",
            entries.len()
        );
    }

    /// Records a description that now points at a newer release.
    ///
    /// The payload list is a user-owned file, so the status line names the old and new
    /// versions.
    pub(super) fn take_relisted(
        &mut self,
        now: pros_core::manifest::Payload,
        found: pros_core::sources::Upstream,
    ) {
        // The fresh answer replaces the sweep's, so the column stops offering this update.
        self.sources.put(&now.name, found);
        let _ = pros_core::sources::save(&self.sources);
        let Some(manifest) = self.manifest.as_mut() else {
            return;
        };
        let was = manifest
            .find(&now.name)
            .and_then(|one| one.version.clone())
            .unwrap_or_else(|| "-".to_owned());
        let to = now.version.clone().unwrap_or_else(|| "-".to_owned());
        let name = now.name.clone();
        manifest.absorb(now);
        match manifest.save() {
            Ok(path) => {
                self.state.said = format!(
                    "{name} in the list is now {to}, was {was} - digest taken from what \
                     downloaded, written to {}",
                    path.display()
                );
            }
            // Right in memory and wrong on disk: the next launch would revert it silently.
            Err(why) => {
                self.state.trouble = Some(format!(
                    "{name} was updated here but not written down: {why}"
                ));
            }
        }
    }

    /// Says whether a fix that has been carried out actually answered its finding.
    ///
    /// A plan ends by checking the target again, and this reads that answer: jobs succeeding
    /// does not mean the finding is answered.
    fn verify_if_due(&mut self, findings: &[pros_core::doctor::Finding], idle: bool) {
        if !idle || self.state.queued() > 0 {
            return;
        }
        let Some(id) = self.state.doctor.fixing.take() else {
            return;
        };
        let still = findings
            .iter()
            .find(|one| one.id == id && one.verdict.is_unwell());
        self.state.said = match still {
            None => format!("{id}: fixed, and checked"),
            Some(one) => format!(
                "{id}: the steps ran and the check still fails - {}",
                one.verdict.describe()
            ),
        };
    }

    /// Makes every edit the survival audit asked for, and shows the result for review.
    ///
    /// It edits and opens the autoload screen rather than writing, so the save goes through
    /// the whole-file review.
    ///
    /// The audit names services (`shsrv`) and a startup list names files (`shsrv_v0.20.elf`);
    /// the target's payload scan joins them.
    pub(super) fn apply_fixes(&mut self, fixes: &[pros_core::recovery::Fix]) {
        use pros_core::recovery::Fix;

        let Some(mut boot) = self.state.autoload.boot.clone() else {
            return;
        };
        let there = self.state.payloads.there.clone().unwrap_or_default();
        let mut done = 0_usize;
        let mut could_not: Vec<String> = Vec::new();

        for fix in fixes {
            match fix {
                Fix::Remove(service) => {
                    // Found the way the check finds it, so what is removed is what was reported.
                    let at = pros_core::chain::Chain::parse(&boot.to_text()).position(service);
                    if let Some(at) = at
                        && boot.remove(at)
                    {
                        done += 1;
                    }
                }
                Fix::Add(service) => {
                    let named = |one: &&There| {
                        pros_core::chain::Chain::parse(&one.name)
                            .position(service)
                            .is_some()
                    };
                    // Internal storage only: the manager lists a payload on removable storage
                    // outside its folder but can never resolve it.
                    match there
                        .iter()
                        .filter(named)
                        .find(|one| one.storage == pros_core::payloads::Where::Internal)
                    {
                        Some(one) if boot.add(&one.name) => done += 1,
                        Some(_) => {}
                        None => {
                            // Where else it is, if anywhere, so the message gives a next step.
                            let elsewhere = there.iter().find(named);
                            could_not.push(match elsewhere {
                                Some(one) => format!(
                                    "{service} (only at {}, which the manager cannot autoload \
                                     - copy it to {} first)",
                                    one.path,
                                    pros_core::payloads::INTERNAL
                                ),
                                None => format!("{service} (not on the target at all)"),
                            });
                        }
                    }
                }
            }
        }

        // The pending change is what brings up the review and the save button.
        self.state.autoload.pending_change = boot.change();
        self.state.autoload.boot = Some(boot);
        self.state.section = Section::Autoload;
        self.state.said = if could_not.is_empty() {
            format!("{done} edits made - review the list and save")
        } else {
            format!(
                "{done} edits made - review and save. Not done: {} is not on the target, so \
                 send it first",
                could_not.join(", ")
            )
        };
    }
}
