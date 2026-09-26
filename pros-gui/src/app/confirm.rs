//! The confirmations the storage sections ask for before acting: deleting, installing,
//! running a file nothing describes, and a copy that was refused.

use std::path::{Path, PathBuf};

use super::App;
use crate::state::Job;

impl App {
    /// What a delete would remove, before it removes it.
    ///
    /// Lists every selected entry and names the side, because a selection made across a fold
    /// or left over from a changed listing is how the wrong thing gets deleted.
    pub(super) fn pending_delete(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some((offer, what)) = self.state.files.pending_delete.clone() else {
            return;
        };
        let side = if offer == crate::listing::Offer::DeleteHere {
            self.state.files.local_path.clone()
        } else {
            self.state.files.library_path.clone()
        };

        ui.add_space(6.0);
        ui.colored_label(
            egui::Color32::from_rgb(220, 120, 120),
            format!("delete {} from {side}?", what.len()),
        );
        egui::ScrollArea::vertical()
            .id_salt("to-delete")
            .max_height(120.0)
            .show(ui, |ui| {
                for entry in &what {
                    ui.monospace(&entry.name);
                }
            });
        // A folder takes everything under it, so folders are called out.
        let folders = what
            .iter()
            .filter(|entry| offer == crate::listing::Offer::DeleteThere && entry.folder_there())
            .count();
        if folders > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(220, 120, 120),
                format!(
                    "{folders} of these {} a folder - everything inside goes too",
                    if folders == 1 { "is" } else { "are" }
                ),
            );
        }
        ui.small("nothing here undoes this");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("delete"))
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                self.state.files.pending_delete = None;
                let names: Vec<String> = what.iter().map(|entry| entry.name.clone()).collect();
                for name in &names {
                    self.state.files.listing.chosen.remove(name);
                }
                if offer == crate::listing::Offer::DeleteHere {
                    let root = PathBuf::from(self.state.files.local_path.trim());
                    let paths = names.iter().map(|name| root.join(name)).collect();
                    self.state.begin(Job::DeleteHere(paths));
                } else if let Some(target) = self.state.target().cloned() {
                    let root = self
                        .state
                        .files
                        .library_path
                        .trim_end_matches('/')
                        .to_owned();
                    let paths = what
                        .iter()
                        .map(|entry| (format!("{root}/{}", entry.name), entry.folder_there()))
                        .collect();
                    self.state.begin(Job::DeleteThere(target, paths));
                }
            }
            if ui.button("cancel").clicked() {
                self.state.files.pending_delete = None;
            }
        });
        ui.separator();
    }

    /// A file somebody dropped that nothing describes, and what can be done with it.
    ///
    /// Not refused, because the build-run-read loop of homebrew development should not need a
    /// manifest entry per build. An undescribed file can be run, which leaves nothing behind,
    /// or kept, which does and says so.
    pub(super) fn adhoc(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.state.files.adhoc.clone() else {
            return;
        };
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());

        // Read once, here, so the shape can be shown before anything is offered.
        let bytes = std::fs::read(&path);
        let shape = bytes
            .as_ref()
            .map(|bytes| pros_link::shape::identify(bytes));

        Self::adhoc_shape(ui, &path, &shape);

        let runnable = shape.as_ref().is_ok_and(|shape| shape.is_payload());
        if self.adhoc_buttons(ui, &path, &name, runnable) {
            self.state.files.adhoc = None;
        }
    }

    /// Which file, what shape it is, and why no digest is checked.
    fn adhoc_shape(
        ui: &mut egui::Ui,
        path: &Path,
        shape: &Result<pros_link::shape::Shape, &std::io::Error>,
    ) {
        ui.add_space(6.0);
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            "nothing describes this file",
        );
        ui.monospace(path.display().to_string());
        match shape {
            Ok(shape) if shape.is_payload() => {
                ui.small("it looks like a payload the loader will take");
            }
            Ok(shape) => {
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), shape.describe());
                ui.small(shape.remedy());
            }
            Err(why) => {
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), why.to_string());
            }
        }
        ui.small(
            "no digest is checked: a digest proves a download is what a publisher claimed, and \
             a file you built here makes no such claim",
        );
    }

    /// Run it, keep it, or cancel; answers whether the file is done with.
    fn adhoc_buttons(
        &mut self,
        ui: &mut egui::Ui,
        path: &Path,
        name: &str,
        runnable: bool,
    ) -> bool {
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        let mut clear = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    runnable && idle && connected,
                    egui::Button::new("run it now"),
                )
                .on_hover_text("send it to the loader - in memory until the next restart")
                .on_disabled_hover_text(if !runnable {
                    "the loader will not take this"
                } else if connected {
                    "wait for what is already running"
                } else {
                    "no target selected"
                })
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state
                    .begin(Job::Send(target, name.to_owned(), path.to_path_buf()));
                clear = true;
            }
            if ui
                .add_enabled(idle, egui::Button::new("keep in payloads"))
                .on_hover_text(
                    "copy it into this machine's payload folder, so it stays in the list",
                )
                .clicked()
                && let Some(into) = pros_core::manifest::staging()
            {
                match std::fs::create_dir_all(&into)
                    .and_then(|()| std::fs::copy(path, into.join(name)))
                {
                    Ok(_) => {
                        self.state.said = format!("{name} kept in {}", into.display());
                        self.read_local();
                    }
                    Err(why) => self.state.trouble = Some(why.to_string()),
                }
                clear = true;
            }
            if ui.button("cancel").clicked() {
                clear = true;
            }
        });
        ui.separator();
        clear
    }

    /// A package waiting to be installed, and the confirm in front of it.
    ///
    /// An install, unlike a copy, has the target unpack and register the package, and nothing
    /// here undoes it. The confirm names each file, and the target's answer is reported in its
    /// own words.
    pub(super) fn pending_install(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some(paths) = self.state.files.pending_install.clone() else {
            return;
        };
        if paths.is_empty() {
            self.state.files.pending_install = None;
            return;
        }
        ui.add_space(6.0);
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            if paths.len() == 1 {
                "install this on the target?".to_owned()
            } else {
                format!(
                    "install these {} on the target, one after another?",
                    paths.len()
                )
            },
        );
        for path in &paths {
            ui.monospace(path.display().to_string());
        }
        ui.small("held out from this machine for the target to fetch, then registered by it");
        ui.small("nothing here undoes that");
        ui.small(
            "this project has never watched an install succeed, so whatever the target says \
             afterwards is shown as it said it",
        );
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("install"))
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state.files.pending_install = None;
                for path in &paths {
                    self.state
                        .queue(Job::InstallPackage(target.clone(), path.clone()));
                }
            }
            if ui.button("cancel").clicked() {
                self.state.files.pending_install = None;
            }
        });
        ui.separator();
    }

    /// A copy that was not attempted, why, and the one way past it.
    ///
    /// A panel rather than a greyed button: whether a copy is refused depends on its
    /// destination, known only when it is asked for. The override is offered but is never the
    /// default.
    pub(super) fn refusal(&mut self, ui: &mut egui::Ui) {
        if let Some(refusal) = self.state.files.guard_refusal.clone() {
            self.guard_refused(ui, &refusal);
        }
        if let Some(needs) = self.state.files.refused.clone() {
            self.origin_refused(ui, &needs);
        }
    }

    /// A title transfer refused for where it was going, with the path it should go to.
    fn guard_refused(&mut self, ui: &mut egui::Ui, refusal: &pros_core::guard::Refusal) {
        let amber = egui::Color32::from_rgb(210, 190, 120);
        ui.colored_label(
            amber,
            "not copied: the destination is a system path the console silently ignores",
        );
        ui.small(format!("source      {}", refusal.from.display()));
        ui.small(format!("target      {}", refusal.target_path));
        ui.small(format!("issue       {}", refusal.explanation));
        ui.small(format!("remedy      {}", refusal.remedy));
        ui.horizontal(|ui| {
            let suggested = refusal.suggested_path.clone();
            if ui
                .button(format!("Use '{suggested}' instead"))
                .on_hover_text("copy to the canonical homebrew directory scanned by the console")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state
                    .files
                    .library_path
                    .clone_from(&refusal.suggested_path);
                self.state.files.guard_refusal = None;
                self.state.begin(Job::Restore(
                    target,
                    refusal.from.clone(),
                    refusal.suggested_path.clone(),
                    false,
                ));
            }
            if ui
                .button("copy anyway")
                .on_hover_text("send it regardless - having read the above")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state.files.guard_refusal = None;
                self.state.begin(Job::Restore(
                    target,
                    refusal.from.clone(),
                    refusal.target_path.clone(),
                    true,
                ));
            }
            if ui.button("leave it").clicked() {
                self.state.files.guard_refusal = None;
            }
        });
        ui.separator();
    }

    /// A save copy refused because whose it is says it would not load where it is going.
    fn origin_refused(&mut self, ui: &mut egui::Ui, needs: &pros_core::origin::Needs) {
        let amber = egui::Color32::from_rgb(210, 190, 120);
        match needs {
            pros_core::origin::Needs::Resigning { wrote, going_to } => {
                ui.colored_label(amber, "not copied: this save belongs to another account");
                ui.small(format!("written by  {wrote}"));
                ui.small(format!("going to    {going_to}"));
                ui.small(
                    "saves are signed for the account that wrote them, so this one needs \
                     decrypting and re-signing first - garlic-savemgr does that",
                );
            }
            pros_core::origin::Needs::Unknown(why) => {
                ui.colored_label(amber, "not copied: whose save this is could not be checked");
                ui.small(why);
                ui.small(
                    "copying it anyway may leave files the target refuses, which looks like \
                     a save that simply will not load",
                );
            }
            pros_core::origin::Needs::Nothing => {}
        }
        ui.horizontal(|ui| {
            if ui
                .button("copy anyway")
                .on_hover_text("send it regardless - having read the above")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                let from = PathBuf::from(self.state.files.local_path.trim());
                let to = self.state.files.library_path.clone();
                self.state.files.refused = None;
                self.state.begin(Job::Restore(target, from, to, true));
            }
            if ui.button("leave it").clicked() {
                self.state.files.refused = None;
            }
        });
        ui.separator();
    }
}
