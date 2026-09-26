//! The shell panel: one command on the target, and what it said.

use super::App;
use super::widgets::{section_heading, section_heading_with};
use crate::state::{Job, Section};

impl App {
    /// A command, and what it printed.
    pub(super) fn shell_panel(&mut self, ui: &mut egui::Ui) {
        if self.state.target().is_none() {
            section_heading(ui, Section::Shell);
            ui.label("no target selected");
            return;
        }
        section_heading_with(ui, Section::Shell, |ui| {
            ui.text_edit_singleline(&mut self.state.command);
            let can = self.state.is_idle() && !self.state.command.trim().is_empty();
            if ui
                .add_enabled(can, egui::Button::new("run"))
                .on_disabled_hover_text("type a command, and wait for what is running")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                let command = self.state.command.clone();
                self.state.begin(Job::Shell(target, command));
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("said")
            .auto_shrink([false, false])
            .show(ui, |ui| ui.monospace(&self.state.said));
    }
}
