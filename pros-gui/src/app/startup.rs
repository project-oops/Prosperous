//! The startup list on the autoload panel: its table, what each entry is and why it is
//! there, and the controls that reorder it.

use pros_core::boot::Step as BootStep;
use pros_core::payloads::There;

use super::widgets::{headings, reason, role_of};
use super::{App, PAYLOADS};

/// One change to the startup list, held until the caller can apply it.
///
/// A boxed closure rather than an enum of the operations, so each is named once, as the call
/// it makes.
type Edit = Box<dyn FnOnce(&mut pros_core::boot::Boot) -> bool>;

/// The payload at a position in the list as it was before an edit.
fn boot_name(boot: Option<&pros_core::boot::Boot>, at: usize) -> Option<&String> {
    boot?.steps.get(at).map(|step| &step.payload)
}

impl App {
    /// The startup list, in order, with the controls that change it.
    ///
    /// One row is selected, not several: moving a step moves one step, and moving several has
    /// an order nobody stated.
    pub(super) fn boot_list(&mut self, ui: &mut egui::Ui, connected: bool) {
        let held = self.state.list();
        let Some(boot) = self.state.autoload.boot.clone() else {
            ui.weak(if connected {
                "not read yet"
            } else {
                "select a target, and this reads its startup list"
            });
            return;
        };

        let at = self.state.autoload.boot_at;
        let act = self.boot_controls(ui, &boot, at);
        ui.add_space(4.0);

        let explain = self.explain(&boot, &held);
        // Only a change to this file: a settings edit is also a pending change, and belongs to
        // the settings panel.
        let pending = self
            .state
            .autoload
            .pending_change
            .clone()
            .filter(|change| change.into == pros_core::chain::PATH);
        let picked = boot_table(ui, &boot, at, pending.as_ref(), &explain);
        if boot.steps.is_empty() {
            ui.weak("the startup list is empty");
        }
        ui.small(
            "an entry is removed rather than commented out: nothing here knows whether the \
             manager accepts comments, and a line it does not understand may stop the chain",
        );

        if let Some(index) = picked {
            self.state.autoload.boot_at = Some(index);
        }
        if let Some(act) = act {
            self.apply_boot_edit(boot, at, act);
        }
    }

    /// What the startup list's rows say about each entry, gathered once for the table.
    fn explain(&self, boot: &pros_core::boot::Boot, held: &pros_core::chain::Held) -> Explain {
        let known = self.catalogue.clone();
        let there = self.state.payloads.there.clone();
        // The audit of the list as it would be, so a proposed change can give its reason.
        let hazards = pros_core::recovery::audit(
            &pros_core::chain::Chain::parse(&boot.to_text()),
            &known,
            there.as_deref().unwrap_or_default(),
            // The rules invert: the loader is required in an autoloader list and
            // impossible in the manager's own.
            if held.autoloader {
                pros_core::recovery::Kind::Autoloader
            } else {
                pros_core::recovery::Kind::Manager
            },
            &self.chain_of_target(),
            self.loader_is_up(),
        );
        Explain {
            known,
            described: self.manifest.clone(),
            there,
            hazards,
        }
    }

    /// Applies an edit asked for by the controls, keeping the selection on the row it was on.
    fn apply_boot_edit(&mut self, boot: pros_core::boot::Boot, at: Option<usize>, act: Edit) {
        let mut edited = boot;
        if act(&mut edited) {
            // The selection follows the row, not the position.
            if let Some(was_at) = at {
                self.state.autoload.boot_at = edited.steps.iter().position(|step| {
                    Some(&step.payload) == boot_name(self.state.autoload.boot.as_ref(), was_at)
                });
            }
            self.state.autoload.pending_change = edited.change();
            self.state.autoload.boot = Some(edited);
        }
    }

    /// The controls that reorder the startup list, and what one of them was asked to do.
    ///
    /// Returned rather than applied, because applying borrows the list these were drawn from.
    fn boot_controls(
        &mut self,
        ui: &mut egui::Ui,
        boot: &pros_core::boot::Boot,
        at: Option<usize>,
    ) -> Option<Edit> {
        let mut act: Option<Edit> = None;
        // Nothing is offered for a list this will not write, so no edit is made and lost.
        let editable = self.state.list().editable;
        ui.horizontal_wrapped(|ui| {
            act = step_buttons(ui, boot, at, editable);
            ui.separator();
            if let Some(add) = self.add_payload(ui, editable) {
                act = Some(add);
            }
        });
        act
    }

    /// The payloads on the target, to add one to the end of the list; answers the one picked.
    fn add_payload(&self, ui: &mut egui::Ui, editable: bool) -> Option<Edit> {
        let mut act: Option<Edit> = None;
        // Only what is on the target, found inside the manager's folders (it keeps
        // `payloads/<name>/<name>_<version>.elf`). Not scanned yet and found nothing are
        // drawn differently.
        let scanned = self.state.payloads.there.clone();
        let there = scanned.clone().unwrap_or_default();
        let known = !there.is_empty();
        egui::ComboBox::from_id_salt("add-payload")
            .selected_text(match &scanned {
                None => "add... (looking)".to_owned(),
                Some(found) if found.is_empty() => "add... (none found)".to_owned(),
                Some(found) => format!("add... ({} on the target)", found.len()),
            })
            .show_ui(ui, |ui| {
                match &scanned {
                    None => {
                        ui.weak("the target has not been asked yet - this fills in on connect");
                    }
                    Some(found) if found.is_empty() => {
                        ui.weak(format!("no .elf files under {PAYLOADS}"));
                    }
                    Some(_) => {}
                }
                // The bare name, not the path: the manager resolves filenames. A payload it
                // can never resolve (on removable storage outside its folder) is tagged and
                // cannot be picked (`pros_core::payloads::Where`).
                for one in there {
                    // Adding needs somewhere to save it, and a read-only list has none.
                    let usable = editable && one.storage.can_autoload();
                    let label = format!("{}   [{}]", one.name, one.storage.tag());
                    if ui
                        .add_enabled(usable, egui::SelectableLabel::new(false, label))
                        .on_hover_text(format!("{}\n{}", one.path, one.storage.means()))
                        .on_disabled_hover_text(one.storage.means())
                        .clicked()
                    {
                        act = Some(Box::new(move |boot| boot.add(&one.name)));
                    }
                }
            });
        ui.weak(if known {
            "adds to the end - reorder it from there"
        } else {
            "only what is on the target can be started at boot"
        });
        act
    }
}

/// Whether a startup-list entry names a payload, however either spells it.
fn named(name: &str, against: &str) -> bool {
    pros_core::chain::Chain::parse(name)
        .position(against)
        .is_some()
}

/// What the startup list's rows say about each entry: what it is, why it is there, and where
/// its file is.
struct Explain {
    /// Which services exist and what each is for.
    known: pros_core::catalogue::Catalogue,
    /// The payload list, when one has been read.
    described: Option<pros_core::manifest::Manifest>,
    /// Every payload file on the target, once looked for.
    there: Option<Vec<There>>,
    /// The audit of the list as it would be.
    hazards: Vec<pros_core::recovery::Hazard>,
}

impl Explain {
    /// What a payload is, from its publisher, most specific first: the sidecar the manager
    /// wrote at install, the payload list, then the catalogue. Why it is at this point in the
    /// chain is a separate column ([`Self::why`]).
    fn what(&self, name: &str) -> Option<String> {
        self.there
            .as_ref()
            .and_then(|there| there.iter().find(|one| one.name == name))
            .and_then(|one| {
                one.about
                    .as_ref()
                    .and_then(|about| about.description.clone())
            })
            .filter(|text| !text.trim().is_empty())
            .or_else(|| {
                self.described
                    .as_ref()?
                    .payloads()
                    .iter()
                    .find(|payload| named(name, &payload.name))
                    .and_then(|payload| payload.description.clone())
                    .filter(|text| !text.trim().is_empty())
            })
            .or_else(|| {
                self.known
                    .services()
                    .iter()
                    .find(|service| named(name, &service.name))
                    .map(|service| service.unlocks.to_string())
            })
    }

    /// Why an entry is here, most specific first: a recorded note, the audit finding that
    /// proposed the change, the tracked recommendation, then the role derived from the
    /// catalogue's flags.
    fn why(&self, name: &str) -> Option<String> {
        let known = &self.known;
        known
            .note(name)
            .map(str::to_owned)
            .or_else(|| {
                known
                    .services()
                    .iter()
                    .find(|service| named(name, &service.name))
                    .and_then(|service| known.note(&service.name).map(str::to_owned))
            })
            .or_else(|| {
                // The finding that asked for this entry to go in or come out.
                self.hazards.iter().find_map(|hazard| match hazard.fix() {
                    Some(
                        pros_core::recovery::Fix::Add(who) | pros_core::recovery::Fix::Remove(who),
                    ) if named(name, &who) => Some(hazard.describe()),
                    _ => None,
                })
            })
            .or_else(|| {
                // The tracked recommendation, a fact about the payloads.
                pros_core::recovery::baseline::about(name).map(|placed| placed.why)
            })
            .or_else(|| {
                known
                    .services()
                    .iter()
                    .find(|service| named(name, &service.name))
                    .and_then(role_of)
            })
            .filter(|text| !text.trim().is_empty())
    }

    /// Where an entry's file is on the target, once looked for.
    fn placed(&self, name: &str) -> Option<pros_core::payloads::Where> {
        self.there.as_ref().and_then(|there: &Vec<There>| {
            there
                .iter()
                .find(|one| one.name == name)
                .map(|one| one.storage)
        })
    }
}

/// The startup list as a table, with any pending change marked in place; answers which row
/// was clicked.
fn boot_table(
    ui: &mut egui::Ui,
    boot: &pros_core::boot::Boot,
    at: Option<usize>,
    pending: Option<&pros_core::autoload::Change>,
    explain: &Explain,
) -> Option<usize> {
    let mut picked = None;
    egui::Grid::new("boot")
        .striped(true)
        .num_columns(7)
        .show(ui, |ui| {
            headings(
                ui,
                &[
                    "order",
                    "change",
                    "payload",
                    "storage",
                    "before it",
                    "what",
                    "why",
                ],
            );
            // The list as it stands, with the change marked in place. A removed entry keeps
            // its row and number: it is still in the list on the target.
            let rows = pending.map_or_else(
                || {
                    boot.steps
                        .iter()
                        .enumerate()
                        .map(|(at, step)| pros_core::autoload::Shown {
                            was_at: Some(at),
                            now_at: Some(at),
                            payload: step.payload.clone(),
                        })
                        .collect()
                },
                pros_core::autoload::Change::shown,
            );
            for row in &rows {
                if let Some(index) = boot_row(ui, boot, row, at, explain) {
                    picked = Some(index);
                }
            }
        });
    picked
}

/// One row of the startup list; answers its position when it was clicked.
fn boot_row(
    ui: &mut egui::Ui,
    boot: &pros_core::boot::Boot,
    row: &pros_core::autoload::Shown,
    at: Option<usize>,
    explain: &Explain,
) -> Option<usize> {
    let mut picked = None;
    // Selection works on the pending list, so a removed entry cannot be picked.
    let index = row.now_at;
    let chosen = index.is_some() && at == index;
    if ui.selectable_label(chosen, order_of(row)).clicked()
        && let Some(index) = index
    {
        picked = Some(index);
    }
    change_cell(ui, row);
    // A removed entry has no step: it is not in the pending list.
    let step = index.and_then(|index| boot.steps.get(index));
    let Some(step) = step else {
        ui.colored_label(
            egui::Color32::from_rgb(220, 120, 120),
            egui::RichText::new(&row.payload).strikethrough(),
        );
        ui.weak("");
        ui.weak("");
        ui.weak(explain.what(&row.payload).unwrap_or_default());
        reason(ui, explain.why(&row.payload));
        ui.end_row();
        return picked;
    };
    let index = index.unwrap_or_default();
    if step_cells(ui, step, chosen, explain) {
        picked = Some(index);
    }
    picked
}

/// A row's position, and where a pending change moves it, coloured by what the change does.
fn order_of(row: &pros_core::autoload::Shown) -> egui::RichText {
    let order = match (row.was_at, row.now_at) {
        (Some(was), Some(now)) if was != now => format!("{was} -> {now}"),
        (Some(was), _) => format!("{was}"),
        (None, Some(now)) => format!("-> {now}"),
        (None, None) => String::new(),
    };
    if row.moved() || row.added() {
        egui::RichText::new(order).color(egui::Color32::from_rgb(120, 200, 140))
    } else if row.removed() {
        egui::RichText::new(order).color(egui::Color32::from_rgb(220, 120, 120))
    } else {
        egui::RichText::new(order)
    }
}

/// What a pending change does to a row, if anything.
fn change_cell(ui: &mut egui::Ui, row: &pros_core::autoload::Shown) {
    if row.removed() {
        ui.colored_label(egui::Color32::from_rgb(220, 120, 120), "removed")
            .on_hover_text("still on the target - saving takes it out");
    } else if row.added() {
        ui.colored_label(egui::Color32::from_rgb(120, 200, 140), "added")
            .on_hover_text("not on the target yet - saving puts it in");
    } else if row.moved() {
        ui.colored_label(egui::Color32::from_rgb(120, 200, 140), "moved")
            .on_hover_text(
                "the manager loads the list in order, so this would run at a \
                 different point",
            );
    } else {
        ui.weak("");
    }
}

/// The cells of an entry still in the list, from its name on; answers whether it was clicked.
fn step_cells(
    ui: &mut egui::Ui,
    step: &pros_core::boot::Step,
    chosen: bool,
    explain: &Explain,
) -> bool {
    // Three states: a set nobody has read is not an empty one. A disabled entry
    // is off on purpose, never missing.
    let missing = (!step.is_disabled())
        .then(|| {
            explain
                .there
                .as_ref()
                .map(|there| !there.iter().any(|one| one.name == step.name()))
        })
        .flatten();
    let label = if step.is_disabled() {
        // In the list, and will not load.
        egui::RichText::new(step.name())
            .strikethrough()
            .color(egui::Color32::GRAY)
    } else if missing == Some(true) {
        egui::RichText::new(step.name()).color(egui::Color32::from_rgb(220, 120, 120))
    } else {
        egui::RichText::new(step.name())
    };
    let clicked = ui.selectable_label(chosen, label).clicked();
    // Where its file is, which decides whether the manager can resolve it.
    match explain.placed(step.name()) {
        Some(storage) if storage.can_autoload() => {
            ui.weak(storage.tag()).on_hover_text(storage.means());
        }
        Some(storage) => {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), storage.tag())
                .on_hover_text(storage.means());
        }
        None => {
            ui.weak("");
        }
    }
    // The instruction that precedes it, shown because reordering carries it.
    if step.is_disabled() {
        ui.weak("off");
    } else {
        match missing {
            Some(true) => {
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), "not on the target")
                    .on_hover_text(
                        "the manager loads this by name and will not find it - the \
                     chain stops here at the next restart",
                    );
            }
            _ => {
                ui.weak(step.before.as_deref().unwrap_or(""));
            }
        }
    }
    ui.weak(explain.what(step.name()).unwrap_or_default());
    reason(ui, explain.why(step.name()));
    ui.end_row();
    clicked
}

/// Up, down, disable and remove, for the selected row; answers the one pressed.
fn step_buttons(
    ui: &mut egui::Ui,
    boot: &pros_core::boot::Boot,
    at: Option<usize>,
    editable: bool,
) -> Option<Edit> {
    let last = boot.steps.len().saturating_sub(1);
    let mut act: Option<Edit> = None;
    let picked = at.is_some() && editable;
    if ui
        .add_enabled(picked && at != Some(0), egui::Button::new("up"))
        .on_hover_text("load this one earlier")
        .on_disabled_hover_text(if picked {
            "already first"
        } else {
            "select a row"
        })
        .clicked()
        && let Some(at) = at
    {
        act = Some(Box::new(move |boot| boot.earlier(at)));
    }
    if ui
        .add_enabled(picked && at != Some(last), egui::Button::new("down"))
        .on_hover_text("load this one later")
        .on_disabled_hover_text(if picked {
            "already last"
        } else {
            "select a row"
        })
        .clicked()
        && let Some(at) = at
    {
        act = Some(Box::new(move |boot| boot.later(at)));
    }
    let step = at.and_then(|at| boot.steps.get(at));
    let off = step.is_some_and(BootStep::is_disabled);
    if ui
        .add_enabled(
            picked,
            egui::Button::new(if off { "enable" } else { "disable" }),
        )
        .on_hover_text(
            "keep it in the list and stop it loading - the manager logs the \
             name it cannot find and carries on",
        )
        .on_disabled_hover_text("select a row")
        .clicked()
        && let Some(at) = at
    {
        act = Some(Box::new(move |boot| boot.disable(at, !off)));
    }
    if ui
        .add_enabled(picked, egui::Button::new("remove"))
        .on_hover_text(
            "take it out of the startup list - the file stays on the target, and \
             nothing is written until you say so",
        )
        .on_disabled_hover_text("select a row")
        .clicked()
        && let Some(at) = at
    {
        act = Some(Box::new(move |boot| boot.remove(at)));
    }
    act
}
