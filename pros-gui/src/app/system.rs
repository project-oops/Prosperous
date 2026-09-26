//! The system panel: what the target is, its storage, and its processes.

use super::widgets::section_heading;
use super::{App, SYSTEM_REFRESH};
use crate::state::{Job, Section};

/// What was pressed on a row of the process list.
///
/// A title is closed by identity (every process it owns); anything else is ended by its one
/// pid. The split is the one `pros close` and `pros kill` make.
enum ProcAction {
    /// Close a title by its identifier.
    CloseTitle(String),
    /// End a single process by its pid.
    EndPid(String),
}

/// How the system panel orders its process list.
///
/// Applied within each section (titles, then everything else) so the titles-first grouping
/// survives the sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ProcSort {
    /// As `ps` listed them.
    #[default]
    Listed,
    /// Most memory in use first. A row with no memory figure sorts last, not as zero.
    Memory,
    /// Grouped by state, so the stopped and the running sit together.
    State,
}

impl ProcSort {
    /// Orders a list of processes in place by this choice.
    fn arrange(self, processes: &mut [&pros_core::system::Process]) {
        match self {
            Self::Listed => {}
            // Descending; a row with no figure sorts after every row that has one.
            Self::Memory => processes.sort_by(|a, b| {
                let key = |p: &pros_core::system::Process| {
                    p.memory
                        .as_ref()
                        .and_then(pros_core::system::Memory::current_mib)
                };
                key(b)
                    .partial_cmp(&key(a))
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            Self::State => processes.sort_by(|a, b| a.state.cmp(&b.state)),
        }
    }

    /// What to call this in a chooser.
    fn label(self) -> &'static str {
        match self {
            Self::Listed => "as listed",
            Self::Memory => "memory",
            Self::State => "state",
        }
    }
}

impl App {
    /// What the target is: firmware, storage, and what is running.
    ///
    /// Nothing is filled in from anything else: a field the target did not answer stays
    /// empty, because a plausible value is indistinguishable from a measured one.
    pub(super) fn system_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::System);

        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        self.system_toolbar(ui, idle, connected);
        ui.add_space(8.0);
        self.refresh_if_due(ui, idle, connected);

        let Some(report) = self.state.system.clone() else {
            ui.weak(if self.state.is_idle() {
                "select a target, and this asks it"
            } else {
                "asking..."
            });
            return;
        };

        for fact in &report.facts {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [90.0, 18.0],
                    egui::Label::new(fact.name).halign(egui::Align::LEFT),
                );
                ui.monospace(&fact.value);
            });
        }
        if report.facts.is_empty() {
            ui.weak("the target answered none of the questions this knows to ask");
        }

        Self::storage_table(ui, &report);
        if let Some(act) = Self::process_list(ui, &report, idle, self.system_sort)
            && let Some(target) = self.state.target().cloned()
        {
            match act {
                ProcAction::CloseTitle(id) => self.state.begin(Job::CloseTitle(target, id)),
                ProcAction::EndPid(pid) => self.state.begin(Job::EndProcess(target, pid)),
            };
        }
    }

    /// Reading the target, restarting its interface, and how the process list is kept.
    fn system_toolbar(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle && connected, egui::Button::new("ask the target"))
                .on_disabled_hover_text("no target selected")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state.begin(Job::ReadSystem(target));
                // So auto-refresh does not fire again immediately after a manual read.
                self.system_asked_at = Some(std::time::Instant::now());
            }
            // Restarts the interface to clear a softlock. Beside the reading rather than on a
            // process row: it restarts the whole screen, not one listed process.
            if ui
                .add_enabled(idle && connected, egui::Button::new("restart UI"))
                .on_hover_text("kill SceShellUI to clear a softlock; the system respawns it")
                .on_disabled_hover_text("no target selected")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state.begin(Job::RestartUi(target));
            }
        });
        ui.horizontal(|ui| {
            ui.label("sort:");
            // A local copy, so the combo's closure does not capture `self` a second time.
            let mut chosen = self.system_sort;
            egui::ComboBox::from_id_salt("proc-sort")
                .selected_text(chosen.label())
                .show_ui(ui, |ui| {
                    for option in [ProcSort::Listed, ProcSort::Memory, ProcSort::State] {
                        ui.selectable_value(&mut chosen, option, option.label());
                    }
                });
            self.system_sort = chosen;
            ui.separator();
            ui.checkbox(&mut self.system_auto, "auto-refresh")
                .on_hover_text(format!(
                    "re-read the target every {}s while this panel is open",
                    SYSTEM_REFRESH.as_secs()
                ));
        });
    }

    /// Reads the target again when auto-refresh is on and the interval has passed.
    ///
    /// The timestamp keeps this from asking every frame, `idle` keeps it from stacking reads,
    /// and `request_repaint_after` wakes the window to check.
    fn refresh_if_due(&mut self, ui: &egui::Ui, idle: bool, connected: bool) {
        if self.system_auto && idle && connected {
            let due = self
                .system_asked_at
                .is_none_or(|when| when.elapsed() >= SYSTEM_REFRESH);
            if due && let Some(target) = self.state.target().cloned() {
                self.state.begin(Job::ReadSystem(target));
                self.system_asked_at = Some(std::time::Instant::now());
            }
            ui.ctx().request_repaint_after(SYSTEM_REFRESH);
        }
    }

    /// The target's own storage, with its sandbox mounts folded away.
    fn storage_table(ui: &mut egui::Ui, report: &pros_core::system::Report) {
        if !report.storage.is_empty() {
            // Sandbox mounts go behind a fold. Measured on a target: most listed filesystems
            // are bind mounts inside running applications, and would bury the real storage.
            let (sandboxed, real): (Vec<_>, Vec<_>) = report
                .storage
                .iter()
                .partition(|one| one.is_a_sandbox_mount());
            ui.add_space(10.0);
            ui.strong(format!("storage  ({})", real.len()));
            egui::Grid::new("storage").striped(true).show(ui, |ui| {
                ui.weak("mounted on");
                ui.weak("size");
                ui.weak("free");
                ui.weak("full");
                ui.end_row();
                for one in &real {
                    ui.label(&one.at);
                    ui.monospace(&one.size);
                    ui.monospace(&one.free);
                    ui.monospace(&one.full);
                    ui.end_row();
                }
            });
            if !sandboxed.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "{} sandbox mounts, from running applications",
                    sandboxed.len()
                ))
                .default_open(false)
                .show(ui, |ui| {
                    for one in &sandboxed {
                        ui.weak(&one.at);
                    }
                });
            }
        }
    }

    /// Draws the running processes, titles first, and returns the action whose button was
    /// pressed.
    ///
    /// A value comes back because this view cannot reach the state; the caller dispatches it.
    fn process_list(
        ui: &mut egui::Ui,
        report: &pros_core::system::Report,
        idle: bool,
        sort: ProcSort,
    ) -> Option<ProcAction> {
        let mut act: Option<ProcAction> = None;
        let mut titles: Vec<&pros_core::system::Process> = report
            .processes
            .iter()
            .filter(|one| one.is_a_title())
            .collect();
        sort.arrange(&mut titles);
        if !report.processes.is_empty() {
            ui.add_space(10.0);
            ui.strong(format!(
                "running  ({} processes, {} of them titles)",
                report.processes.len(),
                titles.len()
            ));
            for one in &titles {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(idle, egui::Button::new("close").small())
                        .on_hover_text("end this title and free what it holds open")
                        .on_disabled_hover_text("busy")
                        .clicked()
                    {
                        act = Some(ProcAction::CloseTitle(one.title.clone()));
                    }
                    ui.monospace(&one.title);
                    ui.label(&one.command);
                    Self::memory_label(ui, one);
                    ui.weak(&one.state);
                });
            }
            egui::CollapsingHeader::new("everything else")
                .default_open(false)
                .show(ui, |ui| {
                    let mut others: Vec<&pros_core::system::Process> = report
                        .processes
                        .iter()
                        .filter(|one| !one.is_a_title())
                        .collect();
                    sort.arrange(&mut others);
                    for one in &others {
                        ui.horizontal(|ui| {
                            // No title to close, so it is ended by pid, as `pros kill` does.
                            if ui
                                .add_enabled(idle, egui::Button::new("end").small())
                                .on_hover_text(
                                    "end this process by pid (SIGKILL, waking it first if stopped)",
                                )
                                .on_disabled_hover_text("busy")
                                .clicked()
                            {
                                act = Some(ProcAction::EndPid(one.pid.clone()));
                            }
                            ui.weak(&one.pid);
                            ui.label(&one.command);
                            Self::memory_label(ui, one);
                            ui.weak(&one.state);
                        });
                    }
                });
        }
        act
    }

    /// The memory figure for a process row, current MiB with the peak on hover.
    ///
    /// Blank for a row with no figure, rather than a `0` that would read as a measurement.
    fn memory_label(ui: &mut egui::Ui, process: &pros_core::system::Process) {
        if let Some(memory) = &process.memory {
            ui.weak(format!("{} MiB", memory.current))
                .on_hover_text(format!("peak {} MiB", memory.peak));
        }
    }
}
