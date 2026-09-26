//! The controllers panel: pad slots, their key bindings, and the feed to the target.

use super::App;
use super::widgets::section_heading;
use crate::state::Section;

impl App {
    /// Reads the keyboard and sends a pad record, every frame.
    ///
    /// Ticks from `update` unconditionally, not from the panel that draws pads, so input keeps
    /// flowing while another section (the stream above all) is shown.
    pub(super) fn drive_pads(&mut self, ctx: &egui::Context) {
        // Level or edge: `keys_down` covers a hold, and the press events catch a press and
        // release inside one frame, which `keys_down` alone drops. The same technique as
        // orbistoun's window (ACKNOWLEDGEMENTS).
        let held: Vec<String> = ctx.input(|input| {
            let mut names: Vec<String> = input
                .keys_down
                .iter()
                .map(|key| key.name().to_owned())
                .collect();
            for event in &input.events {
                if let egui::Event::Key {
                    key, pressed: true, ..
                } = event
                {
                    let name = key.name().to_owned();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
            names
        });

        let asking = self.state.controllers.binding.take();
        if let Some(button) = asking {
            // The first key pressed while waiting takes the binding; Escape abandons it.
            if let Some(name) = held.first() {
                if name != "Escape"
                    && let Some(number) = self.state.controllers.binding_slot
                    && let Some(slot) = self
                        .state
                        .controllers
                        .pads
                        .slots
                        .iter_mut()
                        .find(|slot| slot.number() == number)
                {
                    slot.keys.bind(name, button);
                }
                self.state.controllers.binding_slot = None;
            } else {
                self.state.controllers.binding = Some(button);
            }
        }

        let down = |name: &str| held.iter().any(|key| key == name);
        let records = self.state.controllers.pads.poll(&down);
        self.state.controllers.pad_records = self
            .state
            .controllers
            .pad_records
            .saturating_add(records.len() as u64);
        // A feed that is not open counts these as dropped, which separates a broken
        // connection from a broken mapping.
        self.state.controllers.feed.send(&records);
    }

    /// Controllers presented to the target from this machine.
    ///
    /// The keyboard drives up to four slots, each sending under its own number. A slot set to
    /// a physical controller says nothing can read it: the workspace forbids unsafe code, so
    /// the platform APIs would need a dependency.
    pub(super) fn controllers_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::Controllers);

        self.feed_bar(ui);
        ui.add_space(8.0);

        self.pad_conflicts(ui);
        self.pad_slots(ui);
        ui.add_space(10.0);
        self.pad_keys(ui);
    }

    /// Where records are going, and the one control that changes it.
    ///
    /// Not connected, sending and connection ended are drawn differently, because each needs
    /// different action.
    fn feed_bar(&mut self, ui: &mut egui::Ui) {
        let connected = self.state.target().is_some();
        let sending = self.state.controllers.feed.status.is_sending();

        ui.horizontal(|ui| {
            if sending {
                if ui.button("stop").clicked() {
                    self.state.controllers.feed.close();
                }
            } else if ui
                .add_enabled(connected, egui::Button::new("connect"))
                .on_disabled_hover_text("select a target first")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                let port = self
                    .state
                    .controllers
                    .feed_port
                    .trim()
                    .parse()
                    .unwrap_or(pros_link::feed::PORT);
                // The error is kept in the feed's status, which the line below draws.
                let _ = self.state.controllers.feed.open(&target.address, port);
            }
            ui.small("port:");
            ui.add(
                egui::TextEdit::singleline(&mut self.state.controllers.feed_port)
                    .desired_width(60.0),
            );

            let colour = match &self.state.controllers.feed.status {
                pros_link::feed::Status::Sending => egui::Color32::from_rgb(120, 200, 140),
                pros_link::feed::Status::Idle => egui::Color32::GRAY,
                pros_link::feed::Status::Lost(_) | pros_link::feed::Status::Refused(_) => {
                    egui::Color32::from_rgb(220, 120, 120)
                }
            };
            ui.colored_label(colour, self.state.controllers.feed.status.describe());
        });

        if sending {
            ui.small(format!("{} records sent", self.state.controllers.feed.sent));
        } else {
            ui.small("no payload accepts these yet - see docs/VIDEO.md, under Porthole");
            if self.state.controllers.feed.dropped > 0 {
                ui.small(format!(
                    "{} records had nowhere to go",
                    self.state.controllers.feed.dropped
                ));
            }
        }
    }

    /// Every key doing two jobs, named.
    ///
    /// Shown rather than resolved: silently unbinding one would leave a dead button nobody was
    /// told about.
    fn pad_conflicts(&mut self, ui: &mut egui::Ui) {
        let found = self.state.controllers.pads.conflicts();
        if found.is_empty() {
            return;
        }
        for one in &found {
            ui.colored_label(egui::Color32::from_rgb(220, 120, 120), one.describe());
        }
        ui.add_space(8.0);
    }

    /// One row per slot: what drives it, and what it is doing.
    fn pad_slots(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!(
            "slots  ({} filled)",
            self.state.controllers.pads.filled()
        ));

        let mut binding = None;
        egui::Grid::new("pad-slots").striped(true).show(ui, |ui| {
            for slot in &mut self.state.controllers.pads.slots {
                ui.label(format!("{}", slot.number() + 1));

                let mut source = slot.source;
                egui::ComboBox::from_id_salt(slot.number())
                    .selected_text(source.describe())
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut source, pros_link::pads::Source::Empty, "nothing");
                        ui.selectable_value(
                            &mut source,
                            pros_link::pads::Source::Keyboard,
                            "keyboard",
                        );
                        ui.selectable_value(
                            &mut source,
                            pros_link::pads::Source::Controller(slot.number()),
                            "controller",
                        );
                    });
                slot.source = source;

                if source.is_readable() {
                    // Live, so a mapping can be checked by pressing something.
                    let state = slot.state;
                    // Glyphs, because this line is read while looking at a controller.
                    let mut lit = String::new();
                    for button in pros_link::pad::Button::ALL {
                        if state.holds(button) {
                            lit.push_str(button.glyph());
                            lit.push(' ');
                        }
                    }
                    // Shown as the target reads them: a byte each, centred on 128.
                    ui.monospace(format!("{:>3} {:>3}", state.left_x, state.left_y));
                    ui.label(lit);
                } else if matches!(source, pros_link::pads::Source::Controller(_)) {
                    ui.weak("nothing here can read a controller yet");
                    ui.label("");
                } else {
                    ui.weak("");
                    ui.label("");
                }
                ui.end_row();
            }
            let _ = &mut binding;
        });
        if let Some(button) = binding {
            self.state.controllers.binding = Some(button);
        }
    }

    /// One slot's key layout, and a way to change it.
    ///
    /// Per slot, not shared: two people on one keyboard need two layouts.
    fn pad_keys(&mut self, ui: &mut egui::Ui) {
        let waiting = self.state.controllers.binding;
        let chosen = self.state.controllers.binding_slot;
        for slot in 0..self.state.controllers.pads.slots.len() {
            let number = self.state.controllers.pads.slots[slot].number();
            let bound = self.state.controllers.pads.slots[slot].is_bound();
            let title = if bound {
                format!("pad {} keys", number + 1)
            } else {
                format!("pad {} keys  (nothing bound)", number + 1)
            };
            let mut rebind = None;
            egui::CollapsingHeader::new(title)
                .id_salt(("keys", number))
                .default_open(false)
                .show(ui, |ui| {
                    if let Some(button) = waiting
                        && chosen == Some(number)
                    {
                        ui.colored_label(
                            egui::Color32::from_rgb(120, 200, 140),
                            // The word as well as the shape: out of context the glyph alone is
                            // ambiguous.
                            format!(
                                "press a key for {} {} - escape to abandon",
                                button.glyph(),
                                button.name()
                            ),
                        );
                        ui.separator();
                    }
                    egui::Grid::new(("pad-keys", number))
                        .striped(true)
                        .show(ui, |ui| {
                            for button in pros_link::pad::Button::ALL {
                                // The shape finds the row; the word is what a saved layout
                                // contains.
                                ui.horizontal(|ui| {
                                    ui.monospace(button.glyph());
                                    ui.weak(button.name());
                                });
                                let key = self.state.controllers.pads.slots[slot]
                                    .keys
                                    .key_for(button)
                                    .unwrap_or("-")
                                    .to_owned();
                                ui.monospace(&key);
                                if ui.small_button("change").clicked() {
                                    rebind = Some(button);
                                }
                                ui.end_row();
                            }
                        });
                    ui.small("sticks are on the movement keys and cannot be changed here yet");
                });
            if let Some(button) = rebind {
                self.state.controllers.binding = Some(button);
                self.state.controllers.binding_slot = Some(number);
            }
        }
    }
}
