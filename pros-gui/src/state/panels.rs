//! What each panel holds between frames, grouped by the panel that owns it.

use std::path::PathBuf;

use super::Section;

/// A chain read off a target, being written down as a preset.
///
/// The name is the one thing a person types, so it is held apart and applied on agreement;
/// the panel can refuse a name with nothing to undo.
#[derive(Debug, Clone)]
pub(crate) struct Exporting {
    /// What to call it. One word, because a preset name goes in a whitespace-delimited file.
    pub(crate) name: String,
    /// The preset as measured, with the name not yet applied.
    pub(crate) preset: pros_core::recovery::baseline::Preset,
    /// What the export could not know, in its own words.
    pub(crate) notes: Vec<String>,
    /// Whether the files the chain should carry are still being read off the target.
    ///
    /// The panel opens with the list and fills the files in when the read lands; while this is
    /// set it will not write, so a chain never carries the list without its files.
    pub(crate) capturing: bool,
    /// How many disabled lines were left out.
    pub(crate) disabled: usize,
    /// Where it would be written.
    pub(crate) into: String,
    /// The presets that already exist, so a name that replaces one says so.
    pub(crate) taken: Vec<String>,
}

/// A plan, and the finding it answers, waiting for somebody to agree to it.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    /// Which finding this answers, so the result can be checked against it.
    pub(crate) id: String,
    /// What the finding was called, for the panel's heading.
    pub(crate) label: String,
    /// The steps.
    pub(crate) plan: pros_core::doctor::Plan,
}

/// The probe screen: what it can launch, and what the last run captured.
///
/// Only what is drawn; the run's thread lives beside the worker, like the log's.
#[derive(Debug, Default)]
pub(crate) struct Probing {
    /// What is installed, to choose from. `None` until asked.
    pub(crate) titles: Option<Vec<pros_core::titles::Metadata>>,
    /// Which target the titles were last asked of, so a refusal is not retried every frame.
    pub(crate) titles_for: Option<String>,
    /// Which title will be launched.
    pub(crate) id: Option<String>,
    /// How long a run follows the log before it stops on its own, in seconds.
    pub(crate) seconds: u64,
    /// What the last run captured - its steps, marked `--`, and the log lines between them.
    ///
    /// Kept apart from [`LogState::lines`] so the run's start and end stay visible.
    pub(crate) lines: Vec<String>,
    /// The step the run is on, or how it ended.
    pub(crate) status: String,
    /// What to keep on screen - the log screen's filter, separately held.
    pub(crate) filter: String,
    /// Whether the filter is a regular expression.
    pub(crate) regex: bool,
}

/// Which windows are open.
///
/// Grouped apart from the drawing loop's own notes on [`State`](super::State).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Showing {
    /// The register dialog.
    pub(crate) registering: bool,
    /// The about window.
    pub(crate) about: bool,
}

/// The doctor on the check screen: a plan waiting for agreement, and what carrying one out
/// leaves until its transfers land.
#[derive(Debug, Default)]
pub(crate) struct DoctorState {
    /// A description that now points at a newer release, waiting to be written to the list.
    pub(crate) relisted: Option<(pros_core::manifest::Payload, pros_core::sources::Upstream)>,
    /// Whole startup lists a plan has agreed to write, once its transfers have landed.
    ///
    /// Applied with [`Self::after_transfers`], because the entries name files the plan's sends
    /// put in place. More than one because a payload-manager chain has two lists: the
    /// autoloader's starts the manager, which then runs its own. Each is reviewed separately.
    pub(crate) rebuild: Vec<(String, Vec<String>)>,
    /// List edits a plan has agreed but that cannot be made until its transfers land.
    ///
    /// An entry may only name a file the manager can resolve on internal storage, and the
    /// plan's own send is what puts it there, so the edit waits for the send.
    pub(crate) after_transfers: Vec<pros_core::recovery::Fix>,
    /// A doctor's plan that has been shown to somebody and not yet agreed to.
    ///
    /// A plan reaches the queue only from here, behind a button: this program suggests and
    /// never acts on its own.
    pub(crate) pending_plan: Option<Pending>,
    /// Which finding a plan was carried out for, while it is still being carried out.
    ///
    /// Kept so the next check can confirm the finding is answered, not assume it.
    pub(crate) fixing: Option<String>,
}

/// The autoload screen: the startup lists, the manager's settings, and the configurator.
#[derive(Debug, Default)]
pub(crate) struct AutoloadState {
    /// Which chain the configurator would build, by name.
    ///
    /// A name, not an index, because the presets file can be edited between runs.
    pub(crate) preset: String,
    /// Which list the configurator is being pointed at, while somebody is choosing.
    ///
    /// `None` when it is not open. Separate from the list being viewed, so picking one does
    /// not move the view.
    pub(crate) setting_up: Option<usize>,
    /// The startup list, once read, with any edits not yet written.
    pub(crate) boot: Option<pros_core::boot::Boot>,
    /// Which row of it is selected.
    ///
    /// One row: the actions move a single step.
    pub(crate) boot_at: Option<usize>,
    /// The payload manager's settings, once read.
    pub(crate) settings: Option<pros_core::autoload::Settings>,
    /// An edit to those settings that has not been written.
    pub(crate) pending_change: Option<pros_core::autoload::Change>,
    /// Every startup list the loaded chains declare.
    ///
    /// Read once at startup, not per frame; a chain added while open is seen on next start.
    pub(crate) lists: Vec<pros_core::chain::Held>,
    /// Which startup list the autoload screen is showing, into [`Self::lists`].
    pub(crate) list_at: usize,
    /// A chain read off a target, waiting to be written down as a preset.
    ///
    /// Built once when the button is pressed, since building reads the presets file.
    pub(crate) exporting: Option<Exporting>,
}

/// The two-sided storage sections: both listings, where each is looking, and what is waiting
/// to be confirmed.
#[derive(Debug, Default)]
pub(crate) struct FilesState {
    /// The target's listing of wherever the browser is looking.
    pub(crate) library: Vec<pros_core::library::Item>,
    /// Listings already fetched this session, by the path they are of.
    ///
    /// Every two-sided section browses into the one `library`, so this saves re-fetching when
    /// moving between them. Emptied whenever a job disturbs the target.
    pub(crate) seen: std::collections::BTreeMap<String, Vec<pros_core::library::Item>>,
    /// This machine's listing of the section's folder.
    pub(crate) local: Vec<pros_core::library::Item>,
    /// Where the browser is looking on the target.
    pub(crate) library_path: String,
    /// Which section that path was settled for.
    pub(crate) library_place: Option<Section>,
    /// Where the browser is looking on this machine.
    pub(crate) local_path: String,
    /// Somewhere the target said to go, once it had been asked.
    pub(crate) go_to: Option<String>,
    /// Title names, by identifier, once the target has said.
    pub(crate) names: std::collections::BTreeMap<String, String>,
    /// What the target said about where this section's things live, and which section asked,
    /// so the answer is not shown in another section.
    pub(crate) located: Option<(Section, pros_core::locate::Where)>,
    /// The two sides as one list, with what is ticked in it.
    pub(crate) listing: crate::listing::Listing,
    /// Whether to draw that list as one table rather than two panes.
    pub(crate) merged: bool,
    /// Groups the person has folded away in the payloads table.
    ///
    /// Records what is folded, so a new group starts open.
    pub(crate) folded: std::collections::BTreeSet<String>,
    /// A file somebody dropped that nothing describes.
    ///
    /// Held rather than refused: something just built has no publisher or digest to describe.
    pub(crate) adhoc: Option<PathBuf>,
    /// A destructive action waiting to be confirmed, and what it would act on.
    ///
    /// Not undoable, so it goes through a panel naming each thing that would go.
    pub(crate) pending_delete: Option<(crate::listing::Offer, Vec<crate::listing::Entry>)>,
    /// Packages on this machine waiting for somebody to confirm installing them.
    ///
    /// A list, because the toolbar selects several.
    pub(crate) pending_install: Option<Vec<PathBuf>>,
    /// A copy that was not attempted, and what it would need.
    pub(crate) refused: Option<pros_core::origin::Needs>,
    /// A title transfer refused for an inert destination path or an incompatible prefix.
    pub(crate) guard_refusal: Option<pros_core::guard::Refusal>,
}

/// The payloads screen: what the target holds, and where an install puts it.
#[derive(Debug, Default)]
pub(crate) struct PayloadsState {
    /// Where a payload is installed to on the target.
    ///
    /// Conventional and unmeasured, so it is an editable box rather than a constant.
    pub(crate) install_dir: String,
    /// Every payload file on the target, once looked for.
    ///
    /// `None` until asked, which is not none found; otherwise every startup entry would read
    /// as missing.
    pub(crate) there: Option<Vec<pros_core::payloads::There>>,
}

/// The log screen: what has arrived, and what the view keeps of it.
#[derive(Debug, Default)]
pub(crate) struct LogState {
    /// The log, as it arrives.
    pub(crate) lines: Vec<String>,
    /// What to keep on the log screen, if anything.
    ///
    /// Filters the view, never the record: clearing it shows every line again.
    pub(crate) filter: String,
    /// Whether the log filter box is read as a regular expression rather than plain text.
    ///
    /// Off by default; plain substring is the usual case.
    pub(crate) regex: bool,
    /// Which target the log was last started for, so a refusal is not retried every frame.
    pub(crate) followed_for: Option<String>,
}

/// The controllers screen: the pad slots, their bindings, and where their records go.
#[derive(Debug, Default)]
pub(crate) struct ControllersState {
    /// The four pad slots, what drives each, and the key layout they share.
    pub(crate) pads: pros_link::pads::Pads,
    /// How many pad records have been built this session.
    ///
    /// A count that climbs while keys are pressed confirms the mapping without a receiver.
    pub(crate) pad_records: u64,
    /// Where controller records go, when anywhere.
    pub(crate) feed: pros_link::feed::Feed,
    /// The port the input payload is expected on.
    ///
    /// Editable: both ends are ours and the number is chosen, not measured.
    pub(crate) feed_port: String,
    /// Which slot the pending binding belongs to.
    ///
    /// Held so the binding lands on the slot it was started for, not the one on screen.
    pub(crate) binding_slot: Option<u8>,
    /// Which button is waiting to be bound to the next key pressed.
    ///
    /// Held here so it survives between frames.
    pub(crate) binding: Option<pros_link::pad::Button>,
}

/// The stream screen: the video coming back from the target.
#[derive(Debug, Default)]
pub(crate) struct StreamState {
    /// The stream coming back the other way, when one is.
    ///
    /// Owned here, not by the panel, so switching sections does not end it.
    pub(crate) watching: pros_core::watch::Watching,
    /// The port the video payload is expected on. Editable, for the same reason as the feed's.
    pub(crate) watch_port: String,
}

/// The register dialog: what a person has typed into it.
#[derive(Debug, Default)]
pub(crate) struct RegisterState {
    /// What a person has typed into the address box.
    pub(crate) address: String,
    /// What a person has typed into the name box.
    pub(crate) name: String,
    /// The target whose address is being edited, when the register dialog is in edit mode.
    ///
    /// `None` is a fresh registration; `Some(name)` re-registers that name with a new address,
    /// keeping its ports and chain.
    pub(crate) editing: Option<String>,
}
