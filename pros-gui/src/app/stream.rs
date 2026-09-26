//! The stream panel: watching the target's video and where it comes from.

use super::App;
use super::widgets::{section_heading, size};
use crate::state::Section;

impl App {
    /// Writes the player command file, so a person meeting a disabled button knows what to
    /// write and where.
    pub(super) fn write_player_example(&mut self) {
        let Some(path) = pros_core::watch::command_path() else {
            self.state.trouble = Some("no home directory, so there is nowhere for it".to_owned());
            return;
        };
        match pros_core::watch::write_example() {
            Ok(written) => {
                self.state.said = format!("the player command is in {}", written.display());
                if let Some(at) = written.parent() {
                    self.reveal(at);
                }
            }
            Err(why) => self.state.trouble = Some(format!("{}: {why}", path.display())),
        }
    }

    /// Watching the target, over our own stream.
    ///
    /// Connect, watch what goes past, and drive it. Nothing is disabled pending a payload:
    /// with none running, watch reports the refused connection and names the port.
    pub(super) fn stream_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::Stream);

        self.watch_bar(ui);
        ui.add_space(10.0);
        self.watch_counts(ui);
        ui.add_space(12.0);
        self.watch_input(ui);
    }

    /// Start it, stop it, and say plainly where it stands.
    fn watch_bar(&mut self, ui: &mut egui::Ui) {
        let counts = self.state.stream.watching.counts();
        let running = counts.status.is_watching();
        let target = self.state.target().cloned();

        ui.horizontal(|ui| {
            if running {
                if ui
                    .button("stop")
                    .on_hover_text("closes the player's input, which ends it cleanly")
                    .clicked()
                {
                    self.state.stream.watching.stop();
                }
            } else if ui
                .add_enabled(target.is_some(), egui::Button::new("watch"))
                .on_disabled_hover_text("choose a target first")
                .clicked()
                && let Some(target) = target
            {
                self.begin_watching(&target.link());
            }

            ui.add_space(6.0);
            ui.small("port");
            ui.add(
                egui::TextEdit::singleline(&mut self.state.stream.watch_port).desired_width(56.0),
            );

            ui.add_space(6.0);
            let (colour, said) = match &counts.status {
                pros_core::watch::Status::Watching => (
                    egui::Color32::from_rgb(120, 200, 140),
                    counts.status.describe(),
                ),
                pros_core::watch::Status::Idle => (egui::Color32::GRAY, "not watching".to_owned()),
                // Ended and failed are red and described, never drawn like idle.
                pros_core::watch::Status::Ended(_) | pros_core::watch::Status::Failed(_) => (
                    egui::Color32::from_rgb(220, 130, 130),
                    counts.status.describe(),
                ),
            };
            ui.colored_label(colour, said);
        });

        ui.add_space(4.0);
        if let Some(command) = pros_core::watch::configured() {
            ui.horizontal(|ui| {
                ui.small("player");
                ui.monospace(command);
            });
            return;
        }

        // No player configured: not an error, and the button below writes the file.
        ui.label("The stream is piped to whatever plays video on this machine. This project");
        ui.label("decodes nothing - it counts what goes past instead, which is how it can");
        ui.label("tell you which kind of nothing you are looking at.");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("write the file to edit").clicked() {
                match pros_core::watch::write_example() {
                    Ok(path) => self.state.said = path.display().to_string(),
                    Err(why) => self.state.trouble = Some(why.to_string()),
                }
            }
            if let Some(path) = pros_core::watch::command_path() {
                ui.small(path.display().to_string());
            }
        });
    }

    /// What has gone past, and what to make of it.
    ///
    /// A player shows no picture for several different faults; counting the bytes on the way
    /// through tells them apart: nothing arrived, the wrong kind of stream arrived, units
    /// arrived with no keyframe (which decodes to nothing), or the player went away.
    fn watch_counts(&mut self, ui: &mut egui::Ui) {
        let counts = self.state.stream.watching.counts();
        if matches!(counts.status, pros_core::watch::Status::Idle) {
            return;
        }

        egui::Grid::new("watch-counts")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                // The rate leads: every other figure only climbs, so only the rate tells a
                // stream from a slideshow.
                ui.label("rate");
                match counts.rate {
                    Some(rate) => {
                        ui.colored_label(
                            if rate.is_moving() {
                                egui::Color32::from_rgb(120, 200, 140)
                            } else {
                                egui::Color32::from_rgb(210, 190, 120)
                            },
                            egui::RichText::new(rate.describe()).monospace(),
                        );
                    }
                    // Not zero: no full second has been measured yet.
                    None => {
                        ui.weak("measuring");
                    }
                }
                ui.end_row();
                ui.label("arrived");
                ui.monospace(size(counts.bytes));
                ui.end_row();
                ui.label("units");
                ui.monospace(counts.units.to_string());
                ui.end_row();
                ui.label("keyframes");
                ui.monospace(counts.keyframes.to_string());
                ui.end_row();
                if counts.pending > 0 {
                    // A held figure that only climbs means the stream stopped producing unit
                    // boundaries.
                    ui.label("held");
                    ui.monospace(size(counts.pending as u64));
                    ui.end_row();
                }
            });

        if let Some(said) = counts.diagnose() {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::from_rgb(210, 190, 120), said);
        }
    }

    /// The input half, beside the picture it belongs to.
    ///
    /// On the same screen, because watching a target and playing it are one activity.
    fn watch_input(&mut self, ui: &mut egui::Ui) {
        let driving = self.state.controllers.pads.filled();
        ui.horizontal(|ui| {
            ui.strong("input");
            let sending = self.state.controllers.feed.status.is_sending();
            ui.colored_label(
                if sending {
                    egui::Color32::from_rgb(120, 200, 140)
                } else {
                    egui::Color32::GRAY
                },
                self.state.controllers.feed.status.describe(),
            );
        });
        ui.horizontal(|ui| {
            if ui
                .button("controllers")
                .on_hover_text("which pads are driven from here, and what each key does")
                .clicked()
            {
                self.state.section = Section::Controllers;
            }
            ui.small(format!(
                "{driving} of {} slots driven from this machine",
                pros_link::pad::SLOTS
            ));
        });
        ui.add_space(4.0);
        ui.small("the design of both halves is docs/vIDEO.md part three");
    }

    /// Connects and starts the player, or says why it did not.
    ///
    /// Also opens the input feed, since watching is usually playing. The two fail
    /// independently: a feed that will not open says so and the picture still comes up.
    fn begin_watching(&mut self, link: &pros_link::Link) {
        let Some(command) = pros_core::watch::configured() else {
            self.state.trouble =
                Some("no player named yet - the button below writes the file to edit".to_owned());
            return;
        };
        // An unparsable port falls back to the documented one rather than refusing.
        let port = self
            .state
            .stream
            .watch_port
            .trim()
            .parse()
            .unwrap_or(pros_core::watch::PORT);
        self.state.stream.watching =
            pros_core::watch::Watching::start(&link.address, port, &command);

        if !self.state.controllers.feed.status.is_sending() {
            let port = self
                .state
                .controllers
                .feed_port
                .trim()
                .parse()
                .unwrap_or(pros_link::feed::PORT);
            // The failure stays in the feed's status, drawn beside the input line.
            let _ = self.state.controllers.feed.open(&link.address, port);
        }
    }
}
