//! The menu bar and the windows it opens: about, and registering a target.

use pros_core::target;

use super::App;

impl App {
    /// The menu bar.
    ///
    /// Every control that does not apply is disabled rather than hidden, and says why on
    /// hover: a control that vanishes reads as a bug, a greyed one reads as a state.
    pub(super) fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("file", |ui| {
                    // Porthole's player: the only command to configure is the one the stream
                    // is piped into.
                    if ui.button("configure the player...").clicked() {
                        self.write_player_example();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                // Everything about which machine lives here rather than on the main form.
                ui.menu_button("target", |ui| {
                    if ui.button("register...").clicked() {
                        // A fresh registration, not an edit of the selected one.
                        self.state.register.editing = None;
                        self.state.showing.registering = true;
                        ui.close_menu();
                    }
                    let chosen = self.state.target().cloned();
                    if ui
                        .add_enabled(chosen.is_some(), egui::Button::new("edit this target..."))
                        .on_hover_text("change this target's address - its name and ports are kept")
                        .on_disabled_hover_text("nothing is selected")
                        .clicked()
                    {
                        if let Some(target) = &chosen {
                            // Pre-filled, so a typo in the name cannot register a second target.
                            self.state.register.name.clone_from(&target.name);
                            self.state.register.address.clone_from(&target.address);
                            self.state.register.editing = Some(target.name.clone());
                            self.state.showing.registering = true;
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(chosen.is_some(), egui::Button::new("forget this target"))
                        .on_disabled_hover_text("nothing is selected")
                        .clicked()
                    {
                        if let Some(target) = chosen {
                            match target::forget(&target.name) {
                                Ok(_) => {
                                    self.state.targets = target::load().unwrap_or_default();
                                    self.state.chosen =
                                        (!self.state.targets.is_empty()).then_some(0);
                                }
                                Err(why) => self.state.trouble = Some(why.to_string()),
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("reload registrations").clicked() {
                        self.state.targets = target::load().unwrap_or_default();
                        ui.close_menu();
                    }
                });
                ui.menu_button("help", |ui| {
                    if ui.button("documentation...").clicked() {
                        self.docs.open();
                        ui.close_menu();
                    }
                    if ui.button("about...").clicked() {
                        self.state.showing.about = true;
                        ui.close_menu();
                    }
                });
            });
        });
    }

    /// What this is, and which build of it.
    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.state.showing.about;
        egui::Window::new("about prosperous")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Prosperous");
                ui.label("one instrument for talking to a prepared target");
                ui.add_space(6.0);
                // The build stamp, so a screenshot names the build it came from.
                ui.monospace(&self.stamp);
                ui.add_space(6.0);
                ui.small("a window over the crates: every decision it shows is made below it");
                ui.small("and is reachable from `pros` on the command line too");
                ui.add_space(6.0);
                ui.small("payload binaries are never shipped, only described - and nothing");
                ui.small("is sent that could not be checked first");
            });
        self.state.showing.about = open;
    }

    /// The registration dialog, in one of two modes.
    ///
    /// Registering takes a name and an address; editing changes only the address. The name is
    /// fixed when editing because re-registering under it is what replaces the entry, keeping
    /// its ports and chain.
    pub(super) fn register_dialog(&mut self, ctx: &egui::Context) {
        let editing = self.state.register.editing.clone();
        let mut open = self.state.showing.registering;
        let title = if editing.is_some() {
            "edit target"
        } else {
            "register a target"
        };
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                if let Some(name) = &editing {
                    ui.horizontal(|ui| {
                        ui.label("name");
                        ui.monospace(name);
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.label("name");
                        ui.text_edit_singleline(&mut self.state.register.name);
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("address");
                    ui.text_edit_singleline(&mut self.state.register.address);
                });
                ui.small(if editing.is_some() {
                    "the address is what moves when a target's IP changes; the name stays"
                } else {
                    "an address and a name. What it can do is asked every time,"
                });
                if editing.is_none() {
                    ui.small("because the entry point does not survive a power cycle");
                }
                ui.separator();
                // Re-registering under the same name replaces the entry, keeping its ports and
                // chain (`pros_core::target::register`).
                let name = editing
                    .clone()
                    .unwrap_or_else(|| self.state.register.name.trim().to_owned());
                let can = !name.trim().is_empty() && !self.state.register.address.trim().is_empty();
                let (label, hint) = if editing.is_some() {
                    ("save", "save the new address for this target")
                } else {
                    ("register", "remember this target under that name")
                };
                if ui
                    .add_enabled(can, egui::Button::new(label))
                    .on_hover_text(hint)
                    .on_disabled_hover_text("a name and an address are both needed")
                    .clicked()
                {
                    match target::register(name.trim(), self.state.register.address.trim()) {
                        Ok(_) => {
                            self.state.targets = target::load().unwrap_or_default();
                            // Keep the just-saved target selected rather than jumping to the first.
                            self.state.chosen = self
                                .state
                                .targets
                                .iter()
                                .position(|one| one.name == name.trim());
                            self.state.register.address.clear();
                            self.state.register.editing = None;
                            self.state.showing.registering = false;
                        }
                        Err(why) => self.state.trouble = Some(why.to_string()),
                    }
                }
            });
        // The window's own close button is the other way out, and it must win over the
        // flag the dialog itself cleared.
        if !self.state.showing.registering {
            open = false;
        }
        self.state.showing.registering = open;
        // Closing the window (its X) leaves edit mode too, so the next "register..." is fresh.
        if !self.state.showing.registering {
            self.state.register.editing = None;
        }
    }
}
