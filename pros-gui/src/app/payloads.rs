//! The payloads panel: what is described, what the target holds, and what each project has
//! released.

use std::path::PathBuf;
use std::time::Duration;

use pros_core::payloads::{Boot, Presence, Standing, There, Trust};

use super::App;
use super::widgets::{choose_a_file, half_of, headings, how_long, pane, section_heading, size};
use crate::state::{Job, Section};

/// Everything one row of the payload table draws itself from.
struct Shown<'a> {
    /// The rows in this group.
    rows: &'a [&'a pros_core::payloads::Row<'a>],
    /// What the target holds.
    on_target: &'a [There],
    /// What each project has released, as far as anything has asked.
    sources: &'a pros_core::sources::Sources,
    /// What is ticked.
    chosen: &'a std::collections::BTreeSet<String>,
    /// Whether anything can be started right now.
    idle: bool,
}

/// What the payload table was asked to do, collected while it draws.
///
/// Acted on after the grid, because starting a job borrows the state the grid was drawn from.
#[derive(Debug, Default)]
struct Wanted {
    /// A row whose tick changed.
    ticked: Option<String>,
    /// A row whose list entry should be pointed at the project's latest release.
    relist: Option<String>,
}

/// The word and colour for one finding, from its verdict and how much it matters.
///
/// A check nobody could run and a check that passed are drawn differently, so an unreachable
/// target never looks healthy.
pub(super) fn mark_of(
    verdict: &pros_core::doctor::Verdict,
    gravity: pros_core::recovery::Gravity,
) -> (&'static str, egui::Color32) {
    use pros_core::doctor::Verdict;
    use pros_core::recovery::Gravity;
    match (verdict, gravity) {
        (Verdict::Well(_), _) => ("ok", egui::Color32::from_rgb(120, 190, 120)),
        (Verdict::Unknown(_), _) => ("?", egui::Color32::GRAY),
        (Verdict::Aside(_), _) => ("--", egui::Color32::GRAY),
        (Verdict::Unwell { .. }, Gravity::Warning) => {
            ("warning", egui::Color32::from_rgb(210, 190, 120))
        }
        (Verdict::Unwell { .. }, Gravity::Critical) => {
            ("CRITICAL", egui::Color32::from_rgb(230, 90, 90))
        }
    }
}

impl App {
    /// Takes in whatever the sweep has answered, and keeps it.
    ///
    /// Written to disk as answers arrive, not at the end, so an interrupted sweep keeps what it
    /// learnt.
    pub(super) fn take_sweep_answers(&mut self) {
        let Some(sweep) = self.sweep.as_mut() else {
            return;
        };
        let arrived = sweep.drain();
        let ended = sweep.has_ended();
        if !arrived.is_empty() {
            for answer in arrived {
                self.sources.put(&answer.name, answer.found);
            }
            // A failed save is not reported: the answers are still usable this run.
            let _ = pros_core::sources::save(&self.sources);
        }
        if ended {
            self.sweep = None;
        }
    }

    /// Starts asking the projects that have not been asked recently.
    ///
    /// `forced` ignores how fresh the stored answers are - the button - where the sweep at
    /// launch only asks about what has gone stale.
    pub(super) fn check_sources(&mut self, forced: bool) {
        if self.sweep.is_some() {
            return;
        }
        let Some(manifest) = self.manifest.as_ref() else {
            return;
        };
        let window = if forced {
            Duration::ZERO
        } else {
            pros_core::sources::STALE
        };
        let due: Vec<pros_core::manifest::Payload> = self
            .sources
            .due(manifest.payloads(), window)
            .into_iter()
            .cloned()
            .collect();
        let wanted = due.len();
        self.sweep = crate::sweep::Sweep::start(due);
        if self.sweep.is_none() && forced {
            // Said, because nothing visible would otherwise happen.
            self.state.said = if wanted == 0 {
                "every project with a release page was asked recently - nothing to ask".to_owned()
            } else {
                "nothing to ask".to_owned()
            };
        }
    }

    /// Re-reads the manifest the same way startup does.
    ///
    /// `Tracked::read` merges the shipped catalogue over the file on disk and writes the result
    /// back, so both a shipped addition and a hand-edit appear. A raw file read would miss
    /// payloads added to the shipped catalogue.
    fn read_manifest(&mut self) {
        match pros_core::manifest::Tracked::Payloads.read() {
            Ok(manifest) => self.manifest = Some(manifest),
            // No file is `Ok(shipped)`, so this is only a file that does not parse.
            Err(why) => self.state.trouble = Some(why.to_string()),
        }
    }

    /// What is described, what can be trusted, and what is on the target.
    pub(super) fn payloads_body(&mut self, ui: &mut egui::Ui) {
        // The same two panes and toolbar as every other section; only the left pane differs,
        // showing what a directory listing cannot.
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();

        section_heading(ui, Section::Payloads);

        self.rebuild_listing();
        self.sync_toolbar(ui);
        ui.separator();
        self.refusal(ui);
        self.pending_install(ui, idle);
        self.pending_delete(ui, idle);
        self.adhoc(ui);

        if self.state.files.merged {
            self.merged_view(ui);
            return;
        }
        let size = half_of(ui);
        ui.horizontal_top(|ui| {
            pane(ui, "payloads-here", size, |ui| self.payloads_here(ui));
            ui.separator();
            pane(ui, "payloads-there", size, |ui| {
                self.there_side(ui, idle, connected);
            });
        });
    }

    /// The left half of the payloads view: what is described, and what is true of it.
    ///
    /// Its own table rather than a directory listing: a payload has a digest, a place in the
    /// boot order and a service that answers or not, which a file listing cannot show.
    fn payloads_here(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("here");
            if ui
                .button("refresh")
                .on_hover_text("re-read the list from disk")
                .clicked()
            {
                self.read_manifest();
            }
            if ui
                .button("run from file...")
                .on_hover_text(
                    "choose an ELF and run it - opens in this machine's payload folder, and                      will go anywhere else on the disk",
                )
                .clicked()
            {
                // Created before the dialog opens: `rfd` ignores a missing directory and opens
                // wherever it last was.
                let from = PathBuf::from(self.state.files.local_path.trim());
                if !from.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(&from);
                }
                if let Some(chosen) =
                    choose_a_file("an ELF to run", &["elf"], &self.state.files.local_path)
                {
                    self.state.files.adhoc = Some(chosen);
                }
            }
            if ui
                .button("open folder")
                .on_hover_text("show it in this machine's file browser")
                .clicked()
            {
                // `local_path` (`data_root()/payloads`), the folder the row actions, the toolbar
                // and downloads all use, not the staging directory.
                let path = PathBuf::from(self.state.files.local_path.trim());
                if !path.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(&path);
                    self.reveal(&path);
                }
            }
            self.sources_control(ui);
        });
        // The folder every judgement on this screen is made against. This pane has no path
        // box, so it is shown here.
        ui.horizontal(|ui| {
            ui.weak("here:");
            ui.weak(&self.state.files.local_path)
                .on_hover_text("what `run`, `send` and `delete here` are judged against");
            ui.weak(format!(
                "({} file{})",
                self.state.files.local.len(),
                if self.state.files.local.len() == 1 { "" } else { "s" }
            ));
            if ui
                .button("re-read")
                .on_hover_text(
                    "list it again - it is read when this screen is opened, so a file put                      there since is not known about yet",
                )
                .clicked()
            {
                self.read_local();
            }
        });

        if self.manifest.is_none() {
            ui.add_space(6.0);
            ui.label("no list read yet");
            ui.small("this project ships no payload binaries, only a description of where");
            ui.small("they come from - payloads > read manifest");
            return;
        }

        ui.horizontal(|ui| {
            ui.small("install to:");
            ui.text_edit_singleline(&mut self.state.payloads.install_dir);
        });
        ui.separator();

        self.payload_rows(ui);
    }

    /// What a toolbar button says, given what is selected.
    ///
    /// Only `download` changes: it says `update` when every selected payload already has an
    /// older copy on this disk.
    pub(super) fn says(&self, offer: crate::listing::Offer) -> &'static str {
        if offer != crate::listing::Offer::Download {
            return offer.label();
        }
        let Some(manifest) = self.manifest.as_ref() else {
            return offer.label();
        };
        let picked: Vec<&pros_core::manifest::Payload> = manifest
            .payloads()
            .iter()
            .filter(|payload| {
                let key = payload.filename.as_deref().unwrap_or(&payload.name);
                self.state.files.listing.chosen.contains(key)
            })
            .collect();
        if picked.is_empty() {
            return offer.label();
        }
        // Every one, not any: a mixed selection keeps `download`.
        if picked
            .iter()
            .all(|payload| !pros_core::staging::older_here(payload).is_empty())
        {
            return "update";
        }
        offer.label()
    }

    /// Whether the payload list itself is still current, and the one control that asks.
    ///
    /// Answers are cached for hours to stay inside the release host's rate limit, so the age of
    /// the oldest answer is shown beside the button.
    fn sources_control(&mut self, ui: &mut egui::Ui) {
        if let Some(sweep) = self.sweep.as_ref() {
            let (back, asked) = sweep.progress();
            ui.weak(format!("asking projects... {back} of {asked}"))
                .on_hover_text("spaced out on purpose, and it waits out a rate limit");
            return;
        }
        if ui
            .button("check sources")
            .on_hover_text(
                "ask each payload's own project what it has released, so the version column                  can say whether this list is still current",
            )
            .clicked()
        {
            self.check_sources(true);
        }
        match self.sources.oldest() {
            None => {
                ui.weak("not asked").on_hover_text(
                    "no project has been asked yet, so every version here is only what the                      list claims",
                );
            }
            Some(oldest) => {
                let ago = pros_core::sources::now().saturating_sub(oldest);
                ui.weak(format!("checked {}", how_long(ago)))
                    .on_hover_text(format!(
                        "{} projects answered; the oldest answer is this old",
                        self.sources.len()
                    ));
            }
        }
    }

    /// One row per described payload, and what can be done with each.
    fn payload_rows(&mut self, ui: &mut egui::Ui) {
        let Some(manifest) = &self.manifest else {
            return;
        };
        // An empty local folder, the normal first-run state, is said once rather than left to
        // the hover on every greyed `run`.
        if self.state.files.local.is_empty() {
            ui.add_space(4.0);
            ui.small("nothing on this machine yet - download or fetch one, and run turns on");
        }
        let rows = pros_core::payloads::survey(
            manifest,
            self.state.report.as_ref(),
            self.state.chain.as_ref(),
        );
        // The category is a column, not a foldable heading: a folded row is still in
        // `Listing::build`, so the toolbar would act on ticked rows nobody can see. It is drawn
        // only where it changes from the row above.
        let chosen = self.state.files.listing.chosen.clone();
        let on_target = self.state.payloads.there.clone().unwrap_or_default();
        let on_target = on_target.as_slice();
        let mut asked = Wanted::default();
        let idle = self.state.is_idle();
        egui::Grid::new("payloads").striped(true).show(ui, |ui| {
            headings(
                ui,
                &[
                    "",
                    "run",
                    "name",
                    "size",
                    "running",
                    "version",
                    "",
                    "on target",
                    "boot",
                    "group",
                    "trust",
                    "what it is",
                ],
            );
            for (group, rows) in pros_core::payloads::by_category(&rows) {
                Self::payload_group(
                    ui,
                    group,
                    &Shown {
                        rows: &rows,
                        on_target,
                        sources: &self.sources,
                        chosen: &chosen,
                        idle,
                    },
                    &mut asked,
                );
            }
        });
        if let Some(name) = asked.ticked {
            self.state.files.listing.toggle(&name);
        }
        // After the grid: starting a job borrows what it was drawn from.
        if let Some(name) = asked.relist
            && let Some(payload) = self.described_as(&name)
        {
            self.state.begin(Job::Relist(Box::new(payload)));
        }
    }

    /// One category's worth of rows.
    fn payload_group(ui: &mut egui::Ui, group: &str, what: &Shown<'_>, asked: &mut Wanted) {
        let Shown {
            rows,
            on_target,
            sources,
            chosen,
            idle,
        } = *what;
        // No grid of its own: drawing into the caller's keeps every group's columns in line.
        for (at, row) in rows.iter().enumerate() {
            // Worked out first, drawn second, so each cell below is one line in column order.
            let (mark, colour, hover) = Self::running_of(row.presence);
            let (boot, boot_hover) = Self::boot_of(row.boot);
            let (there, there_colour, there_hover) = Self::on_target_of(row, on_target);
            let stale = pros_core::sources::against(row.payload, sources.get(&row.payload.name))
                .is_behind();
            let (listed, listed_colour, listed_hover) = Self::listed_of(row.payload, sources);
            let (bytes, size_hover) = Self::size_of(row.payload);

            // Keyed by filename, the listing's key for an entry; the display name often
            // differs.
            let key = row
                .payload
                .filename
                .clone()
                .unwrap_or_else(|| row.payload.name.clone());
            let mut on = chosen.contains(&key);
            if ui.checkbox(&mut on, "").changed() {
                asked.ticked = Some(key);
            }
            ui.label(&row.payload.name);
            ui.weak(bytes).on_hover_text(size_hover);
            ui.colored_label(colour, mark).on_hover_text(hover);
            ui.colored_label(listed_colour, listed)
                .on_hover_text(listed_hover);
            // A stale list entry needs repointing, not a download (which would fetch the old
            // version). Repointing downloads the new release to learn its digest, the one
            // point where this program takes a file on trust.
            if stale {
                if ui
                    .add_enabled(idle, egui::Button::new("update entry"))
                    .on_hover_text(
                        "point this list entry at the project's latest release - downloads it                          to record its digest, because a new version has none anywhere yet",
                    )
                    .on_disabled_hover_text("wait for what is already running")
                    .clicked()
                {
                    asked.relist = Some(row.payload.name.clone());
                }
            } else {
                ui.label("");
            }
            ui.colored_label(there_colour, there)
                .on_hover_text(there_hover);
            ui.label(boot).on_hover_text(boot_hover);
            // Strong on the first row of a group, dim after, never blank: every row still
            // names its group when the first has scrolled off.
            if at == 0 {
                ui.strong(group);
            } else {
                ui.weak(group);
            }
            match &row.trust {
                Trust::Verifiable => {
                    ui.colored_label(egui::Color32::from_rgb(120, 190, 120), "verifiable");
                }
                Trust::Doubtful(why) => {
                    ui.colored_label(egui::Color32::from_rgb(210, 190, 120), "unverifiable")
                        .on_hover_text(why.to_string());
                }
            }
            ui.label(row.payload.description.as_deref().unwrap_or(""));
            ui.end_row();
        }
    }

    /// How big the staged copy is, when there is one.
    ///
    /// The file on this disk, measured, never the size a description carries. Three answers:
    /// staged and measured, staged and unreadable, and not staged.
    fn size_of(payload: &pros_core::manifest::Payload) -> (String, String) {
        let Some(path) = pros_core::staging::path_for(payload) else {
            return (
                "-".to_owned(),
                "the description names no file, so there is nothing to have here".to_owned(),
            );
        };
        match std::fs::metadata(&path) {
            Ok(about) => (size(about.len()), path.display().to_string()),
            // `NotFound` is a payload not fetched; anything else is a file that could not be
            // read.
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => (
                "-".to_owned(),
                "not on this machine - download it, or fetch it from the target".to_owned(),
            ),
            Err(why) => (
                "?".to_owned(),
                format!("{} could not be read: {why}", path.display()),
            ),
        }
    }

    /// Whether a payload is answering, in three states rather than two.
    ///
    /// A payload with no known port is unknown, not absent.
    fn running_of(presence: Presence) -> (&'static str, egui::Color32, &'static str) {
        match presence {
            Presence::Loaded => ("on", egui::Color32::from_rgb(120, 190, 120), "answering"),
            Presence::NotLoaded => (
                "off",
                egui::Color32::from_rgb(220, 120, 120),
                "its port did not answer",
            ),
            Presence::Unknown => (
                "?",
                egui::Color32::GRAY,
                "no port this project knows, so nothing here can tell",
            ),
        }
    }

    /// Where a payload sits in the startup list.
    ///
    /// A separate question from whether it is running: a service answering now and absent from
    /// the list is gone after the next power cycle.
    fn boot_of(boot: Boot) -> (String, &'static str) {
        match boot {
            Boot::At(at) => (format!("{at}"), "in the boot list, at this position"),
            Boot::NotInList => (
                "-".to_owned(),
                "not in the boot list, so it will not come back after a reboot",
            ),
            Boot::Unknown => (
                "?".to_owned(),
                "the boot list was not read, so nothing here can tell",
            ),
        }
    }

    /// The version the list describes, coloured against what the project has released.
    ///
    /// Grey is not a pass: a project not yet asked and one whose entry matches its latest
    /// release are drawn differently, because a payload list goes out of date silently.
    fn listed_of(
        payload: &pros_core::manifest::Payload,
        sources: &pros_core::sources::Sources,
    ) -> (String, egui::Color32, String) {
        use pros_core::sources::Against;

        let listed = payload.version.clone().unwrap_or_else(|| "-".to_owned());
        match pros_core::sources::against(payload, sources.get(&payload.name)) {
            Against::Current => (
                listed,
                egui::Color32::from_rgb(120, 190, 120),
                "this list describes the project's latest release".to_owned(),
            ),
            Against::Behind { upstream, .. } => (
                format!("{listed} < {upstream}"),
                egui::Color32::from_rgb(230, 160, 90),
                format!(
                    "the project has released {upstream}; this list still describes {listed} - \
                     the list needs updating, not the target"
                ),
            ),
            Against::Different { upstream, .. } => (
                format!("{listed} / {upstream}"),
                egui::Color32::from_rgb(210, 190, 120),
                format!(
                    "the project's latest release is called {upstream} and this list says \
                     {listed} - these cannot be ordered, so neither is called newer"
                ),
            ),
            Against::NotChecked(why) => (listed, egui::Color32::GRAY, why),
        }
    }

    /// The version the target has, coloured against the one the list describes.
    ///
    /// Each machine has its own column (`size` for this one, this for the target, `version`
    /// for the list), so no cell has to say which pair of versions it compares.
    ///
    /// Read from the sidecar the manager writes beside each payload; the file carries no
    /// version. Absent is drawn as absent, never as out of date, and present but unversioned
    /// as neither.
    fn on_target_of(
        row: &pros_core::payloads::Row<'_>,
        on_target: &[There],
    ) -> (String, egui::Color32, String) {
        let installed = on_target.iter().find(|one| {
            pros_core::chain::Chain::parse(&one.name)
                .position(&row.payload.name)
                .is_some()
        });
        match installed.map(|one| one.standing(row.payload)) {
            Some(Standing::Current) => (
                row.payload.version.clone().unwrap_or_default(),
                egui::Color32::from_rgb(120, 190, 120),
                "the target has the version this list describes".to_owned(),
            ),
            Some(Standing::Behind {
                installed,
                described,
            }) => (
                installed.clone(),
                egui::Color32::from_rgb(230, 160, 90),
                format!("the target has {installed}; this list describes {described}"),
            ),
            // Amber, not green: versions that cannot be ordered still differ.
            Some(Standing::Different {
                installed,
                described,
            }) => (
                installed.clone(),
                egui::Color32::from_rgb(210, 190, 120),
                format!(
                    "the target has {installed} and this list describes {described} - these \
                     cannot be ordered, so neither is called newer"
                ),
            ),
            // On the target with no sidecar version, which is distinct from not there.
            Some(Standing::Unknown) => (
                "?".to_owned(),
                egui::Color32::GRAY,
                "it is on the target, and nothing there says which version".to_owned(),
            ),
            None => (
                "-".to_owned(),
                egui::Color32::GRAY,
                "not on the target - send it, and it will come back after a restart only if \
                 it is in the startup list"
                    .to_owned(),
            ),
        }
    }
}
