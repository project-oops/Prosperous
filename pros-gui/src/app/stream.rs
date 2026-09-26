//! The stream panel: watching the target's video and where it comes from.

use std::sync::Mutex;
use std::sync::mpsc::Receiver;

use super::App;
use super::widgets::{section_heading, size};
use crate::state::Section;

static PLAYER_INSTALL: Mutex<Option<Receiver<pros_core::Result<std::path::PathBuf>>>> =
    Mutex::new(None);

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

    /// Starts downloading and installing a vendored copy of mpv on a background thread.
    pub(super) fn install_player(&mut self) {
        if self.state.stream.installing_player {
            return;
        }
        self.state.stream.installing_player = true;
        if pros_core::watch::configured().is_none() {
            let _ = pros_core::watch::write_example();
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        if let Ok(mut held) = PLAYER_INSTALL.lock() {
            *held = Some(receiver);
        }
        std::thread::spawn(move || {
            let res = pros_core::watch::install_vendored_mpv();
            let _ = sender.send(res);
        });
    }

    /// Checks if a background player download/extract has finished.
    fn poll_player_install(&mut self, ctx: &egui::Context) {
        let Ok(mut held) = PLAYER_INSTALL.lock() else {
            return;
        };
        let Some(rx) = held.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.state.stream.installing_player = false;
                self.state.said = format!("installed player to {}", path.display());
                if self.state.stream.watch_after_install {
                    self.state.stream.watch_after_install = false;
                    let target_ip = self.state.stream.target_ip.clone();
                    self.begin_watching(&target_ip);
                }
                *held = None;
            }
            Ok(Err(why)) => {
                self.state.stream.installing_player = false;
                self.state.stream.watch_after_install = false;
                self.state.trouble = Some(format!("failed to install player: {why}"));
                *held = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.state.stream.installing_player = false;
                self.state.stream.watch_after_install = false;
                *held = None;
            }
        }
    }

    /// Watching the target, over our own stream.
    ///
    /// Connect, watch what goes past, and drive it. Nothing is disabled pending a payload:
    /// with none running, watch reports the refused connection and names the port.
    pub(super) fn stream_panel(&mut self, ui: &mut egui::Ui) {
        self.poll_player_install(ui.ctx());
        section_heading(ui, Section::Stream);

        Self::watch_intro(ui);
        ui.add_space(8.0);
        self.watch_bar(ui);
        ui.add_space(10.0);
        self.watch_counts(ui);
        ui.add_space(12.0);
        self.watch_input(ui);
    }

    /// A brief description linking to Porthole Companion.
    fn watch_intro(ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Watch the target's live video stream and forward controller input over the network. Requires");
            ui.hyperlink_to(
                "Porthole Companion",
                "https://project-oops.github.io/oops-apps/#porthole-companion",
            );
            ui.label("running on the target.");
        });
    }

    /// Start it, stop it, and say plainly where it stands.
    fn watch_bar(&mut self, ui: &mut egui::Ui) {
        self.watch_controls(ui);
        ui.add_space(6.0);
        self.player_config(ui);
    }

    /// IP, port, and start/stop controls for watching.
    fn watch_controls(&mut self, ui: &mut egui::Ui) {
        let counts = self.state.stream.watching.counts();
        let running = counts.status.is_watching();

        if self.state.stream.target_ip.is_empty() {
            let address = self.state.target().map(|target| target.address.clone());
            if let Some(address) = address {
                self.state.stream.target_ip = address;
            }
        }

        ui.horizontal(|ui| {
            ui.small("target IP");
            ui.add(
                egui::TextEdit::singleline(&mut self.state.stream.target_ip)
                    .hint_text("e.g. 192.168.1.205")
                    .desired_width(110.0),
            );

            ui.add_space(4.0);
            ui.small("port");
            ui.add(
                egui::TextEdit::singleline(&mut self.state.stream.watch_port).desired_width(50.0),
            );

            ui.add_space(6.0);
            let address = self.state.stream.target_ip.trim().to_owned();
            let has_address = !address.is_empty();

            if running {
                if ui
                    .button("stop")
                    .on_hover_text("closes the player's input, which ends it cleanly")
                    .clicked()
                {
                    self.state.stream.watching.stop();
                    self.state.controllers.feed.close();
                }
            } else if ui
                .add_enabled(
                    has_address && !self.state.stream.installing_player,
                    egui::Button::new("watch"),
                )
                .on_disabled_hover_text("enter the target IP shown in Porthole Companion")
                .on_hover_text("connects to the target and starts the player")
                .clicked()
            {
                if pros_core::watch::configured_player_available() {
                    self.begin_watching(&address);
                } else {
                    self.state.stream.watch_after_install = true;
                    self.install_player();
                }
            }

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
    }

    /// Shows the configured player command and warns/installs if missing.
    fn player_config(&mut self, ui: &mut egui::Ui) {
        let player_available = pros_core::watch::configured_player_available();

        if let Some(command) = pros_core::watch::configured() {
            ui.horizontal(|ui| {
                ui.small("player");
                ui.monospace(&command);
                if ui
                    .small_button("edit")
                    .on_hover_text("open or reveal player.txt to change the player command")
                    .clicked()
                {
                    self.write_player_example();
                }
            });
            if !player_available {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(210, 190, 120),
                        "player 'mpv' is not installed",
                    );
                    if self.state.stream.installing_player {
                        ui.spinner();
                        ui.label("installing mpv into app data...");
                    } else if ui
                        .button("install mpv (vendored into app data)")
                        .on_hover_text(
                            "downloads and extracts a portable copy of mpv into OOPS app data",
                        )
                        .clicked()
                    {
                        self.install_player();
                    }
                });
            }
            return;
        }

        // No player configured: make it easy to configure with one click.
        ui.horizontal(|ui| {
            ui.label("Video is piped to a local player (default: mpv).");
            if self.state.stream.installing_player {
                ui.spinner();
                ui.label("installing mpv into app data...");
            } else if ui
                .button("configure default player (mpv)")
                .on_hover_text("writes the default mpv command to player.txt")
                .clicked()
            {
                self.write_player_example();
                if !player_available {
                    self.install_player();
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
        ui.horizontal(|ui| {
            ui.small("For the stream format and input record specification, see the");
            if ui.link("Video & Stream documentation").clicked() {
                self.docs.open_at("video");
            }
        });
    }

    /// Connects and starts the player, or says why it did not.
    ///
    /// Also opens the input feed, since watching is usually playing. The two fail
    /// independently: a feed that will not open says so and the picture still comes up.
    pub(super) fn begin_watching(&mut self, address: &str) {
        if !pros_core::watch::configured_player_available() {
            self.state.stream.watch_after_install = true;
            self.install_player();
            return;
        }
        let command = pros_core::watch::configured().or_else(|| {
            pros_core::watch::write_example().ok()?;
            pros_core::watch::configured()
        });
        let Some(command) = command else {
            self.state.trouble =
                Some("no player configured - write player.txt to configure one".to_owned());
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
        self.state.stream.watching = pros_core::watch::Watching::start(address, port, &command);

        if !self.state.controllers.feed.status.is_sending() {
            let port = self
                .state
                .controllers
                .feed_port
                .trim()
                .parse()
                .unwrap_or(pros_link::feed::PORT);
            // The failure stays in the feed's status, drawn beside the input line.
            let _ = self.state.controllers.feed.open(address, port);
        }
    }
}
