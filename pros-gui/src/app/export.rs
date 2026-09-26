//! Writing a chain read off a target down as a preset.

use super::App;
use crate::state::Job;

impl App {
    /// Builds the preset from what was read, so the panel has something to show.
    ///
    /// A step holds the line as written (`kstuff-lite_v1.09.elf`, `#` in front when off); a
    /// preset entry is the bare name. The chain parser does that translation, and is the one
    /// answer to whether two lines name the same payload.
    pub(super) fn begin_export(&mut self, held: &pros_core::chain::Held) {
        let Some(boot) = self.state.autoload.boot.as_ref() else {
            return;
        };
        let disabled = boot.steps.iter().filter(|step| step.is_disabled()).count();
        let lines = boot
            .steps
            .iter()
            .map(|step| step.payload.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let entries = pros_core::chain::Chain::parse(&lines).order().to_vec();

        let target = self
            .state
            .target()
            .map_or_else(|| "a target".to_owned(), |one| one.name.clone());
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        let taken_from = format!("{} on {target}, at {}", held.label, held.path);
        let (preset, notes) =
            pros_core::recovery::baseline::from_list("", &taken_from, &entries, kind);

        self.state.autoload.exporting = Some(crate::state::Exporting {
            name: format!(
                "{target}-{}",
                if held.autoloader {
                    "autoloader"
                } else {
                    "manager"
                }
            ),
            preset,
            notes,
            // The companion files are a round trip, so the panel opens now and they fill in
            // when the read below returns. Nothing is written while this is set.
            capturing: true,
            disabled,
            into: pros_core::recovery::baseline::path().map_or_else(
                || "nowhere on this machine".to_owned(),
                |at| at.display().to_string(),
            ),
            taken: pros_core::recovery::baseline::all()
                .0
                .into_iter()
                .map(|one| one.name)
                .collect(),
        });
        // Reads the declared companion files off the target so the chain carries them.
        if let Some(target) = self.state.target().cloned() {
            self.state.queue(Job::CaptureConfig(target));
        } else if let Some(export) = self.state.autoload.exporting.as_mut() {
            export.capturing = false;
        }
    }

    /// The warning under the name field: what is wrong with the typed name, or what it replaces.
    fn export_name_notice(
        ui: &mut egui::Ui,
        name: &str,
        is_shipped: bool,
        usable: bool,
        already_taken: bool,
    ) {
        if name.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(230, 160, 90), "it needs a name");
        } else if is_shipped {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                format!(
                    "'{name}' is a built-in chain provided by Prosperous and cannot be overwritten. \
                     Choose a custom name for your chain."
                ),
            );
        } else if !usable {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "a preset name is one word - a target's registry line is whitespace-delimited, \
                 so half of this would be read as an address",
            );
        } else if already_taken {
            ui.colored_label(
                egui::Color32::from_rgb(230, 160, 90),
                format!("there is already a custom preset called {name}, and this replaces it."),
            );
        }
    }

    /// The files the chain carries beside its list: settings files read off the target and put
    /// back verbatim on deploy, shown by path and size before anything is written.
    fn export_files_shown(ui: &mut egui::Ui, export: &crate::state::Exporting) {
        ui.add_space(4.0);
        if export.capturing {
            ui.weak("    reading the files this chain carries...");
        } else if export.preset.files.is_empty() {
            ui.weak("    no settings files carried - just the payload order");
        } else {
            ui.label("and it carries these files, put back as they are on deploy:");
            for file in &export.preset.files {
                ui.weak(format!(
                    "    {} - {} ({} bytes)",
                    file.label,
                    file.path,
                    file.content.len()
                ));
            }
        }
    }

    /// What would be written down, where, and what it could not know.
    ///
    /// It asks before writing because a preset replaces by name, so the typed name decides
    /// whether an existing preset is replaced.
    pub(super) fn export_panel(&mut self, ui: &mut egui::Ui) {
        let Some(export) = self.state.autoload.exporting.as_mut() else {
            return;
        };

        ui.add_space(8.0);
        ui.separator();
        ui.strong("WRITE THIS LIST DOWN AS A CHAIN PRESET");
        ui.weak(&export.preset.about);
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("call it:");
            ui.text_edit_singleline(&mut export.name);
        });
        let name = export.name.trim().to_owned();
        let is_shipped = pros_core::recovery::baseline::is_shipped_name(&name);
        // One word: the registry line is whitespace-delimited, so a name with a space would be
        // written as `chain=<half>` and the rest read as an address.
        let usable = !name.is_empty() && !name.contains(char::is_whitespace) && !is_shipped;
        Self::export_name_notice(ui, &name, is_shipped, usable, export.taken.contains(&name));

        Self::export_entries_shown(ui, export);
        Self::export_files_shown(ui, export);

        if !export.notes.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(210, 190, 120),
                "what this could not know, and did not invent:",
            );
            for note in &export.notes {
                ui.weak(format!("    {note}"));
            }
        }

        ui.add_space(6.0);
        // Not while the files are still being read, or the chain would carry the list without
        // its files.
        let ready = usable && !export.capturing;
        let (write_it, drop_it) = Self::export_buttons(ui, ready, export.capturing);

        if drop_it {
            self.state.autoload.exporting = None;
            return;
        }
        if !write_it {
            return;
        }
        let mut preset = export.preset.clone();
        preset.name = name;
        self.write_export(&preset);
    }

    /// Where the preset would go, and its entries in the order they will be written.
    fn export_entries_shown(ui: &mut egui::Ui, export: &crate::state::Exporting) {
        ui.weak(format!("into {}", export.into));
        ui.add_space(4.0);
        ui.label(format!(
            "{} entries, in this order:",
            export.preset.entries.len()
        ));
        // In preset order, not read order: for a manager's list they can differ, and the
        // preset order is what will be written.
        let mut shown = export.preset.entries.clone();
        shown.sort_by_key(|one| one.rank(pros_core::recovery::Kind::Manager));
        for entry in &shown {
            ui.weak(format!("    {}", entry.name));
        }
        if export.disabled > 0 {
            ui.weak(format!(
                "{} disabled {} left out - a line the manager will not resolve is not part of \
                 what this target loads",
                export.disabled,
                if export.disabled == 1 {
                    "line"
                } else {
                    "lines"
                }
            ));
        }
    }

    /// Write and cancel; answers which was pressed.
    fn export_buttons(ui: &mut egui::Ui, ready: bool, capturing: bool) -> (bool, bool) {
        let mut write_it = false;
        let mut drop_it = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new("write it"))
                .on_disabled_hover_text(if capturing {
                    "still reading the files this chain carries"
                } else {
                    "give it a one-word name first"
                })
                .clicked()
            {
                write_it = true;
            }
            if ui.button("cancel").clicked() {
                drop_it = true;
            }
        });
        (write_it, drop_it)
    }

    /// Writes the preset down, closing the panel only when it was written.
    fn write_export(&mut self, preset: &pros_core::recovery::baseline::Preset) {
        // Straight to the filesystem: one small local file, and the queue is for target work.
        self.state.said = match pros_core::recovery::baseline::keep(preset) {
            Ok(at) => {
                self.state.autoload.exporting = None;
                format!(
                    "{} written to {} - it is offered as a chain from the next start",
                    preset.name,
                    at.display()
                )
            }
            // Kept open on failure: the panel holds the only copy of what was read.
            Err(why) => format!("not written: {why}"),
        };
    }
}
