//! The two-sided storage sections (packages, titles, saves, cheats, filesystem): this
//! machine on one side, the target on the other.

use std::path::{Path, PathBuf};

use pros_core::library::Kind as LibraryKind;
use pros_core::target;

use super::App;
use super::widgets::{
    Hit, choose_files, entered, fold, group_row, headings, listing_row, pane, parent_of,
    section_heading, side_cell, split_at, splitter, standing_of, up_row,
};
use crate::state::{Job, Section};

/// What was done to the target's listing in one frame.
#[derive(Default)]
struct Gestures {
    /// A row whose tick was toggled.
    toggled: Option<String>,
    /// A folder that was opened, by the target's name for it.
    entered: Option<String>,
    /// Whether the row that goes up a level was used.
    going_up: bool,
    /// A group that was folded or unfolded.
    folds: Option<String>,
}

/// The job one toolbar action implies for one selected entry, if any.
fn job_for(
    offer: crate::listing::Offer,
    entry: &crate::listing::Entry,
    target: &target::Target,
    local: &Path,
    remote: &str,
) -> Option<Job> {
    use crate::listing::Offer;
    match offer {
        // A local copy goes to the loader, which runs it without writing to the
        // target's disk. A payload only on the target is started in place through the
        // shell.
        Offer::Run => match (entry.here.as_ref(), entry.there.as_ref()) {
            (Some(here), _) => Some(Job::Send(
                target.clone(),
                here.name.clone(),
                local.join(&here.name),
            )),
            (None, Some(there)) => Some(Job::RunThere(
                target.clone(),
                on_target(remote, entry, there),
            )),
            (None, None) => None,
        },
        Offer::Send => send_job(entry, target, local, remote),
        Offer::Fetch => {
            let there = entry.there.as_ref()?;
            let from = format!("{remote}/{}", there.name);
            let into = local.join(&there.name);
            Some(if there.folder {
                Job::Backup(target.clone(), from, into)
            } else {
                Job::Pull(target.clone(), from, into)
            })
        }
        Offer::Download => entry
            .described
            .clone()
            .map(|payload| Job::Fetch(Box::new(payload), Some(local.to_path_buf()))),
        Offer::Launch => Some(Job::Launch(target.clone(), entry.name.clone())),
        // Both handled by the caller, each in one go for the whole selection.
        Offer::Install | Offer::DeleteHere | Offer::DeleteThere => None,
    }
}

/// The job that sends one entry from this machine to the target, when it is here.
fn send_job(
    entry: &crate::listing::Entry,
    target: &target::Target,
    local: &Path,
    remote: &str,
) -> Option<Job> {
    let here = entry.here.as_ref()?;
    let from = local.join(&here.name);
    if here.folder {
        // A folder (a save, a title's data) is copied across as it is.
        let to = format!("{remote}/{}", here.name);
        Some(Job::Restore(target.clone(), from, to, false))
    } else if let Some(described) = entry.described.clone() {
        // A payload goes into its own folder: the manager resolves
        // `<dir>/<name>/<file>` (measured, `payloads::on_target_at`) and does not
        // see a flat `<dir>/<name>.elf`. `Job::Install` lays out the folder, the
        // ELF and the `.json` sidecar under `remote`.
        Some(Job::Install(
            target.clone(),
            Box::new(described),
            from,
            remote.to_owned(),
        ))
    } else {
        // Not a described payload, so there is no folder name: a bare file is
        // copied where the browser is pointed.
        let to = format!("{remote}/{}", here.name);
        Some(Job::Push(target.clone(), from, to))
    }
}

/// Where on the target a payload that is already there can be started from.
///
/// The payload manager keeps each payload in its own directory (measured on a target:
/// `/data/pldmgr/payloads/pldmgr/` holds `pldmgr_v0.5.1.elf` and a `.json`), and the shell
/// cannot start a directory. The file name comes from the payload's description; without one
/// the folder path is used as it stands, so the target refuses in its own words rather than
/// this inventing a filename.
fn on_target(remote: &str, entry: &crate::listing::Entry, there: &crate::listing::Side) -> String {
    if !there.folder {
        return format!("{remote}/{}", there.name);
    }
    match entry
        .described
        .as_ref()
        .and_then(|payload| payload.filename.as_deref())
    {
        Some(file) => format!("{remote}/{}/{file}", there.name),
        None => format!("{remote}/{}", there.name),
    }
}

impl App {
    /// Stages anything dropped on the window.
    ///
    /// A dropped file is checked against the manifest entry whose file name it matches before
    /// it is kept.
    pub(super) fn take_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        let Some(manifest) = &self.manifest else {
            self.state.trouble = Some(
                "read a manifest first - a payload is staged against a description".to_owned(),
            );
            return;
        };
        for path in dropped {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string());
            // Matched by file name, the name the manifest states and the publisher used.
            let entry = name.as_ref().and_then(|name| {
                manifest
                    .payloads()
                    .iter()
                    .find(|payload| payload.filename.as_ref() == Some(name))
            });
            match entry {
                Some(payload) => match pros_core::staging::accept(payload, &path) {
                    Ok(into) => self.state.said = format!("staged {}", into.display()),
                    Err(why) => self.state.trouble = Some(why.to_string()),
                },
                // Nothing describes it, the ordinary case for a local build: offered to run
                // rather than refused. A digest checks a publisher's claim, and a local build
                // makes none; its shape is still checked before it is sent.
                None => self.state.files.adhoc = Some(path.clone()),
            }
        }
    }

    /// Puts each section's two sides where they belong, the first time it is shown.
    ///
    /// Only the first time, so navigation survives a visit to another section.
    pub(super) fn settle(&mut self, section: Section) {
        if self.state.files.library_place == Some(section) {
            return;
        }
        self.state.files.library_place = Some(section);
        section
            .there()
            .clone_into(&mut self.state.files.library_path);
        self.state.files.local_path = Self::local_place(section)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.read_local();
        self.browse();
    }

    /// This machine's own folder for a section.
    ///
    /// Beside the registry, one directory per section, so the payload staging directory and
    /// the payloads section are the same folder.
    fn local_place(section: Section) -> Option<PathBuf> {
        Some(target::directory()?.join(section.name()))
    }

    /// Reads the local side.
    ///
    /// Synchronously, unlike the target side: it is a local directory read.
    pub(super) fn read_local(&mut self) {
        let path = PathBuf::from(self.state.files.local_path.trim());
        match pros_core::library::here(&path) {
            Ok(items) => self.state.files.local = items,
            Err(why) => {
                self.state.files.local.clear();
                self.state.trouble = Some(why.to_string());
            }
        }
    }

    /// This machine on the left, the target on the right, and the traffic between them.
    ///
    /// Both at once because the question is comparative. The left side works with no target
    /// registered.
    pub(super) fn sync_body(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, self.state.section);

        self.rebuild_listing();
        self.sync_toolbar(ui);
        ui.separator();
        self.refusal(ui);
        self.pending_install(ui, self.state.is_idle());
        self.pending_delete(ui, self.state.is_idle());

        if self.state.files.merged {
            self.merged_view(ui);
        } else {
            let idle = self.state.is_idle();
            let connected = self.state.target().is_some();
            let (left, right) = split_at(ui, self.state.split);
            let mut dragged = 0.0;
            ui.horizontal_top(|ui| {
                pane(ui, "sync-here", left, |ui| {
                    self.here_side(ui, idle, connected);
                });
                dragged = splitter(ui, left.y);
                pane(ui, "sync-there", right, |ui| {
                    self.there_side(ui, idle, connected);
                });
            });
            if dragged != 0.0 {
                // Kept as a fraction of the usable width, so it survives a resize.
                let usable = (left.x + right.x).max(1.0);
                self.state.split = (self.state.split + dragged / usable).clamp(0.15, 0.85);
            }
        }
    }

    /// Rebuilds the merged listing from the two sides, keeping what is still selected.
    ///
    /// Rebuilt every frame from the sides rather than patched, so it cannot drift from them.
    pub(super) fn rebuild_listing(&mut self) {
        let described = self
            .state
            .section
            .tracks()
            .and_then(|kind| kind.read().ok())
            .unwrap_or_default();
        let chosen = std::mem::take(&mut self.state.files.listing.chosen);
        self.state.files.listing = crate::listing::Listing::build(
            &described,
            &self.state.files.local,
            &self.state.files.library,
        );
        self.state.files.listing.chosen = chosen;
        self.state.files.listing.forget_what_left();
    }

    /// The actions, which apply to what is ticked rather than to one row.
    ///
    /// Disabled with the reason on hover rather than hidden.
    pub(super) fn sync_toolbar(&mut self, ui: &mut egui::Ui) {
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        let mut act = None;
        ui.horizontal_wrapped(|ui| {
            // Only what could ever apply to this section (`Offer::applies_to`).
            for offer in crate::listing::Offer::ALL
                .into_iter()
                .filter(|offer| offer.applies_to(self.state.section))
            {
                let can = self.state.files.listing.offers(offer);
                let allowed = can.is_ok() && idle && connected;
                let refused = match &can {
                    Err(why) => why.clone(),
                    Ok(()) if !connected => "no target selected".to_owned(),
                    Ok(()) => "wait for what is already running".to_owned(),
                };
                if ui
                    .add_enabled(allowed, egui::Button::new(self.says(offer)))
                    .on_hover_text(offer.describes())
                    .on_disabled_hover_text(refused)
                    .clicked()
                {
                    act = Some(offer);
                }
            }

            ui.separator();
            let picked = self.state.files.listing.chosen.len();
            let all = self.state.files.listing.entries.len();
            if ui
                .add_enabled(all > 0, egui::Button::new("all"))
                .on_hover_text("tick everything listed")
                .clicked()
            {
                let names: Vec<String> = self
                    .state
                    .files
                    .listing
                    .entries
                    .iter()
                    .map(|entry| entry.name.clone())
                    .collect();
                self.state.files.listing.chosen.extend(names);
            }
            if ui
                .add_enabled(picked > 0, egui::Button::new("none"))
                .clicked()
            {
                self.state.files.listing.chosen.clear();
            }
            ui.weak(format!("{picked} of {all} selected"));

            ui.separator();
            // One list or two: the split is a projection of the merged model.
            if ui
                .selectable_label(self.state.files.merged, "merged")
                .on_hover_text("one list, with a column for each side")
                .clicked()
            {
                self.state.files.merged = !self.state.files.merged;
            }
        });

        if let Some(offer) = act {
            self.take(offer);
        }
    }

    /// Starts the jobs an action implies, one per selected entry.
    fn take(&mut self, offer: crate::listing::Offer) {
        use crate::listing::Offer;
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        let picked: Vec<crate::listing::Entry> = self
            .state
            .files
            .listing
            .picked()
            .into_iter()
            .cloned()
            .collect();
        if picked.is_empty() {
            return;
        }
        // Both deletes take the whole selection in one job, so the confirm is asked once.
        if offer.is_destructive() {
            self.state.files.pending_delete = Some((offer, picked));
            return;
        }
        let local = PathBuf::from(self.state.files.local_path.trim());
        let remote = self
            .state
            .files
            .library_path
            .trim_end_matches('/')
            .to_owned();

        // Install is a confirm, not a job: one panel names every selected package.
        if offer == Offer::Install {
            self.state.files.pending_install =
                Some(picked.iter().map(|entry| local.join(&entry.name)).collect());
            self.state.files.listing.chosen.clear();
            return;
        }

        // Every selected entry is queued; the worker still runs one job at a time.
        let mut asked = 0_usize;
        for entry in picked {
            if let Some(job) = job_for(offer, &entry, &target, &local, &remote) {
                self.state.queue(job);
                // Unticked as it is queued, so what stays ticked is what was not taken up.
                self.state.files.listing.chosen.remove(&entry.name);
                asked += 1;
            }
        }
        debug_assert!(
            asked > 0 || offer.is_destructive(),
            "a toolbar press did nothing"
        );
    }

    /// The left half: what is on this machine.
    fn here_side(&mut self, ui: &mut egui::Ui, _idle: bool, _connected: bool) {
        ui.horizontal(|ui| {
            ui.strong("here");
            if ui.small_button("refresh").clicked() {
                self.read_local();
            }
            if ui
                .small_button("open folder")
                .on_hover_text("show it in this machine's file browser")
                .clicked()
            {
                self.reveal(&PathBuf::from(self.state.files.local_path.trim()));
            }
            if ui
                .small_button("add files...")
                .on_hover_text("copy files from anywhere on this machine into this folder")
                .clicked()
            {
                self.add_files();
            }
        });
        // Return navigates, so an edited path never sits on screen looking applied.
        if entered(
            ui,
            egui::TextEdit::singleline(&mut self.state.files.local_path),
        ) {
            self.state.files.listing.chosen.clear();
            self.read_local();
        }
        ui.separator();

        // Entries this side knows about, plus anything described and on neither side: that is
        // something to fetch onto this machine.
        let section = self.state.section.name();
        let rows: Vec<crate::listing::Entry> = self
            .state
            .files
            .listing
            .entries
            .iter()
            .filter(|entry| entry.here.is_some() || entry.described.is_some())
            .cloned()
            .collect();
        let mut toggled = None;
        let mut folds = None;
        egui::Grid::new(format!("{section}-here"))
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                headings(ui, &["", "name", "size"]);
                let (present, absent): (Vec<_>, Vec<_>) =
                    rows.iter().partition(|entry| entry.here.is_some());
                for (label, group) in [("on this machine", &present), ("not here yet", &absent)] {
                    if group.is_empty() {
                        continue;
                    }
                    let key = format!("{section}-here-{label}");
                    if group_row(
                        ui,
                        &self.state.files.folded,
                        &key,
                        label,
                        group.len(),
                        &mut folds,
                    ) {
                        continue;
                    }
                    for entry in group {
                        if listing_row(ui, entry, &self.state.files.listing.chosen, false, None)
                            .is_some()
                        {
                            toggled = Some(entry.name.clone());
                        }
                    }
                }
            });
        if let Some(name) = toggled {
            self.state.files.listing.toggle(&name);
        }
        fold(&mut self.state.files.folded, folds);
    }

    /// The target pane's toolbar: where to look, and what to ask it about.
    fn there_toolbar(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        ui.horizontal(|ui| {
            ui.strong("there");
            if ui
                .add_enabled(idle && connected, egui::Button::new("refresh"))
                .on_disabled_hover_text("no target selected")
                .clicked()
            {
                self.browse();
            }
            // Which device. The places under each come from `pros_core::places`, a table of
            // measured paths with the payload that owns each one.
            let now = pros_core::places::device_of(&self.state.files.library_path);
            let mut going_to: Option<String> = None;
            egui::ComboBox::from_id_salt("which-device")
                .selected_text(now.label())
                .show_ui(ui, |ui| going_to = self.device_choices(ui));
            if let Some(path) = going_to {
                self.state.files.library_path = path;
                self.state.files.seen.clear();
                self.browse();
            }
            let above = parent_of(&self.state.files.library_path);
            if ui
                .add_enabled(
                    idle && connected && above.is_some(),
                    egui::Button::new("up"),
                )
                .on_disabled_hover_text("already at the root")
                .clicked()
                && let Some(above) = above
            {
                self.state.files.library_path = above;
                self.browse();
            }
            self.title_buttons(ui, idle, connected);
        });
    }

    /// Each device, with the places under it where this section's things are measured to
    /// live; answers the place picked.
    fn device_choices(&self, ui: &mut egui::Ui) -> Option<String> {
        let mut going_to: Option<String> = None;
        for device in pros_core::places::Device::all() {
            let spots = pros_core::places::where_to_look(self.state.section.looking_for(), device);
            // A device with nothing measured is shown disabled, not left out: an
            // absent entry reads as a device that is not there.
            let Some(first) = spots.first() else {
                ui.add_enabled(false, egui::SelectableLabel::new(false, device.label()))
                    .on_disabled_hover_text(
                        "nothing measured for this kind of thing on a removable device \
                         - browse it from the filesystem screen",
                    );
                continue;
            };
            ui.label(egui::RichText::new(device.label()).strong());
            for spot in &spots {
                if ui
                    .selectable_label(
                        self.state.files.library_path == spot.path,
                        format!("   {}", spot.label),
                    )
                    .on_hover_text(format!("{}\n{}", spot.path, spot.note))
                    .clicked()
                {
                    going_to = Some(spot.path.clone());
                }
            }
            let _ = first;
        }
        going_to
    }

    /// Finding saves, in the saves section, and reading the names of listed titles.
    fn title_buttons(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        // Only where there are titles to name.
        let titles: Vec<String> = self
            .state
            .files
            .library
            .iter()
            .filter(|item| item.kind == LibraryKind::Title)
            .map(|item| item.name.clone())
            .collect();
        // Only in the saves section, where saves sit two folders down under a per-user
        // folder.
        if self.state.section == Section::Saves
            && ui
                .add_enabled(idle && connected, egui::Button::new("find saves"))
                .on_hover_text("saves are under a per-user folder; this finds it")
                .clicked()
            && let Some(target) = self.state.target().cloned()
        {
            self.state.begin(Job::FindSaves(target));
        }
        if !titles.is_empty()
            && ui
                .add_enabled(idle && connected, egui::Button::new("read names"))
                .on_hover_text("ask the target what each of these is called")
                .clicked()
            && let Some(target) = self.state.target().cloned()
        {
            self.state.begin(Job::Names(target, titles));
        }
    }

    /// One button per place this section's things might live.
    ///
    /// There is no standard place (different payloads keep cheats in different directories),
    /// so the choice is the user's.
    ///
    /// Each button has three states: here, not here, or unmarked. Probing stops at the first
    /// directory that answers, so the ones after it were never asked about.
    fn candidate_buttons(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        let candidates = self.state.section.candidates();
        if candidates.is_empty() {
            return;
        }
        let current = self.state.files.library_path.trim().to_owned();
        let mut go = None;
        ui.horizontal_wrapped(|ui| {
            ui.small("keep them in:");
            for place in candidates {
                let path = place.path;
                let known = self
                    .state
                    .files
                    .located
                    .as_ref()
                    .filter(|(asked, _)| *asked == self.state.section)
                    .map(|(_, found)| found)
                    .and_then(|found| match found {
                        pros_core::locate::Where::Found { path: won, .. } if won == path => {
                            Some(true)
                        }
                        pros_core::locate::Where::Found { instead_of, .. } => {
                            instead_of.contains(&path.to_owned()).then_some(false)
                        }
                        pros_core::locate::Where::NoneOfThem(tried) => {
                            tried.contains(&path.to_owned()).then_some(false)
                        }
                    });
                let chosen = current == path;
                let mark = match known {
                    Some(true) => " ✓",
                    Some(false) => " ·",
                    None => "",
                };
                // The label says what the place is; the path is in the hover.
                let button = egui::Button::new(format!("{}{mark}", place.label)).selected(chosen);
                if ui
                    .add_enabled(idle, button)
                    .on_hover_text(format!(
                        "{path}\n{}\n{}",
                        place.note,
                        match known {
                            Some(true) => "the target has this one",
                            Some(false) => "the target does not have this one",
                            None => "not asked about - an earlier one answered first",
                        }
                    ))
                    .clicked()
                {
                    go = Some(path.to_owned());
                }
            }
        });
        if let Some(path) = go {
            self.state.files.library_path = path;
            // Listed straight away, so a stale listing never sits under the new path.
            if connected {
                self.browse();
            }
        }
    }

    /// The right half: what is on the target.
    pub(super) fn there_side(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        self.there_toolbar(ui, idle, connected);
        // Return navigates, as on the local side.
        if entered(
            ui,
            egui::TextEdit::singleline(&mut self.state.files.library_path),
        ) {
            self.state.files.listing.chosen.clear();
            self.state.files.seen.clear();
            self.browse();
        }
        self.candidate_buttons(ui, idle, connected);
        self.locate_notice(ui);
        ui.separator();

        let section = self.state.section.name();
        let looking_at = self.state.files.library_path.clone();
        let rows: Vec<crate::listing::Entry> = self
            .state
            .files
            .listing
            .entries
            .iter()
            .filter(|entry| entry.there.is_some())
            .cloned()
            .collect();
        if rows.is_empty() {
            ui.weak("nothing listed here");
        }
        let Gestures {
            toggled,
            entered,
            going_up,
            folds,
        } = self.there_table(ui, section, &looking_at, &rows);
        if let Some(name) = toggled {
            self.state.files.listing.toggle(&name);
        }
        // The selection is cleared on navigation: a tick is a name, and a name means a
        // different file in another folder. That includes the folder the double click ticked.
        if let Some(name) = entered {
            self.state.files.library_path = format!(
                "{}/{name}",
                self.state.files.library_path.trim_end_matches('/')
            );
            self.state.files.listing.chosen.clear();
            self.browse();
        } else if going_up && let Some(above) = parent_of(&self.state.files.library_path) {
            self.state.files.library_path = above;
            self.state.files.listing.chosen.clear();
            self.browse();
        }
        fold(&mut self.state.files.folded, folds);
    }

    /// The target's listing, folders then files, under a row that goes up a level.
    fn there_table(
        &self,
        ui: &mut egui::Ui,
        section: &str,
        looking_at: &str,
        rows: &[crate::listing::Entry],
    ) -> Gestures {
        let mut asked = Gestures::default();
        egui::Grid::new(format!("{section}-there"))
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                headings(ui, &["", "name", "size"]);
                if up_row(ui, looking_at) {
                    asked.going_up = true;
                }
                let (folders, files): (Vec<_>, Vec<_>) =
                    rows.iter().partition(|entry| entry.folder_there());
                for (label, group) in [("folders", &folders), ("files", &files)] {
                    if group.is_empty() {
                        continue;
                    }
                    let key = format!("{section}-there-{label}");
                    if group_row(
                        ui,
                        &self.state.files.folded,
                        &key,
                        label,
                        group.len(),
                        &mut asked.folds,
                    ) {
                        continue;
                    }
                    for entry in group {
                        // The row reports which gesture happened, so a folder stays selectable.
                        let known = self.state.files.names.get(&entry.name);
                        match listing_row(ui, entry, &self.state.files.listing.chosen, true, known)
                        {
                            // Opens the directory by the target's name for it (`elfldr`), not
                            // the row's description-based name (`elfldr_v0.25.elf`); see
                            // `listing::Side`. The tick still keys off the row name.
                            Some(Hit::Open) => {
                                asked.entered =
                                    Some(entry.there.as_ref().map_or_else(
                                        || entry.name.clone(),
                                        |side| side.name.clone(),
                                    ));
                            }
                            Some(Hit::Tick) => asked.toggled = Some(entry.name.clone()),
                            None => {}
                        }
                    }
                }
            });
        asked
    }

    /// One list, with a column for each side.
    ///
    /// The model drawn plainly; the split panes are two filtered views of it.
    pub(super) fn merged_view(&mut self, ui: &mut egui::Ui) {
        let mut toggled = None;
        egui::ScrollArea::both()
            .id_salt("merged")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("merged-rows")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        headings(ui, &["", "name", "here", "there", "described"]);
                        ui.weak("");
                        ui.weak("name");
                        ui.weak("here");
                        ui.weak("target");
                        ui.weak("");
                        ui.end_row();

                        for entry in &self.state.files.listing.entries {
                            let mut ticked = self.state.files.listing.chosen.contains(&entry.name);
                            if ui.checkbox(&mut ticked, "").changed() {
                                toggled = Some(entry.name.clone());
                            }
                            ui.label(&entry.name);
                            side_cell(ui, entry.here.as_ref());
                            side_cell(ui, entry.there.as_ref());
                            let (word, colour) = standing_of(entry);
                            ui.colored_label(colour, word);
                            ui.end_row();
                        }
                    });
                if self.state.files.listing.entries.is_empty() {
                    ui.weak("nothing on either side, and nothing described");
                }
            });
        if let Some(name) = toggled {
            self.state.files.listing.toggle(&name);
        }
    }

    /// The notice about where a section's things live, when the target has none of them.
    fn locate_notice(&mut self, ui: &mut egui::Ui) {
        if let Some((asked, pros_core::locate::Where::NoneOfThem(tried))) =
            &self.state.files.located
            && *asked == self.state.section
        {
            ui.colored_label(
                egui::Color32::from_rgb(210, 190, 120),
                format!(
                    "the target has none of these, so nothing here handles {} yet:",
                    self.state.section.name()
                ),
            );
            for path in tried {
                ui.small(path);
            }
        }
    }

    /// Copies chosen files into the section's folder, so they appear in the list.
    ///
    /// Copied rather than referenced: the list is a listing of one folder.
    fn add_files(&mut self) {
        let Some(chosen) = choose_files(&self.state.files.local_path) else {
            return;
        };
        let into = PathBuf::from(self.state.files.local_path.trim());
        let mut refused = Vec::new();
        let mut taken = 0;
        for path in chosen {
            let Some(name) = path.file_name() else {
                continue;
            };
            match std::fs::create_dir_all(&into)
                .and_then(|()| std::fs::copy(&path, into.join(name)).map(|_| ()))
            {
                Ok(()) => taken += 1,
                Err(why) => refused.push(format!("{}: {why}", path.display())),
            }
        }
        if refused.is_empty() {
            self.state.said = format!("{taken} copied into {}", into.display());
        } else {
            self.state.trouble = Some(refused.join("; "));
        }
        self.read_local();
    }

    /// Shows a folder in the system's file browser.
    ///
    /// Not on the worker: it starts a program and returns, so it has nothing to wait for.
    pub(super) fn reveal(&mut self, path: &Path) {
        match pros_core::reveal::folder(path) {
            Ok(()) => self.state.said = path.display().to_string(),
            Err(why) => self.state.trouble = Some(why.to_string()),
        }
    }

    /// Lists the library path.
    pub(super) fn browse(&mut self) {
        let where_to = self.state.files.library_path.clone();
        // A path already read this session is not read again; the sections share one listing
        // slot. Cleared whenever a job reports it changed the target (`Disturbs::There`).
        if let Some(known) = self.state.files.seen.get(&where_to) {
            self.state.files.library = known.clone();
            return;
        }
        if let Some(target) = self.state.target().cloned() {
            self.state.begin(Job::Browse(target, where_to));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::state::Section;

    /// Every place has a label and a note, and no two in a section share a label.
    #[test]
    fn every_place_says_what_it_is_and_why() {
        for section in Section::GROUPS.iter().flat_map(|(_, sections)| *sections) {
            let places = section.candidates();
            for place in places {
                assert!(!place.label.is_empty(), "{} is unlabelled", place.path);
                assert!(!place.note.is_empty(), "{} says nothing", place.path);
            }
            let mut labels: Vec<&str> = places.iter().map(|place| place.label).collect();
            let all = labels.len();
            labels.sort_unstable();
            labels.dedup();
            assert_eq!(
                labels.len(),
                all,
                "{} offers two places under one name",
                section.name()
            );
        }
    }

    /// Every known cheat location has a button, since none is the standard.
    #[test]
    fn the_cheat_section_offers_every_place_cheats_are_kept() {
        let paths: Vec<&str> = Section::Cheats
            .candidates()
            .iter()
            .map(|place| place.path)
            .collect();
        assert_eq!(
            paths,
            [
                "/data/cheatrunner/cheats",
                "/data/etaHEN/cheats",
                "/data/elf-arsenal/cheats"
            ]
        );
    }

    /// A section with one measured path (`/user/app`, `/user/home`) offers no alternatives.
    #[test]
    fn a_section_with_a_measured_path_offers_no_alternatives() {
        assert!(Section::Titles.candidates().is_empty());
        assert!(Section::Saves.candidates().is_empty());
    }

    /// Both places packages were found on a target have a button, and the first is the start.
    #[test]
    fn packages_offer_both_places_they_were_found() {
        let places = Section::Packages.candidates();
        let paths: Vec<&str> = places.iter().map(|place| place.path).collect();
        assert_eq!(paths, ["/data/homebrew/pkg", "/data/pkg"]);
        assert_eq!(
            Section::Packages.there(),
            places[0].path,
            "the starting path is the first candidate, not a third answer"
        );
        // Two directories, not a link: measured with the target's own `file` (it uses
        // `lstat`).
        assert_eq!(places[0].label, "uploads");
        assert_eq!(places[1].label, "install staging");
    }

    /// Going up stops at the root rather than producing a path above it.
    #[test]
    fn the_way_up_runs_out_at_the_root() {
        assert_eq!(super::parent_of("/data/pkg").as_deref(), Some("/data"));
        assert_eq!(super::parent_of("/data").as_deref(), Some("/"));
        assert_eq!(super::parent_of("/"), None);
        assert_eq!(super::parent_of(""), None);
    }

    /// A trailing separator is not a level of its own.
    #[test]
    fn a_trailing_separator_does_not_add_a_step() {
        assert_eq!(super::parent_of("/data/pkg/").as_deref(), Some("/data"));
    }
}
