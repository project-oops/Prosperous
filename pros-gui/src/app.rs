//! Drawing, and nothing else.
//!
//! # What is not here
//!
//! No decision. What a registration is, what a missing loader means, which files a loader
//! will accept, whether an answer took long enough to remark on - all of it is in a crate
//! below this one and all of it is reachable from `pros` too. This module reads state and
//! draws it.
//!
//! The rules about *changing* that state live in [`crate::state`], where they can be tested,
//! because a window cannot be looked at from a test and on the machine this was written on
//! it cannot be looked at at all.
//!
//! # Why immediate mode
//!
//! A check is a table that is replaced wholesale every time it is run, not a form that is
//! edited field by field. Immediate mode draws from current state each frame, which is
//! exactly that shape. The same reasoning as the sibling project's shell, and the same
//! version of the same library, so that a shared crate later is a move rather than a port.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pros_core::boot::Step as BootStep;
use pros_core::check::{Remedy, Verdict};
use pros_core::library::Kind as LibraryKind;
use pros_core::manifest::Manifest;
use pros_core::payloads::{Boot, Presence, Standing, There, Trust};
use pros_core::target;

use crate::state::{Job, Section, State};
use crate::work::Worker;

/// One row of a listing: a tick, a name, and what that side knows about it.
///
/// Returns `true` when the row was clicked. **No action buttons.** What can be done depends on
/// everything that is selected, not on one row, so it lives in the toolbar - which also stops
/// the same three buttons being drawn fifty times.
fn listing_row(
    ui: &mut egui::Ui,
    entry: &crate::listing::Entry,
    chosen: &std::collections::BTreeSet<String>,
    target_side: bool,
    known: Option<&String>,
) -> Option<Hit> {
    let mut hit = None;
    let mut ticked = chosen.contains(&entry.name);
    if ui.checkbox(&mut ticked, "").changed() {
        hit = Some(Hit::Tick);
    }

    let side = if target_side { entry.there } else { entry.here };
    let folder = target_side && entry.folder_there();
    if folder {
        // **Opened on a double click, like every file browser.** A single click used to open
        // it, which made selecting a folder impossible - the tick and the navigation were the
        // same gesture, and navigation won.
        let name = ui.add(
            egui::Label::new(&entry.name)
                .sense(egui::Sense::click())
                .halign(egui::Align::LEFT),
        );
        hit = hit_of(name.double_clicked(), name.clicked()).or(hit);
        name.on_hover_text("double-click to open");
    } else if side.is_some() {
        ui.label(&entry.name);
    } else {
        // Described and not here: dimmed, because the group it sits under already says so.
        ui.weak(&entry.name);
    }

    // The name the target gave, beside the identifier - not instead of it, because the
    // identifier is what every path and every other tool uses.
    if let Some(name) = known {
        ui.strong(name);
    } else {
        ui.weak(
            side.and_then(|side| side.size)
                .map_or_else(String::new, size),
        );
    }
    ui.end_row();
    hit
}

/// A row that goes up a directory, when there is one above.
///
/// **Not a listing entry.** The server does not send `..` - it is filtered out on the way in,
/// because a walk that followed it climbs out of the directory somebody asked about. This is
/// the navigation, put back where a person expects it and nowhere near the copying.
///
/// Returns `true` when it was used.
fn up_row(ui: &mut egui::Ui, path: &str) -> bool {
    if parent_of(path).is_none() {
        return false;
    }
    ui.label("");
    let up = ui.add(
        egui::Label::new("..")
            .sense(egui::Sense::click())
            .halign(egui::Align::LEFT),
    );
    ui.weak("up one");
    ui.end_row();
    // One click, because going up is not a selection and there is nothing here to tick. The
    // double-click case is covered: in egui a double click is also a click.
    up.clicked()
}

/// The directory above this one, when it is not already the root.
fn parent_of(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return None;
    }
    match trimmed.rfind('/') {
        // A path one level down goes to the root, which is `/` rather than the empty string.
        Some(0) => Some("/".to_owned()),
        Some(at) => Some(trimmed[..at].to_owned()),
        None => None,
    }
}

/// What a click on a folder's name means.
///
/// **Its own function so it can be tested**, because on screen the failure was invisible: the
/// row highlighted, the tick moved, and the folder did not open. Nothing about that says which
/// of the two gestures won.
///
/// The order is the whole content. **In egui a double click is a click** -
/// `double_clicked()` is defined as `clicked && is_double`, so both arrive on the same frame.
/// Written as two separate `if`s, which it was, the second overwrites the first and a folder
/// can never be opened at all.
const fn hit_of(double_clicked: bool, clicked: bool) -> Option<Hit> {
    if double_clicked {
        Some(Hit::Open)
    } else if clicked {
        // Single click selects, which is what a single click does everywhere else here.
        Some(Hit::Tick)
    } else {
        None
    }
}

/// What a row was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    /// Select it, or stop selecting it.
    Tick,
    /// Look inside it.
    Open,
}

/// What one side knows, for the merged view's columns.
fn side_cell(ui: &mut egui::Ui, side: Option<crate::listing::Side>) {
    match side {
        Some(crate::listing::Side { folder: true, .. }) => {
            ui.weak("dir");
        }
        Some(crate::listing::Side { size: bytes, .. }) => {
            ui.label(bytes.map_or_else(|| "yes".to_owned(), size));
        }
        // **Empty rather than a dash or a cross.** The column beside it is full or empty, and
        // that contrast is the whole information; a symbol in the gap competes with it.
        None => {
            ui.label("");
        }
    }
}

/// A word for which sides an entry is on, and a colour for it.
fn standing_of(entry: &crate::listing::Entry) -> (&'static str, egui::Color32) {
    match entry.standing() {
        crate::listing::Standing::Both => ("both", egui::Color32::from_rgb(120, 190, 120)),
        crate::listing::Standing::OnlyHere => ("here only", egui::Color32::from_rgb(140, 180, 220)),
        crate::listing::Standing::OnlyThere => {
            ("target only", egui::Color32::from_rgb(210, 190, 120))
        }
        crate::listing::Standing::Described => ("described", egui::Color32::GRAY),
    }
}

/// What pressing something on one of the doctor's rows asked for.
enum Asked {
    /// Show this plan, for somebody to agree to or not.
    Plan(crate::state::Pending),
    /// One of several routes was picked, so build that one's plan.
    Chose(String, String),
}

/// What one row offers, given what its check found.
///
/// **Its own function because the answer differs per verdict**, and a row that offered the
/// same button whatever it found would be offering the button as decoration.
fn doctor_action(
    ui: &mut egui::Ui,
    finding: &pros_core::doctor::Finding,
    idle: bool,
) -> Option<Asked> {
    use pros_core::doctor::{Remedy, Verdict};

    let mut picked: Option<String> = None;
    match &finding.verdict {
        Verdict::Unwell {
            remedy: Remedy::Ready(plan),
            ..
        } if !plan.is_settled() => {
            if ui
                .add_enabled(idle, egui::Button::new("fix...").min_size(FIX))
                .on_hover_text("show every step that would take - nothing happens yet")
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                return Some(Asked::Plan(crate::state::Pending {
                    id: finding.id.clone(),
                    label: finding.label.clone(),
                    plan: plan.clone(),
                }));
            }
        }
        // **Several would do, so this asks rather than picks.** Choosing how a
        // target boots on somebody's behalf is the one decision this will not
        // make for them, however obvious it looks from here.
        Verdict::Unwell {
            remedy: Remedy::Choose { between, why },
            ..
        } => {
            ui.menu_button("choose...", |ui| {
                ui.weak(why.as_str());
                ui.separator();
                for (name, unlocks) in between {
                    if ui
                        .button(name.as_str())
                        .on_hover_text(unlocks.as_str())
                        .clicked()
                    {
                        picked = Some(name.clone());
                        ui.close_menu();
                    }
                }
            });
        }
        Verdict::Unwell {
            remedy: Remedy::Beyond(said),
            ..
        } => {
            ui.weak("nothing from here").on_hover_text(said.as_str());
        }
        // **Every step already done, and it still fails.** Said rather than
        // drawn as an offer: a fix button that would do nothing at all is a
        // button that spends somebody's attention to achieve nothing.
        Verdict::Unwell {
            remedy: Remedy::Ready(_),
            ..
        } => {
            ui.weak("already done").on_hover_text(
                "every step of this remedy is already the case and the check still \
                 fails, so this is not what is wrong",
            );
        }
        Verdict::Well(_) | Verdict::Unknown(_) | Verdict::Aside(_) => {
            ui.label("");
        }
    }
    picked.map(|name| Asked::Chose(finding.id.clone(), name))
}

/// Everything one row of the payload table draws itself from.
///
/// **A struct because it was nine arguments.** They are one thing - the state a row is drawn
/// against - and passing them separately meant every new column widened a signature that
/// nobody could read at the call site anyway.
struct Shown<'a> {
    /// The rows in this group.
    rows: &'a [&'a pros_core::payloads::Row<'a>],
    /// What the target holds.
    on_target: &'a [There],
    /// What each project has released, as far as anything has asked.
    sources: &'a pros_core::sources::Sources,
    /// What is ticked.
    chosen: &'a std::collections::BTreeSet<String>,
    /// What this machine holds, so a row knows whether it has anything to send.
    here: &'a [pros_core::library::Item],
    /// Whether anything can be started right now.
    idle: bool,
    /// Whether there is a target to send to.
    connected: bool,
}

/// Whether this machine holds a file of that name, ready to be sent.
///
/// Read from the listing already taken for the section rather than by asking the filesystem:
/// this is called once per row per frame, and a `stat` per row per frame to answer a question
/// that changes when a download lands is machinery around something already known.
fn what_is_here(here: &[pros_core::library::Item], file: &str) -> bool {
    here.iter()
        .any(|item| item.name.eq_ignore_ascii_case(file) && item.kind != pros_core::library::Kind::Folder)
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
    /// A row to send to the loader and start, by filename.
    run: Option<String>,
}

/// The word and colour for one finding, from its verdict and how much it matters.
///
/// **Four words, not two.** A check nobody could run and a check that passed are drawn
/// differently on purpose: showing the two the same is how an unreachable target comes to look
/// like a healthy one.
fn mark_of(
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

/// What to say after a plan has been handed to the queue.
///
/// **Counts what was started, not what worked.** Nothing has finished at the point this is
/// written, and a sentence claiming otherwise would be the same lie the whole plan exists to
/// stop telling - the checking happens at the end, and says so separately.
fn summarise(queued: usize, edited: usize, could_not: &[String]) -> String {
    let mut said = match (queued, edited) {
        (0, 0) => "nothing to do".to_owned(),
        (0, _) => format!("{edited} list edits ready - review and save them"),
        (_, 0) => format!("{queued} steps started"),
        // **Not *ready*.** They are waiting on the transfers above them, and calling them ready
        // would have somebody go looking for a review panel that is not there yet.
        _ => format!(
            "{queued} steps started - the {edited} list edits follow once those land, and open              for review"
        ),
    };
    if !could_not.is_empty() {
        said.push_str(" - not done: ");
        said.push_str(&could_not.join(", "));
    }
    said
}

/// One width for the check screen's fix buttons, so the notes beside them line up.
const FIX: egui::Vec2 = egui::vec2(74.0, 0.0);

/// One change to the startup list, held until the caller can apply it.
///
/// A boxed closure rather than an enum of the four operations: what they have in common is
/// that they are applied later, and naming them twice - once as a variant and once as the call
/// it makes - is two lists to keep in step.
type Edit = Box<dyn FnOnce(&mut pros_core::boot::Boot) -> bool>;

/// The payload at a position in the list as it was before an edit.
fn boot_name(boot: Option<&pros_core::boot::Boot>, at: usize) -> Option<&String> {
    boot?.steps.get(at).map(|step| &step.payload)
}

/// The top of a section: its name, what it is for, and a rule under both.
///
/// **One function because there are fourteen places that drew a heading**, several of them two
/// branches of the same screen, and a heading that says something on one branch and not the
/// other is the kind of difference nobody sees until they are looking for it.
fn section_heading(ui: &mut egui::Ui, section: Section) {
    section_heading_with(ui, section, |_| ());
}

/// The same top, with controls beside the name.
///
/// Three screens put a button or a field next to their heading. Left as they were, those three
/// would be the ones without an explanation under it - which is exactly the inconsistency this
/// was written to remove, arrived at by accident instead of on purpose.
fn section_heading_with(ui: &mut egui::Ui, section: Section, controls: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.heading(section.name());
        controls(ui);
    });
    ui.weak(section.explains());
    ui.separator();
}

/// What part a service plays in a chain, from the catalogue own flags.
///
/// **Derived, not written per payload.** Each of these is a property the catalogue already
/// carries and a rule this project already enforces, so a service declared in `services.json`
/// gets the same explanation as one that was compiled in - which is the point of it being
/// config at all.
///
/// `None` for a payload with no role: it is in the list because somebody put it there, and
/// saying why is theirs to do.
fn role_of(service: &pros_link::service::Service) -> Option<String> {
    let mut parts = Vec::new();
    if service.name == pros_link::service::LOADER.name {
        // Measured from the manager source: it sends every entry to the loader, so nothing
        // after this point loads if this is not already up.
        parts.push(
            "everything else here is loaded through it, so it has to be running before them"
                .to_owned(),
        );
    }
    if service.runs_lists {
        parts.push("runs the startup list itself".to_owned());
    }
    if service.required {
        parts.push("required - there is no workflow without it".to_owned());
    }
    if service.recovers {
        parts.push("a way back in if the rest of the chain fails".to_owned());
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// The `why` cell: why this entry is in this chain, and how to say so.
///
/// **Only ever a note somebody wrote.** The column beside it says *what* a payload is, from
/// whoever published it. This one says why it is in this list at this point, which is
/// knowledge about one setup that nothing on the target records.
///
/// Falling back from one to the other - which this did - reads as an answer to a question
/// nobody asked: *Lite version of kstuff* is a fine WHAT and says nothing about why that
/// entry loads first. So an empty cell here means **nobody has said why**, and it says where
/// to say it.
fn reason(ui: &mut egui::Ui, known: Option<String>) {
    match known {
        Some(text) => {
            ui.weak(text);
        }
        // **A dash, and nothing to click.** Where a reason belongs is `data/chain.json` in
        // this repository, beside every other fact about a payload - not typed into a box in
        // a window, where it would live on one machine and be lost with it.
        None => {
            ui.weak("-")
                .on_hover_text("no ordering requirement recorded for this payload");
        }
    }
}

/// The title row of a listing grid.
///
/// **A column with no name is a column somebody has to decode.** Two of these tables carried a
/// size, a presence mark and a name with nothing saying which was which, and the merged view
/// had five such columns.
///
/// Drawn as a row of the grid rather than above it, for the same reason the group headings are:
/// anything outside the grid does not share its column widths, so a heading placed above would
/// drift out of line with the thing it names the moment a name got longer.
fn headings(ui: &mut egui::Ui, titles: &[&str]) {
    for title in titles {
        ui.small(egui::RichText::new(*title).weak());
    }
    ui.end_row();
}

/// A group heading inside a listing grid, and whether its rows are hidden.
///
/// **The same shape in every listing, because they are the same kind of thing.** A heading is
/// a row in the grid rather than a widget wrapped around it, which is what keeps the columns
/// underneath one group lined up with the columns underneath the next - the thing a details
/// view has and a stack of separate tables does not.
///
/// Returns `true` when the group is folded and its rows should be skipped. The set records
/// what is **folded**, not what is open, so a group that appears later - a category that grows,
/// a kind of file that turns up - starts open like every other one rather than hidden.
fn group_row(
    ui: &mut egui::Ui,
    folded: &std::collections::BTreeSet<String>,
    key: &str,
    label: &str,
    count: usize,
    toggled: &mut Option<String>,
) -> bool {
    let shut = folded.contains(key);
    let arrow = if shut { ">" } else { "v" };
    if ui
        .selectable_label(false, format!("{arrow} {label}  ({count})"))
        .clicked()
    {
        *toggled = Some(key.to_owned());
    }
    ui.end_row();
    shut
}

/// Applies a heading that was clicked.
fn fold(folded: &mut std::collections::BTreeSet<String>, toggled: Option<String>) {
    if let Some(key) = toggled
        && !folded.remove(&key)
    {
        folded.insert(key);
    }
}

/// Draws two panes side by side, each exactly half, each scrolling its own content.
///
/// # Why this is a function and not two `set_width` calls
///
/// It was two `set_width` calls, and they did not hold. A pane is only as narrow as what is
/// inside it: the payloads table is wider than half a window, so it took the width it wanted
/// and pushed the target pane off the right-hand edge entirely.
///
/// **Setting a width is a request; clipping and scrolling is what makes it true.** Each half
/// gets a hard maximum and its content scrolls in both directions inside that, so a long list
/// scrolls down and a wide row scrolls across rather than either of them deciding the layout
/// for the other pane.
///
/// Half of what is actually available, so it follows the window rather than a number that was
/// right at one size.
fn pane(ui: &mut egui::Ui, salt: &str, size: egui::Vec2, body: impl FnOnce(&mut egui::Ui)) {
    // **Top-down, said explicitly.** These panes sit inside a horizontal layout, and a plain
    // `allocate_ui` inherits its parent's direction - so everything inside flowed left to
    // right, and the payload groups ended up side by side across the pane instead of stacked.
    ui.allocate_ui_with_layout(size, egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_max_width(size.x);
        egui::ScrollArea::both()
            .id_salt(salt)
            .auto_shrink([false, false])
            .show(ui, body);
    });
}

/// How big each of two side-by-side panes should be.
///
/// The separator and its padding come out of the total before it is halved, so the two are
/// equal rather than the left being wider by the gap. **Half of what is actually available**,
/// so it follows the window instead of a number that was right at one size.
///
/// The height is what is left, which is what gives the scroll area something to scroll
/// against - an unbounded one grows to fit its content and never scrolls at all.
fn half_of(ui: &egui::Ui) -> egui::Vec2 {
    egui::vec2(
        ((ui.available_width() - GAP) * 0.5).max(120.0),
        ui.available_height().max(120.0),
    )
}

/// The same, split where somebody dragged it to.
///
/// `share` is the left pane's fraction of the usable width, so the split **survives the window
/// being resized** - a split remembered in pixels creeps towards one edge every time somebody
/// makes the window smaller, and eventually the pane it was protecting is the one that
/// disappears.
fn split_at(ui: &egui::Ui, share: f32) -> (egui::Vec2, egui::Vec2) {
    let usable = (ui.available_width() - GAP - HANDLE).max(240.0);
    let height = ui.available_height().max(120.0);
    let left = (usable * share).clamp(120.0, usable - 120.0);
    (egui::vec2(left, height), egui::vec2(usable - left, height))
}

/// How wide the draggable divider is.
///
/// **Wider than the line it draws.** A one-pixel target is one nobody can hit; this is the
/// grabbable area, and the rule inside it is drawn thinner.
const HANDLE: f32 = 8.0;

/// Draws the divider and reports how far it was dragged, in pixels.
///
/// Returns `0.0` when it was not touched, so a caller can add unconditionally.
fn splitter(ui: &mut egui::Ui, height: f32) -> f32 {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(HANDLE, height), egui::Sense::drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let colour = if response.dragged() {
        ui.visuals().widgets.active.bg_fill
    } else if response.hovered() {
        ui.visuals().widgets.hovered.bg_fill
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    let line = egui::Rect::from_center_size(rect.center(), egui::vec2(2.0, height));
    ui.painter().rect_filled(line, 1.0, colour);
    response.drag_delta().x
}

/// Asks for one file, starting where the pane is looking.
///
/// **In the window crate rather than the core**, because a modal dialog belongs to a window
/// and the crates below this one have none - the command line reaches the same code by being
/// given a path instead.
///
/// `None` when somebody closed it without choosing, which is an answer and not a failure.
fn choose_a_file(what: &str, kinds: &[&str], from: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(what)
        .add_filter(what, kinds)
        .set_directory(from.trim())
        .pick_file()
}

/// Asks for any number of files, of any kind.
fn choose_files(from: &str) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_title("files to copy in")
        .set_directory(from.trim())
        .pick_files()
}

/// Where the manager keeps the payload files it loads.
///
/// Measured: one folder per payload, with the file inside it.
const PAYLOADS: &str = "/data/pldmgr/payloads";

/// What the separator between two panes costs, with its padding.
const GAP: f32 = 24.0;

/// The window.
pub(crate) struct App {
    /// The documentation reader. Holds which page is open and the parsed form of the ones
    /// already looked at, so the markdown is not re-parsed on every frame.
    docs: oops_docs::DocsWindow,
    state: State,
    worker: Worker,
    stamp: String,
    /// What is described, when a manifest has been read.
    manifest: Option<Manifest>,
    /// Which services exist and what each is for - defaults, then this project own file.
    ///
    /// **Read once, at start.** It says what a service means rather than what a target is
    /// doing, so unlike a capability it does not expire on a power cycle.
    catalogue: pros_core::catalogue::Catalogue,
    /// Where the manifest came from, so the browser can say.
    manifest_from: String,
    /// The log being followed, when one is.
    ///
    /// **Beside the worker rather than inside it.** The worker runs one job at a time, which
    /// is right for requests and wrong for a subscription: a log being watched would block
    /// every other thing the window can do, including sending the payload whose failure
    /// somebody is reading about.
    tail: Option<crate::tail::Tail>,
    /// What each payload's own project has released, as far as anything has asked.
    ///
    /// **Read from disk at start and written back as answers arrive.** The point of keeping
    /// it is not to ask again on the next launch; held only in memory it would ask every time
    /// and be rate-limited by the third one.
    sources: pros_core::sources::Sources,
    /// Whether the sweep that runs on its own has been started.
    ///
    /// **A flag rather than doing it in `new`.** Asking thirty projects is spaced out on
    /// purpose, and a window that did it before its first frame would look like one that will
    /// not open.
    asked_at_launch: bool,
    /// A sweep of those projects, while one is running.
    ///
    /// Beside the worker for the same reason the log is: it is slow **on purpose** - spaced
    /// out and waiting out refusals - and a queue that runs one thing at a time would be
    /// entirely blocked for as long as it took, including at launch.
    sweep: Option<crate::sweep::Sweep>,
}

impl App {
    /// Opens with whatever is registered on this machine.
    #[must_use]
    pub(crate) fn new() -> Self {
        // A registry that cannot be read is not a reason to refuse to start: the window can
        // still register one, which is the thing a person would do about it anyway.
        let targets = target::load().unwrap_or_default();
        // A manifest in the usual place if there is one, and the built-in recommended list
        // if there is not. **Falling back rather than showing nothing**: somebody who has
        // just installed this wants to know what a target ought to be running, and an
        // empty window tells them to already know the answer.
        let (manifest, manifest_from) = pros_core::manifest::default_path()
            .and_then(|path| {
                Manifest::from_file(&path)
                    .ok()
                    .map(|manifest| (Some(manifest), path.display().to_string()))
            })
            .unwrap_or_else(|| {
                // **Written out on first run, then read from disk like anything else.**
                // A list compiled into the binary cannot be corrected without a rebuild,
                // and this one is a description somebody should be able to edit.
                let seed = pros_core::manifest::recommended();
                let where_from = seed.save().map_or_else(
                    |_| "the built-in list, which could not be written out".to_owned(),
                    |path| path.display().to_string(),
                );
                (Some(seed), where_from)
            });

        Self {
            state: State::new(targets),
            worker: Worker::new(),
            stamp: pros_core::build::line(),
            docs: oops_docs::DocsWindow::default(),
            tail: None,
            sweep: None,
            asked_at_launch: false,
            sources: pros_core::sources::load(),
            manifest,
            // Defaults when no file exists, which is the normal case - see `pros_core::catalogue`.
            catalogue: pros_core::catalogue::load()
                .unwrap_or_else(|_| pros_core::catalogue::Catalogue::builtin()),
            manifest_from,
        }
    }

    /// Takes in whatever the sweep has answered, and keeps it.
    ///
    /// **Written to disk as they arrive, not at the end.** A sweep that is interrupted - the
    /// window closed, a rate limit reached, the machine put to sleep - has still learnt what it
    /// learnt, and throwing that away would mean the next launch asks all of it again.
    fn take_sweep_answers(&mut self) {
        let Some(sweep) = self.sweep.as_mut() else {
            return;
        };
        let arrived = sweep.drain();
        let ended = sweep.has_ended();
        if !arrived.is_empty() {
            for answer in arrived {
                self.sources.put(&answer.name, answer.found);
            }
            // Not trouble: the answers are in hand and usable this run - only keeping them
            // for the next one failed, and saying so in the status bar would put a message
            // about a cache in front of somebody who asked about payloads.
            let _ = pros_core::sources::save(&self.sources);
        }
        if ended {
            self.sweep = None;
        }
    }

    /// Starts asking the projects that have not been asked recently.
    ///
    /// `forced` ignores how fresh the stored answers are - the button - where the sweep at
    /// launch only asks about what has gone stale, so starting the program twice in a morning
    /// costs one request rather than sixty-eight.
    fn check_sources(&mut self, forced: bool) {
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
            // **Said, because nothing visible would otherwise happen.** A button that does
            // nothing and reports nothing is a button somebody presses twice.
            self.state.said = if wanted == 0 {
                "every project with a release page was asked recently - nothing to ask".to_owned()
            } else {
                "nothing to ask".to_owned()
            };
        }
    }

    /// The menu bar.
    ///
    /// **Every control is disabled rather than hidden when it does not apply, and says why
    /// on hover.** A control that vanishes reads as a bug; a greyed one reads as a state.
    /// The same rule the sibling project's shell holds, and worth holding for the same
    /// reason: a person looking for a button that is not there cannot tell whether they are
    /// wrong about the tool or the tool is wrong.
    fn menu_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("file", |ui| {
                    // **Porthole's player, not a remote-play client.** This wrote the
                    // example for somebody else's client, and that whole route is gone: this
                    // project serves its own stream, so the only command it needs configured
                    // is the one the stream is piped into.
                    if ui.button("configure the player...").clicked() {
                        self.write_player_example();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                // Everything about *which machine* lives here rather than on the main form.
                ui.menu_button("target", |ui| {
                    if ui.button("register...").clicked() {
                        self.state.showing.registering = true;
                        ui.close_menu();
                    }
                    let chosen = self.state.target().cloned();
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
                ui.menu_button("payloads", |ui| {
                    if ui.button("read manifest").clicked() {
                        self.read_manifest();
                        ui.close_menu();
                    }
                    // **Moved here from the payloads screen.** It acts on the list rather
                    // than on what is displayed, which is what this menu is for - and having
                    // it on the screen made payloads look like a different kind of section
                    // from the ones that quietly read a list from disk.
                    let can = self.state.target().is_some() && self.state.is_idle();
                    if ui
                        .add_enabled(can, egui::Button::new("merge the target's repository"))
                        .on_hover_text("its entries carry urls and digests, so downloads verify")
                        .on_disabled_hover_text("select a target, and wait for what is running")
                        .clicked()
                    {
                        if let Some(target) = self.state.target().cloned() {
                            // The path is typed rather than assumed - a good guess is still a
                            // guess, and a default would carry the authority of a measurement
                            // nobody has made. (D007)
                            let path = self.state.repository.clone();
                            self.state.begin(Job::ReadManifest(target, path));
                        }
                        ui.close_menu();
                    }
                    ui.horizontal(|ui| {
                        ui.small("from:");
                        ui.text_edit_singleline(&mut self.state.repository);
                    });
                    ui.separator();
                    if ui.button("write the recommended list").clicked() {
                        self.write_recommended();
                        ui.close_menu();
                    }
                    if !self.manifest_from.is_empty() {
                        ui.separator();
                        ui.small(&self.manifest_from);
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
    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.state.showing.about;
        egui::Window::new("about prosperous")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.heading("Prosperous");
                ui.label("one instrument for talking to a prepared target");
                ui.add_space(6.0);
                // The stamp, because a screenshot and a working copy are two claims about
                // the same software and there is otherwise no way to tell whether they agree.
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

    /// The registration dialog.
    ///
    /// A window rather than a panel, because it is a thing somebody does once. **A
    /// registration is a name and an address and nothing else**, so this form cannot grow a
    /// third field without somebody first changing what a registration means.
    fn register_dialog(&mut self, ctx: &egui::Context) {
        let mut open = self.state.showing.registering;
        egui::Window::new("register a target")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("name");
                    ui.text_edit_singleline(&mut self.state.name);
                });
                ui.horizontal(|ui| {
                    ui.label("address");
                    ui.text_edit_singleline(&mut self.state.address);
                });
                ui.small("an address and a name. What it can do is asked every time,");
                ui.small("because a jailbreak does not survive a power cycle");
                ui.separator();
                let can =
                    !self.state.name.trim().is_empty() && !self.state.address.trim().is_empty();
                if ui
                    .add_enabled(can, egui::Button::new("register"))
                    .on_hover_text("remember this target under that name")
                    .on_disabled_hover_text("a name and an address are both needed")
                    .clicked()
                {
                    match target::register(self.state.name.trim(), self.state.address.trim()) {
                        Ok(_) => {
                            self.state.targets = target::load().unwrap_or_default();
                            self.state.chosen = (!self.state.targets.is_empty()).then_some(0);
                            self.state.address.clear();
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
    }

    /// Writes the built-in list where a person can edit it.
    fn write_recommended(&mut self) {
        let Some(path) = pros_core::manifest::default_path() else {
            self.state.trouble = Some("no home directory, so there is nowhere for it".to_owned());
            return;
        };
        if path.exists() {
            // Refused rather than overwritten: what it holds that the built-in list does
            // not is exactly the part somebody had to find out.
            self.state.trouble = Some(format!("{} already exists", path.display()));
            return;
        }
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .map_err(|why| why.to_string())
            .and_then(|()| {
                pros_core::manifest::recommended()
                    .to_json()
                    .map_err(|why| why.to_string())
            })
            .and_then(|text| std::fs::write(&path, text).map_err(|why| why.to_string()));
        match written {
            Ok(()) => self.state.said = format!("written to {}", path.display()),
            Err(why) => self.state.trouble = Some(why),
        }
    }

    /// Takes what a target's repository knows into the list already here, and keeps it.
    ///
    /// **Merged and saved rather than swapped in.** Reading the target used to replace nine
    /// entries with twenty-five, every time, which reads as an explosion instead of as
    /// finding out about sixteen more. Now the first read grows the list once and writes it
    /// down; the next read changes almost nothing, and says so.
    fn absorb(&mut self, repository: &Manifest, from: &str) {
        let before = self.manifest.clone().unwrap_or_default();
        let merged = before.merged_with(repository);
        let (added, changed) = merged.difference_from(&before);

        match merged.save() {
            Ok(path) => {
                self.manifest_from = path.display().to_string();
                // Said out loud, because a merge that silently rewrote a file is one
                // nobody can review.
                self.state.said = match (added, changed) {
                    (0, 0) => format!("{from}: nothing new"),
                    _ => format!("{from}: {added} added, {changed} filled in"),
                };
            }
            // It still shows, it just is not kept. Losing the read would be worse than
            // losing the saving of it.
            Err(why) => {
                self.manifest_from = format!("{from} (from the target, not saved)");
                self.state.trouble = Some(why);
            }
        }
        self.manifest = Some(merged);
    }

    /// Reads the manifest from the usual place.
    /// Reads the manifest from the usual place.
    fn read_manifest(&mut self) {
        let Some(path) = pros_core::manifest::default_path() else {
            self.state.trouble =
                Some("no home directory, so there is nowhere to keep one".to_owned());
            return;
        };
        if !path.exists() {
            // Not a failure. A machine where nobody has written one is the ordinary state
            // of a machine where nobody has written one.
            self.manifest = Some(pros_core::manifest::recommended());
            "the built-in recommended list - it states no digests"
                .clone_into(&mut self.manifest_from);
            return;
        }
        match Manifest::from_file(&path) {
            Ok(manifest) => {
                self.manifest = Some(manifest);
                self.manifest_from = path.display().to_string();
            }
            // Named rather than swallowed: *there is no manifest* and *this is not a
            // manifest* are different problems, and the library words both already.
            Err(why) => self.state.trouble = Some(why.to_string()),
        }
    }

    /// Stages anything dropped on the window.
    ///
    /// **A file dropped here is checked before it is kept**, against the manifest entry whose
    /// file name it matches. That is the whole of the workflow that can exist before there is
    /// any way to fetch: somebody downloads the payload from the project that publishes it -
    /// which is where the manifest points anyway - and this makes sure it is the right one.
    fn take_dropped(&mut self, ctx: &egui::Context) {
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
            // Matched by file name, because that is what the manifest states and what the
            // publisher called it. A file this does not recognise is named back rather than
            // guessed at.
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
                // **Nothing describes it, which is the ordinary case for something you
                // just built.** Offered to run rather than refused.
                //
                // The digest rule is not being loosened here, because it is not the rule that
                // applies. A digest proves bytes from a mirror are the bytes somebody
                // published - it answers *is this what it claims to be*, and the answer
                // matters because the claim came from a stranger. A file you compiled on this
                // machine and dragged onto this window has no such claim: **you are its
                // provenance**, and there is nobody to check it against.
                //
                // What is still checked is the shape, because that is a correctness question
                // rather than a trust one: an ELF the loader cannot take fails the same way
                // whoever built it.
                None => self.state.adhoc = Some(path.clone()),
            }
        }
    }

    /// Puts each section's two sides where they belong, the first time it is shown.
    ///
    /// Only the first time: a person who has navigated somewhere should find it still there
    /// when they come back from another section.
    fn settle(&mut self, section: Section) {
        if self.state.library_place == Some(section) {
            return;
        }
        self.state.library_place = Some(section);
        section.there().clone_into(&mut self.state.library_path);
        self.state.local_path = Self::local_place(section)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.read_local();
        self.browse();
    }

    /// This machine's own folder for a section.
    ///
    /// Beside the registry, one directory per section, so what this project keeps is in one
    /// findable place - and so the payload staging directory and the payloads section are
    /// the same folder rather than two ideas about one.
    fn local_place(section: Section) -> Option<PathBuf> {
        Some(target::directory()?.join(section.name()))
    }

    /// Reads the local side.
    ///
    /// Synchronously, unlike the target side: this is a directory on a disk, and a job
    /// with a thread and a channel for it would be machinery around nothing.
    fn read_local(&mut self) {
        let path = PathBuf::from(self.state.local_path.trim());
        match pros_core::library::here(&path) {
            Ok(items) => self.state.local = items,
            Err(why) => {
                self.state.local.clear();
                self.state.trouble = Some(why);
            }
        }
    }

    /// This machine on the left, the target on the right, and the traffic between them.
    ///
    /// # Why both at once
    ///
    /// Because the question is always comparative. *Do I have this?* and *is it on there?*
    /// are one question, and a view that answers half of it makes a person hold the other
    /// half in their head.
    ///
    /// **The left side works with no target at all.** Somebody organising their own copies
    /// should not be told to register target first.
    fn sync_body(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, self.state.section);

        self.rebuild_listing();
        self.sync_toolbar(ui);
        ui.separator();
        self.refusal(ui);
        self.pending_install(ui, self.state.is_idle());
        self.pending_delete(ui, self.state.is_idle());

        if self.state.merged {
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
                // Applied as a fraction of the usable width, so the split means the same thing
                // after the window is resized as it did before.
                let usable = (left.x + right.x).max(1.0);
                self.state.split = (self.state.split + dragged / usable).clamp(0.15, 0.85);
            }
        }
    }

    /// Rebuilds the merged listing from the two sides, keeping what is still selected.
    ///
    /// **Rebuilt every frame from the sides**, rather than kept and patched. The sides change
    /// under it - a refresh, a download landing, a directory entered - and a listing that was
    /// updated by hand would drift from them in ways nothing would notice.
    fn rebuild_listing(&mut self) {
        let described = self
            .state
            .section
            .tracks()
            .and_then(|kind| kind.read().ok())
            .unwrap_or_default();
        let chosen = std::mem::take(&mut self.state.listing.chosen);
        self.state.listing =
            crate::listing::Listing::build(&described, &self.state.local, &self.state.library);
        self.state.listing.chosen = chosen;
        self.state.listing.forget_what_left();
    }

    /// The actions, which apply to what is ticked rather than to one row.
    ///
    /// **Offered and refused rather than hidden.** A control that vanishes leaves somebody
    /// unable to tell whether they are wrong about the tool or the tool is wrong about them;
    /// a greyed one with the reason on it answers that.
    fn sync_toolbar(&mut self, ui: &mut egui::Ui) {
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        let mut act = None;
        ui.horizontal_wrapped(|ui| {
            // Only what could ever apply here. See `Offer::applies_to`: a control that can
            // never become live is not a disabled control, it is furniture beside the one
            // somebody wanted.
            for offer in crate::listing::Offer::ALL
                .into_iter()
                .filter(|offer| offer.applies_to(self.state.section))
            {
                let can = self.state.listing.offers(offer);
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
            let picked = self.state.listing.chosen.len();
            let all = self.state.listing.entries.len();
            if ui
                .add_enabled(all > 0, egui::Button::new("all"))
                .on_hover_text("tick everything listed")
                .clicked()
            {
                let names: Vec<String> = self
                    .state
                    .listing
                    .entries
                    .iter()
                    .map(|entry| entry.name.clone())
                    .collect();
                self.state.listing.chosen.extend(names);
            }
            if ui
                .add_enabled(picked > 0, egui::Button::new("none"))
                .clicked()
            {
                self.state.listing.chosen.clear();
            }
            ui.weak(format!("{picked} of {all} selected"));

            ui.separator();
            // One list or two. The merged one is the model; the split is a projection of it,
            // so this changes how it is drawn and nothing about what is true.
            if ui
                .selectable_label(self.state.merged, "merged")
                .on_hover_text("one list, with a column for each side")
                .clicked()
            {
                self.state.merged = !self.state.merged;
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
        let picked: Vec<crate::listing::Entry> =
            self.state.listing.picked().into_iter().cloned().collect();
        if picked.is_empty() {
            return;
        }
        // **Both deletes take the whole selection in one job**, so a confirm is asked once
        // rather than per file. Handled before the loop below, which exists for actions that
        // move one thing at a time.
        if offer.is_destructive() {
            self.state.pending_delete = Some((offer, picked));
            return;
        }
        let local = PathBuf::from(self.state.local_path.trim());
        let remote = self.state.library_path.trim_end_matches('/').to_owned();

        // **Install is a confirm, not a job**, so the whole selection goes into one panel that
        // names every package rather than the first one silently becoming the only one.
        if offer == Offer::Install {
            self.state.pending_install =
                Some(picked.iter().map(|entry| local.join(&entry.name)).collect());
            self.state.listing.chosen.clear();
            return;
        }

        // **One job at a time is still the rule; the rest now wait rather than vanish.** This
        // loop used to start the first, untick that one, and break - so a selection of four
        // and one press did one file and said nothing about the other three.
        let mut asked = 0_usize;
        for entry in picked {
            let job = match offer {
                // The loader takes the bytes from here and runs them; nothing is written to
                // the target's disk, which is what makes this different from sending.
                Offer::Run => match local.join(&entry.name) {
                    path if path.is_file() => {
                        Some(Job::Send(target.clone(), entry.name.clone(), path))
                    }
                    _ => None,
                },
                Offer::Send => {
                    let from = local.join(&entry.name);
                    let to = format!("{remote}/{}", entry.name);
                    Some(if entry.here.is_some_and(|side| side.folder) {
                        Job::Restore(target.clone(), from, to, false)
                    } else {
                        Job::Push(target.clone(), from, to)
                    })
                }
                Offer::Fetch => {
                    let from = format!("{remote}/{}", entry.name);
                    let into = local.join(&entry.name);
                    Some(if entry.there.is_some_and(|side| side.folder) {
                        Job::Backup(target.clone(), from, into)
                    } else {
                        Job::Pull(target.clone(), from, into)
                    })
                }
                Offer::Download => entry
                    .described
                    .clone()
                    .map(|payload| Job::Fetch(Box::new(payload), Some(local.clone()))),
                Offer::Launch => Some(Job::Launch(target.clone(), entry.name.clone())),
                // Both handled above, each in one go for the whole selection.
                Offer::Install | Offer::DeleteHere | Offer::DeleteThere => None,
            };
            if let Some(job) = job {
                self.state.queue(job);
                // Unticked as it joins the line, so what stays ticked is what was **not**
                // taken up - which is the only reading of the checkboxes that survives a
                // failure part way through.
                self.state.listing.chosen.remove(&entry.name);
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
                self.reveal(&PathBuf::from(self.state.local_path.trim()));
            }
            if ui
                .small_button("add files...")
                .on_hover_text("copy files from anywhere on this machine into this folder")
                .clicked()
            {
                self.add_files();
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.state.local_path).desired_width(f32::INFINITY));
        ui.separator();

        // **The projection: entries this side knows about.** Something described and on
        // neither side belongs here too - it is a thing to fetch onto this machine, and the
        // target pane has nothing to say about it.
        let section = self.state.section.name();
        let rows: Vec<crate::listing::Entry> = self
            .state
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
                    if group_row(ui, &self.state.folded, &key, label, group.len(), &mut folds) {
                        continue;
                    }
                    for entry in group {
                        if listing_row(ui, entry, &self.state.listing.chosen, false, None).is_some()
                        {
                            toggled = Some(entry.name.clone());
                        }
                    }
                }
            });
        if let Some(name) = toggled {
            self.state.listing.toggle(&name);
        }
        fold(&mut self.state.folded, folds);
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
            // **Which device.** Every screen that browses the target gets the same one,
            // because *is it actually on the stick* is a question every one of them can be
            // asked - and until now none of them could answer it. The places under each
            // device come from `pros_core::places`, which is a table of measured paths with
            // the payload that owns each one named beside it.
            let now = pros_core::places::device_of(&self.state.library_path);
            let mut going_to: Option<String> = None;
            egui::ComboBox::from_id_salt("which-device")
                .selected_text(now.label())
                .show_ui(ui, |ui| {
                    for device in pros_core::places::Device::all() {
                        let spots = pros_core::places::where_to_look(
                            self.state.section.looking_for(),
                            device,
                        );
                        // **A device with nothing measured is shown and says so**, rather than
                        // left out - an absent entry reads as a device that is not there.
                        let Some(first) = spots.first() else {
                            ui.add_enabled(
                                false,
                                egui::SelectableLabel::new(false, device.label()),
                            )
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
                                    self.state.library_path == spot.path,
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
                });
            if let Some(path) = going_to {
                self.state.library_path = path;
                self.state.seen.clear();
                self.browse();
            }
            let above = parent_of(&self.state.library_path);
            if ui
                .add_enabled(
                    idle && connected && above.is_some(),
                    egui::Button::new("up"),
                )
                .on_disabled_hover_text("already at the root")
                .clicked()
                && let Some(above) = above
            {
                self.state.library_path = above;
                self.browse();
            }
            // Only where there are titles to name. A button that does nothing useful in
            // five of six sections is a button somebody has to learn to ignore.
            let titles: Vec<String> = self
                .state
                .library
                .iter()
                .filter(|item| item.kind == LibraryKind::Title)
                .map(|item| item.name.clone())
                .collect();
            // Only in the saves section, where the answer is two folders down and which
            // user is a question this project will not answer for somebody.
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
        });
    }

    /// What the target is: firmware, target, storage, and what is running.
    ///
    /// **Firmware first, because it decides everything else.** Which jailbreak works, which
    /// payloads run, whether a game needs backporting - all of it follows from that one line,
    /// and it is the thing people otherwise go and look up somewhere else.
    ///
    /// Controllers presented to the target from this machine.
    ///
    /// # What works today and what does not
    ///
    /// The **keyboard half works now**, because the window already receives key state and no
    /// dependency is needed to read it. Four slots, each independently assignable, each
    /// sending under its own number.
    ///
    /// What is missing is the far end: no payload accepts these yet. So the records are built
    /// and counted and shown, and nothing is sent - which is a state worth being able to look
    /// at, because it means the mapping can be finished and tested before there is anything to
    /// send to.
    ///
    /// Reading a **physical** controller is a separate and real decision: this workspace
    /// forbids unsafe code, so the platform APIs are out of reach directly and a crate would
    /// have to be argued for like every other dependency here. A slot set to one says nothing
    /// can read it rather than quietly behaving like an empty slot.
    /// Reads the keyboard and sends a pad record, every frame.
    ///
    /// # Why this is not in the panel that draws pads
    ///
    /// It was, and that was a bug of exactly the kind this project keeps writing. A pump
    /// inside a panel's drawing runs only on frames where that panel is drawn - so input
    /// stopped the moment somebody switched sections, and the section they switch to is the
    /// **stream**, which is the one place they are certainly trying to play.
    ///
    /// Nothing said so. A feed sending nothing and a feed not being polled at all look
    /// identical from outside, which is this project's recurring defect: a mechanism whose
    /// *did nothing* is indistinguishable from its *changed nothing*.
    ///
    /// So it ticks from `update`, unconditionally, and the panel only draws.
    fn drive_pads(&mut self, ctx: &egui::Context) {
        // **Level or edge.** A key held across frames reports down, which is what a hold
        // needs - but a press and release landing inside one frame would otherwise be
        // invisible, and the shortest real tap on a fast display is close to that. Reading
        // only `keys_down` drops those, which reads as a controller that misses inputs.
        //
        // Taken from orbistoun's window, which had it first. Credited in ACKNOWLEDGEMENTS.
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

        let asking = self.state.binding.take();
        if let Some(button) = asking {
            // The first key pressed while waiting takes the binding. Escape abandons it, so a
            // rebinding started by accident is not something somebody has to complete.
            if let Some(name) = held.first() {
                if name != "Escape"
                    && let Some(number) = self.state.binding_slot
                    && let Some(slot) = self
                        .state
                        .pads
                        .slots
                        .iter_mut()
                        .find(|slot| slot.number() == number)
                {
                    slot.keys.bind(name, button);
                }
                self.state.binding_slot = None;
            } else {
                self.state.binding = Some(button);
            }
        }

        let down = |name: &str| held.iter().any(|key| key == name);
        let records = self.state.pads.poll(&down);
        self.state.pad_records = self.state.pad_records.saturating_add(records.len() as u64);
        // Sent rather than counted and discarded. A feed that is not open counts them as
        // dropped, which is what tells somebody the mapping is fine and the connection is not.
        self.state.feed.send(&records);
    }

    fn controllers_panel(&mut self, ui: &mut egui::Ui) {
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
    /// # Why the three states are drawn differently
    ///
    /// *Not connected*, *sending* and *the connection ended* need different work from a
    /// person, and a bar that showed the third as the first would hide the only fact worth
    /// having - something broke, rather than something never started.
    fn feed_bar(&mut self, ui: &mut egui::Ui) {
        let connected = self.state.target().is_some();
        let sending = self.state.feed.status.is_sending();

        ui.horizontal(|ui| {
            if sending {
                if ui.button("stop").clicked() {
                    self.state.feed.close();
                }
            } else if ui
                .add_enabled(connected, egui::Button::new("connect"))
                .on_disabled_hover_text("select a target first")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                let port = self
                    .state
                    .feed_port
                    .trim()
                    .parse()
                    .unwrap_or(pros_link::feed::PORT);
                // The error is kept in the feed's own status, which the line below draws -
                // so ignoring it here loses nothing.
                let _ = self.state.feed.open(&target.address, port);
            }
            ui.small("port:");
            ui.add(egui::TextEdit::singleline(&mut self.state.feed_port).desired_width(60.0));

            let colour = match &self.state.feed.status {
                pros_link::feed::Status::Sending => egui::Color32::from_rgb(120, 200, 140),
                pros_link::feed::Status::Idle => egui::Color32::GRAY,
                // A break and a refusal are both worth noticing, and neither is grey.
                pros_link::feed::Status::Lost(_) | pros_link::feed::Status::Refused(_) => {
                    egui::Color32::from_rgb(220, 120, 120)
                }
            };
            ui.colored_label(colour, self.state.feed.status.describe());
        });

        if sending {
            ui.small(format!("{} records sent", self.state.feed.sent));
        } else {
            // **Nothing accepts these yet**, and that is said here rather than as a permanent
            // banner: once a payload exists the sentence stops being true, and a warning that
            // outlives its reason is one people learn to read past.
            ui.small("no payload accepts these yet - see docs/vIDEO.md part three");
            if self.state.feed.dropped > 0 {
                ui.small(format!(
                    "{} records had nowhere to go",
                    self.state.feed.dropped
                ));
            }
        }
    }

    /// Every key doing two jobs, named.
    ///
    /// **Shown rather than resolved.** Silently unbinding whatever had a key is the same fault
    /// from the other side - a dead button nobody was told about - so the panel says what is
    /// wrong and leaves the decision where it belongs.
    fn pad_conflicts(&mut self, ui: &mut egui::Ui) {
        let found = self.state.pads.conflicts();
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
        ui.strong(format!("slots  ({} filled)", self.state.pads.filled()));

        let mut binding = None;
        egui::Grid::new("pad-slots").striped(true).show(ui, |ui| {
            for slot in &mut self.state.pads.slots {
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
                    // Glyphs, because this line is read while looking at a controller and
                    // `triangle` asks somebody to translate where the shape does not.
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
            self.state.binding = Some(button);
        }
    }

    /// One slot's key layout, and a way to change it.
    ///
    /// **Per slot, not shared.** Two people on one keyboard need two layouts, and a single
    /// shared one makes the second player impossible rather than merely awkward.
    fn pad_keys(&mut self, ui: &mut egui::Ui) {
        let waiting = self.state.binding;
        let chosen = self.state.binding_slot;
        for slot in 0..self.state.pads.slots.len() {
            let number = self.state.pads.slots[slot].number();
            let bound = self.state.pads.slots[slot].is_bound();
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
                            // The word as well as the shape here: a prompt naming one button
                            // out of context is the one place the glyph alone is ambiguous.
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
                                // Shape and word: this is a table somebody reads down while
                                // rebinding, so the shape finds the row and the word confirms
                                // it - and the word is what a saved layout will contain.
                                ui.horizontal(|ui| {
                                    ui.monospace(button.glyph());
                                    ui.weak(button.name());
                                });
                                let key = self.state.pads.slots[slot]
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
                self.state.binding = Some(button);
                self.state.binding_slot = Some(number);
            }
        }
    }

    /// Nothing here is filled in from anything else here. A target that answers about its
    /// model and not its processors reports the model and says nothing about processors,
    /// because a plausible value in a panel is indistinguishable from a measured one.
    fn system_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::System);

        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        if ui
            .add_enabled(idle && connected, egui::Button::new("ask the target"))
            .on_disabled_hover_text("no target selected")
            .clicked()
            && let Some(target) = self.state.target().cloned()
        {
            self.state.begin(Job::ReadSystem(target));
        }
        ui.add_space(8.0);

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
        Self::process_list(ui, &report);
    }

    /// The target's own storage, with its sandbox mounts folded away.
    fn storage_table(ui: &mut egui::Ui, report: &pros_core::system::Report) {
        if !report.storage.is_empty() {
            // **The target's storage, then its sandbox mounts behind a fold.** A target
            // measured here listed 1183 filesystems: twenty-two are the machine, the rest
            // are bind mounts inside running applications. Listing them flat would bury
            // the ones that answer how much room is left.
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

    /// What is running, titles first.
    fn process_list(ui: &mut egui::Ui, report: &pros_core::system::Report) {
        let titles: Vec<&pros_core::system::Process> = report
            .processes
            .iter()
            .filter(|one| one.is_a_title())
            .collect();
        if !report.processes.is_empty() {
            ui.add_space(10.0);
            ui.strong(format!(
                "running  ({} processes, {} of them titles)",
                report.processes.len(),
                titles.len()
            ));
            // Titles first: a person looking at this list wants the game, and the twenty
            // system processes around it are context rather than the answer.
            for one in &titles {
                ui.horizontal(|ui| {
                    ui.monospace(&one.title);
                    ui.label(&one.command);
                    ui.weak(&one.state);
                });
            }
            egui::CollapsingHeader::new("everything else")
                .default_open(false)
                .show(ui, |ui| {
                    for one in &report.processes {
                        if one.is_a_title() {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            ui.weak(&one.pid);
                            ui.label(&one.command);
                            ui.weak(&one.state);
                        });
                    }
                });
        }
    }

    /// What the target loads at startup, and the manager's settings.
    ///
    /// # Read-only until somebody presses the one button that is not
    ///
    /// This is the first place in the tool that could write to a target, and the file it
    /// would write decides what loads at boot. A wrong one is a target that comes up without
    /// its file service or its loader - the exact state in which nothing here can help, and
    /// the recovery is re-running the jailbreak by hand.
    ///
    /// So the view shows what is there, an edit produces a **diff rather than a write**, and
    /// the write happens on a second, explicit press with those lines on screen. Confirming
    /// *"change the delay"* and confirming *these two lines* are different acts, and only the
    /// second catches a tool about to do something else as well.
    fn autoload_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::Autoload);

        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle && connected, egui::Button::new("read"))
                .on_hover_text("re-read the startup list and the manager's settings")
                .on_disabled_hover_text(if connected {
                    "wait for what is already running"
                } else {
                    "no target selected"
                })
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                // Not emptied first: the list stays up while it is read again, with the
                // panel saying so. See the re-read notice in `section`.
                self.state.followed_for = None;
                self.state.begin(Job::ReadAutoload(target));
            }
            // **Which list is being looked at.** The manager keeps one at a fixed path; the
            // autoloader that runs before it looks in several places, and *that* list is the
            // one that decides whether the manager runs at all. They are audited by opposite
            // rules, so which one this is showing is not a detail.
            let held = self.state.list();
            egui::ComboBox::from_id_salt("which-list")
                .selected_text(held.label)
                .show_ui(ui, |ui| {
                    for (at, one) in pros_core::chain::LISTS.iter().enumerate() {
                        if ui
                            .selectable_label(self.state.list_at == at, one.label)
                            .on_hover_text(format!(
                                "{}
{}",
                                one.path,
                                if one.editable {
                                    "editable"
                                } else {
                                    "read only - a list on removable storage is \
                                     the way back in when the internal one is broken"
                                }
                            ))
                            .clicked()
                            && self.state.list_at != at
                        {
                            self.state.list_at = at;
                            self.state.boot = None;
                            self.state.pending_change = None;
                            if let Some(target) = self.state.target().cloned() {
                                self.state.begin(Job::ReadList(target, *one));
                            }
                        }
                    }
                });
            // **Reading a chain out, as against deploying one in.** The presets that ship
            // here were taken from a working console by hand, once. A person whose console
            // works has the thing those were copied from, and until now the only way to keep
            // it was to read the list off this screen and retype it into a file.
            let worth_exporting = self
                .state
                .boot
                .as_ref()
                .is_some_and(|boot| boot.steps.iter().any(|step| !step.is_disabled()));
            if ui
                .add_enabled(worth_exporting, egui::Button::new("export chain..."))
                .on_hover_text(
                    "write this list down as a chain preset of your own, so it can be \
                     deployed to another target - or to this one after something breaks it",
                )
                .on_disabled_hover_text(if connected {
                    "read a list first - there is nothing to write down"
                } else {
                    "no target selected"
                })
                .clicked()
            {
                self.begin_export(held);
            }
            ui.weak(held.path);
            if !held.editable {
                ui.colored_label(egui::Color32::from_rgb(210, 190, 120), "read only");
            }
        });
        ui.separator();

        self.list_findings(ui, self.state.list(), idle);
        self.boot_list(ui, connected);
        ui.add_space(10.0);
        // **The settings belong to the manager, and only to it.** Drawing them under an
        // autoloader's list would offer somebody a checkbox that changes a different file
        // from the one they are looking at - and `AUTOLOAD_ENABLED` under a list that is not
        // the one it enables is the exact confusion that cost this target its jailbreak.
        if self.state.list().autoloader {
            ui.weak("the manager's settings belong to its own list - choose it to see them");
        } else {
            self.settings_rows(ui);
        }
        self.export_panel(ui);
        self.pending_write(ui, idle, connected);
    }

    /// Builds the preset from what was read, so the panel has something to show.
    ///
    /// # Why the list is put back through the chain parser
    ///
    /// A step holds the line as written - `kstuff-lite_v1.09.elf`, and `#` in front of one that
    /// is turned off. A preset entry is the payload's bare name. There is already one piece of
    /// code that does that translation, and it is the one every comparison in this program goes
    /// through; a second here would be a second answer to *is this the same payload*, which is
    /// the question this project has got wrong more times than any other.
    fn begin_export(&mut self, held: pros_core::chain::Held) {
        let Some(boot) = self.state.boot.as_ref() else {
            return;
        };
        let disabled = boot.steps.iter().filter(|step| step.is_disabled()).count();
        let lines = boot
            .steps
            .iter()
            .map(|step| step.payload.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let entries = pros_core::chain::Chain::parse(&lines).order().to_vec();

        let target = self
            .state
            .target()
            .map_or_else(|| "a target".to_owned(), |one| one.name.clone());
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        let taken_from = format!("{} on {target}, at {}", held.label, held.path);
        let (preset, notes) =
            pros_core::recovery::baseline::from_list("", &taken_from, &entries, kind);

        self.state.exporting = Some(crate::state::Exporting {
            name: format!(
                "{target}-{}",
                if held.autoloader { "autoloader" } else { "manager" }
            ),
            preset,
            notes,
            disabled,
            into: pros_core::recovery::baseline::path()
                .map_or_else(|| "nowhere on this machine".to_owned(), |at| {
                    at.display().to_string()
                }),
            taken: pros_core::recovery::baseline::all()
                .0
                .into_iter()
                .map(|one| one.name)
                .collect(),
        });
    }

    /// What would be written down, where, and what it could not know.
    ///
    /// # Why this asks at all, when it writes to this machine and not to the console
    ///
    /// Because it replaces by name, and the name somebody types is the whole of what decides
    /// whether this is a new preset or their existing one gone. Everything else on this screen
    /// that overwrites something says what it would overwrite first, and a local file is not a
    /// good enough reason to be the exception.
    fn export_panel(&mut self, ui: &mut egui::Ui) {
        let Some(export) = self.state.exporting.as_mut() else {
            return;
        };

        ui.add_space(8.0);
        ui.separator();
        ui.strong("WRITE THIS LIST DOWN AS A CHAIN PRESET");
        ui.weak(&export.preset.about);
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label("call it:");
            ui.text_edit_singleline(&mut export.name);
        });
        let name = export.name.trim().to_owned();
        // **One word, because the registry line is whitespace-delimited.** A name with a space
        // in it would be written as `chain=<half>` and the rest read as an address.
        let usable = !name.is_empty() && !name.contains(char::is_whitespace);
        if name.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(230, 160, 90), "it needs a name");
        } else if !usable {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "a preset name is one word - a target's registry line is whitespace-delimited, \
                 so half of this would be read as an address",
            );
        } else if export.taken.contains(&name) {
            ui.colored_label(
                egui::Color32::from_rgb(230, 160, 90),
                format!(
                    "there is already a preset called {name}, and this replaces it. A preset \
                     that ships with this program is not changed on disk, but yours wins."
                ),
            );
        }

        ui.weak(format!("into {}", export.into));
        ui.add_space(4.0);
        ui.label(format!("{} entries, in this order:", export.preset.entries.len()));
        // Ordered as the preset orders them, not as they were read - for a manager's list those
        // differ by exactly the entry whose two positions had to be told apart, and showing the
        // read order here would hide that from the one person who could catch it.
        let mut shown = export.preset.entries.clone();
        shown.sort_by_key(|one| one.rank(pros_core::recovery::Kind::Manager));
        for entry in &shown {
            ui.weak(format!("    {}", entry.name));
        }
        if export.disabled > 0 {
            ui.weak(format!(
                "{} disabled {} left out - a line the manager will not resolve is not part of \
                 what this target loads",
                export.disabled,
                if export.disabled == 1 { "line" } else { "lines" }
            ));
        }
        if !export.notes.is_empty() {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(210, 190, 120),
                "what this could not know, and did not invent:",
            );
            for note in &export.notes {
                ui.weak(format!("    {note}"));
            }
        }

        ui.add_space(6.0);
        let mut write_it = false;
        let mut drop_it = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(usable, egui::Button::new("write it"))
                .on_disabled_hover_text("give it a one-word name first")
                .clicked()
            {
                write_it = true;
            }
            if ui.button("cancel").clicked() {
                drop_it = true;
            }
        });

        if drop_it {
            self.state.exporting = None;
            return;
        }
        if !write_it {
            return;
        }
        let mut preset = export.preset.clone();
        preset.name = name;
        // Straight to the filesystem rather than through a job: this writes one small file on
        // this machine, and the queue is for things that talk to a target and can hang.
        self.state.said = match pros_core::recovery::baseline::keep(&preset) {
            Ok(at) => {
                self.state.exporting = None;
                format!(
                    "{} written to {} - it is offered as a chain from the next start",
                    preset.name,
                    at.display()
                )
            }
            // Kept open on failure. The panel holds the only copy of what was measured, and
            // closing it would mean reading the list off the target again to try twice.
            Err(why) => format!("not written: {why}"),
        };
    }

    /// Setting a target up from nothing: which list, what would go in it, and a warning.
    ///
    /// # Why it asks twice
    ///
    /// This is the most destructive thing here - it replaces a whole startup list, and one of
    /// the places it can point at is the stick somebody keeps as their way back in when the
    /// internal list is broken. So it agrees a plan first, and the file that plan produces then
    /// goes through the same whole-file review as every other write. Neither of those is
    /// ceremony: the first says what will be fetched and sent, the second says what the target
    /// will actually try to run.
    fn configurator(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some(at) = self.state.setting_up else {
            return;
        };
        let chosen = pros_core::chain::LISTS.get(at).copied();
        let Some(held) = chosen else {
            self.state.setting_up = None;
            return;
        };

        ui.add_space(8.0);
        ui.separator();
        ui.colored_label(
            egui::Color32::from_rgb(230, 90, 90),
            "SET UP FROM NOTHING - THIS REPLACES A STARTUP LIST",
        );
        ui.add_space(4.0);

        self.setup_choices(ui, at, held);
        // **What is there now, so *overwritten* is a quantity rather than a word.** Only for
        // the list being shown: reading another one to count it would be a request, and this
        // panel does not make requests.
        let showing = self.state.list_at == at;
        match (showing, self.state.boot.as_ref()) {
            (true, Some(boot)) if !boot.steps.is_empty() => {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 160, 90),
                    format!(
                        "{} entries are in {} now, and all of them go",
                        boot.steps.len(),
                        held.path
                    ),
                );
            }
            (true, _) => {
                ui.weak(format!("{} is empty or was not read", held.path));
            }
            (false, _) => {
                ui.colored_label(
                    egui::Color32::from_rgb(230, 160, 90),
                    format!(
                        "whatever is in {} now will be replaced - this screen is showing a \
                         different list, so it has not been read",
                        held.path
                    ),
                );
            }
        }
        if !held.editable {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "this list is on removable storage - the one that gets you back in when the \
                 internal one is broken. Setting it up replaces exactly that.",
            );
        }

        ui.add_space(6.0);
        let mut go = false;
        let mut drop_it = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("show me what it would write"))
                .on_hover_text("plan it - nothing happens until you agree to the plan")
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                go = true;
            }
            if ui.button("cancel").clicked() {
                drop_it = true;
            }
        });

        if drop_it {
            self.state.setting_up = None;
        }
        if go {
            self.plan_a_setup(held);
        }
    }

    /// The two questions the configurator asks before it will plan anything.
    ///
    /// **Which chain, then which list** - the order somebody decides them in: what this target
    /// is going to run, and then where the file that runs it goes.
    fn setup_choices(&mut self, ui: &mut egui::Ui, at: usize, held: pros_core::chain::Held) {
        // **Which chain, before which list.** They are asked in the order somebody decides
        // them: what this target is going to run, and then where the file that runs it goes.
        let (presets, trouble) = pros_core::recovery::baseline::all();
        let mut pick = at;
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.label("chain:");
            egui::ComboBox::from_id_salt("which-chain")
                .selected_text(&self.state.preset)
                .show_ui(ui, |ui| {
                    for one in &presets {
                        if ui
                            .selectable_label(one.name == self.state.preset, &one.name)
                            .on_hover_text(&one.about)
                            .clicked()
                        {
                            chosen = Some(one.name.clone());
                        }
                    }
                });
            ui.label("into:");
            egui::ComboBox::from_id_salt("setting-up")
                .selected_text(held.label)
                .show_ui(ui, |ui| {
                    for (which, one) in pros_core::chain::LISTS.iter().enumerate() {
                        if ui
                            .selectable_label(which == at, one.label)
                            .on_hover_text(one.path)
                            .clicked()
                        {
                            pick = which;
                        }
                    }
                });
        });
        if let Some(name) = chosen {
            self.state.preset = name;
        }
        if pick != at {
            self.state.setting_up = Some(pick);
        }
        // **A file somebody wrote and this could not read is said out loud.** Quietly falling
        // back to the shipped presets would have them setting a target up from a chain they
        // thought they had replaced.
        if let Some(why) = trouble {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), why);
        }
        if let Some(one) = presets.iter().find(|one| one.name == self.state.preset) {
            ui.weak(&one.about);
            if !one.result.is_empty() {
                ui.add_space(4.0);
                ui.label("what you end up with:");
                // Printed exactly as the chains file states it. This program has no opinion
                // about what a chain does; the file that describes the chain does.
                ui.colored_label(egui::Color32::from_rgb(150, 190, 220), &one.result);
            }
        }
        if let Some(path) = pros_core::recovery::baseline::path() {
            ui.weak(format!("chains are read from {}", path.display()))
                .on_hover_text(
                    "a file of this shape beside the registry adds chains, or replaces one of \
                     these by using its name - read when this program starts",
                );
        }
    }

    /// Builds the configurator's plan and hands it to the panel that agrees to plans.
    fn plan_a_setup(&mut self, held: pros_core::chain::Held) {
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        let preset = pros_core::recovery::baseline::named(&self.state.preset)
            .unwrap_or_else(pros_core::recovery::baseline::first);
        // **Written down on the target, because the advice depends on it.** Everything after
        // this - what the check reports missing, what a fix offers - is answered against the
        // chain this target is meant to be running, and the only thing that knows is the
        // registration. Recorded when the plan is made rather than when it finishes: it is what
        // somebody has decided, and a plan they abandon leaves a decision they still made.
        if let Some(target) = self.state.target().cloned() {
            match target::remember_chain(&target.name, Some(&preset.name)) {
                Ok(_) => {
                    if let Some(one) = self
                        .state
                        .targets
                        .iter_mut()
                        .find(|one| one.name == target.name)
                    {
                        one.chain = Some(preset.name.clone());
                    }
                }
                // Not fatal: the plan is still correct and can still be carried out. What is
                // lost is the *next* check knowing which chain to judge this target against,
                // so it is said rather than swallowed.
                Err(why) => {
                    self.state.trouble = Some(format!("the chain was not recorded: {why}"));
                }
            }
        }
        let (plan, left_out) =
            self.with_known(|known| pros_core::doctor::provision(known, held.path, kind, &preset));
        self.state.setting_up = None;
        // **Named, not dropped quietly.** A payload with no route is left out of the list on
        // purpose - an entry naming a file the loader cannot find fails at every boot with only
        // a log line to say so - but somebody setting a target up needs to know which ones.
        if !left_out.is_empty() {
            self.state.said = format!("left out, with no way to get them: {}", left_out.join("; "));
        }
        self.state.pending_plan = Some(crate::state::Pending {
            id: format!("set up {}", held.path),
            label: format!("{} runs the {} chain", held.label, preset.name),
            plan,
        });
    }

    /// What is wrong with **this** list, on the screen where it is edited.
    ///
    /// # Why it is here and not only on the check screen
    ///
    /// The check screen audits the manager's own list, because that is the one it reads beside
    /// its probe. This screen shows whichever list somebody chose - and it is the screen they
    /// are on when they change one. A finding about a list, visible only on a screen that is
    /// looking at a different list, is a finding that arrives after the edit it was about.
    ///
    /// It shows the list checks only. What is answering right now is a real question and it is
    /// not this screen's.
    fn list_findings(&mut self, ui: &mut egui::Ui, held: pros_core::chain::Held, idle: bool) {
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        // Parsed from what is on screen rather than from what was read, so an edit somebody has
        // made and not yet saved is audited as it is being made.
        let shown = self
            .state
            .boot
            .as_ref()
            .map(|boot| pros_core::chain::Chain::parse(&boot.to_text()));
        let findings = self.with_known_of(shown.as_ref(), kind, pros_core::doctor::examine_list);
        if findings.is_empty() {
            return;
        }

        let mut asked: Option<crate::state::Pending> = None;
        let mut choose: Option<(String, String)> = None;
        ui.add_space(4.0);
        egui::Grid::new("list-findings")
            .striped(true)
            .num_columns(4)
            .show(ui, |ui| {
                for finding in &findings {
                    let (mark, colour) = mark_of(&finding.verdict, finding.gravity);
                    ui.colored_label(colour, mark);
                    ui.label(&finding.label);
                    ui.label(finding.verdict.describe());
                    match doctor_action(ui, finding, idle) {
                        Some(Asked::Plan(one)) => asked = Some(one),
                        Some(Asked::Chose(id, name)) => choose = Some((id, name)),
                        None => {}
                    }
                    ui.end_row();
                }
            });
        // Nothing green is drawn here. A list with nothing wrong says so once, at the bottom of
        // the table, rather than putting a row of reassurance above every list somebody opens.
        if findings.iter().all(|one| !one.verdict.is_unwell()) {
            ui.weak("nothing here says this list leaves you locked out");
        }
        ui.add_space(4.0);

        if let Some((id, name)) = choose
            && let pros_core::doctor::Remedy::Ready(plan) =
                self.with_known(|known| pros_core::doctor::plan_for(known, &name))
        {
            asked = Some(crate::state::Pending {
                id,
                label: format!("{name} is in the startup list"),
                plan,
            });
        }
        if let Some(one) = asked {
            self.state.pending_plan = Some(one);
        }
    }

    /// The startup list, in order, with the controls that change it.
    ///
    /// # Why one row is selected rather than several
    ///
    /// Every other listing here selects many, because its actions apply to many. These do not:
    /// moving a step up moves *one* step, and moving several at once has an order of its own
    /// that nobody stated. One selection is what the actions actually mean.
    #[allow(
        clippy::too_many_lines,
        reason = "one table, and splitting a grid across two functions costs the shared \
                  column widths that keep its rows in line"
    )]
    fn boot_list(&mut self, ui: &mut egui::Ui, connected: bool) {
        let known = self.catalogue.clone();
        let described = self.manifest.clone();
        let chain_here = self.chain_of_target();
        let loader_here = self.loader_is_up();
        let held = self.state.list();
        let Some(boot) = self.state.boot.clone() else {
            ui.weak(if connected {
                "not read yet"
            } else {
                "select a target, and this reads its startup list"
            });
            return;
        };

        let at = self.state.boot_at;
        let act = self.boot_controls(ui, &boot, at);
        ui.add_space(4.0);

        let there = self.state.payloads_there.clone();
        // **Only a change to this file.** A settings edit is also a pending change, and
        // feeding one to a table that renders the startup list drew `KILL_DISC_PLAYER=1` as
        // an entry being removed from the boot order. Two files, two panels.
        let pending = self
            .state
            .pending_change
            .clone()
            .filter(|change| change.into == pros_core::chain::PATH);
        let pending = pending.as_ref();
        let mut picked = None;
        egui::Grid::new("boot")
            .striped(true)
            .num_columns(7)
            .show(ui, |ui| {
                headings(
                    ui,
                    &[
                        "order",
                        "change",
                        "payload",
                        "storage",
                        "before it",
                        "what",
                        "why",
                    ],
                );
                // **What each entry is on the target**, so a row can say where its file lives
                // rather than leaving that to a panel underneath.
                // **Why an entry is in the list, kept whether or not it is changing.** What a
                // service buys comes from the catalogue, so it is a property of the service
                // rather than prose written here - and a payload nobody has declared anything
                // about says nothing, which is honest and is not a gap to fill with a guess.
                // **Three sources, most specific first, so every row can say something.**
                //
                // The catalogue knows five services and nothing else, so on its own it left
                // most of the list blank - and a column that is empty for two rows in three
                // is a column nobody reads.
                //
                // Below it are two descriptions that travel with the payload itself: the
                // sidecar the manager wrote beside the file when it installed it, and the
                // published list. Neither is prose written here, which is the point: a reason
                // this program invented would be a reason nobody can correct.
                // **Two questions, two columns, two owners.**
                //
                // *What* a payload is comes from whoever published it - a sidecar the manager
                // wrote when it installed the file, or the payload list. It is about the
                // payload, and it is the same wherever that payload appears.
                //
                // *Why* it is in this chain, at this point, is knowledge about one setup.
                // Nothing on the target records it and nothing here can derive it, so it comes
                // only from a note somebody wrote.
                //
                // They were one column with the first falling back to the second, which reads
                // as an answer to a question nobody asked: *Lite version of kstuff* is a fine
                // WHAT and says nothing at all about WHY that entry loads first.
                let named = |name: &str, against: &str| {
                    pros_core::chain::Chain::parse(name)
                        .position(against)
                        .is_some()
                };
                let what = |name: &str| {
                    there
                        .as_ref()
                        .and_then(|there| there.iter().find(|one| one.name == name))
                        .and_then(|one| {
                            one.about
                                .as_ref()
                                .and_then(|about| about.description.clone())
                        })
                        .filter(|text| !text.trim().is_empty())
                        .or_else(|| {
                            described
                                .as_ref()?
                                .payloads()
                                .iter()
                                .find(|payload| named(name, &payload.name))
                                .and_then(|payload| payload.description.clone())
                                .filter(|text| !text.trim().is_empty())
                        })
                        .or_else(|| {
                            known
                                .services()
                                .iter()
                                .find(|service| named(name, &service.name))
                                .map(|service| service.unlocks.to_string())
                        })
                };
                // What the audit makes of the list as it would be, so a proposed change can say
                // what it was worried about rather than appearing without a reason.
                let hazards = pros_core::recovery::audit(
                    &pros_core::chain::Chain::parse(&boot.to_text()),
                    &known,
                    there.as_deref().unwrap_or_default(),
                    // **The rules invert between the two.** The loader is required in an
                    // autoloader list and impossible in the manager own, so auditing one by
                    // the other rules would recommend exactly the wrong edit.
                    if held.autoloader {
                        pros_core::recovery::Kind::Autoloader
                    } else {
                        pros_core::recovery::Kind::Manager
                    },
                    &chain_here,
                    loader_here,
                );
                // **Why an entry is here, in three kinds, most specific first.**
                //
                // A note somebody wrote wins - it is the only source that knows about *this*
                // setup. Below it are two the program can answer for itself rather than
                // leaving blank:
                //
                // - **why a change is being proposed**, which the audit already knows: it
                //   asked for the edit and can say what it was worried about.
                // - **what role the payload plays**, derived from the catalogue's own flags
                //   rather than written per payload. That is what explains the order: the
                //   loader has to be up before anything is loaded through it.
                let why = |name: &str| {
                    known
                        .note(name)
                        .map(str::to_owned)
                        .or_else(|| {
                            known
                                .services()
                                .iter()
                                .find(|service| named(name, &service.name))
                                .and_then(|service| known.note(&service.name).map(str::to_owned))
                        })
                        .or_else(|| {
                            // The finding that asked for this entry to go in or come out.
                            hazards.iter().find_map(|hazard| match hazard.fix() {
                                Some(
                                    pros_core::recovery::Fix::Add(who)
                                    | pros_core::recovery::Fix::Remove(who),
                                ) if named(name, &who) => Some(hazard.describe()),
                                _ => None,
                            })
                        })
                        .or_else(|| {
                            // The tracked recommendation: why this belongs where it does, the
                            // same on every target because it is a fact about the payloads.
                            pros_core::recovery::baseline::about(name).map(|placed| placed.why)
                        })
                        .or_else(|| {
                            known
                                .services()
                                .iter()
                                .find(|service| named(name, &service.name))
                                .and_then(role_of)
                        })
                        .filter(|text| !text.trim().is_empty())
                };
                let placed = |name: &str| {
                    there.as_ref().and_then(|there: &Vec<There>| {
                        there
                            .iter()
                            .find(|one| one.name == name)
                            .map(|one| one.storage)
                    })
                };
                // **The list as it stands, with the change marked in place.** A removed entry
                // keeps its row and its number: it is in the list on the target, and dropping
                // the row read as removing something that was not there.
                let rows = pending.map_or_else(
                    || {
                        boot.steps
                            .iter()
                            .enumerate()
                            .map(|(at, step)| pros_core::autoload::Shown {
                                was_at: Some(at),
                                now_at: Some(at),
                                payload: step.payload.clone(),
                            })
                            .collect()
                    },
                    pros_core::autoload::Change::shown,
                );
                for row in &rows {
                    // Selection and the actions work on the pending list, so an entry that
                    // would not be in it cannot be picked - there is nothing to move.
                    let index = row.now_at;
                    let chosen = index.is_some() && at == index;
                    let order = match (row.was_at, row.now_at) {
                        (Some(was), Some(now)) if was != now => format!("{was} -> {now}"),
                        (Some(was), _) => format!("{was}"),
                        (None, Some(now)) => format!("-> {now}"),
                        (None, None) => String::new(),
                    };
                    let order = if row.moved() || row.added() {
                        egui::RichText::new(order).color(egui::Color32::from_rgb(120, 200, 140))
                    } else if row.removed() {
                        egui::RichText::new(order).color(egui::Color32::from_rgb(220, 120, 120))
                    } else {
                        egui::RichText::new(order)
                    };
                    if ui.selectable_label(chosen, order).clicked()
                        && let Some(index) = index
                    {
                        picked = Some(index);
                    }
                    if row.removed() {
                        ui.colored_label(egui::Color32::from_rgb(220, 120, 120), "removed")
                            .on_hover_text("still on the target - saving takes it out");
                    } else if row.added() {
                        ui.colored_label(egui::Color32::from_rgb(120, 200, 140), "added")
                            .on_hover_text("not on the target yet - saving puts it in");
                    } else if row.moved() {
                        ui.colored_label(egui::Color32::from_rgb(120, 200, 140), "moved")
                            .on_hover_text(
                                "the manager loads the list in order, so this would run at a \
                                 different point",
                            );
                    } else {
                        ui.weak("");
                    }
                    // The step behind the row, when there is one. A removed entry has none:
                    // it is not in the pending list, which is what the steps are.
                    let step = index.and_then(|index| boot.steps.get(index));
                    let Some(step) = step else {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 120, 120),
                            egui::RichText::new(&row.payload).strikethrough(),
                        );
                        ui.weak("");
                        ui.weak("");
                        ui.weak(what(&row.payload).unwrap_or_default());
                        reason(ui, why(&row.payload));
                        ui.end_row();
                        continue;
                    };
                    let index = index.unwrap_or_default();
                    // **Three states, because a set nobody has read is not an empty one.**
                    // Marking every entry missing before the target has been asked would put
                    // a red row against a chain that is perfectly fine.
                    // A disabled entry is not missing: it is off on purpose, and the manager
                    // failing to resolve it is the mechanism rather than a fault.
                    let missing = (!step.is_disabled())
                        .then(|| {
                            there
                                .as_ref()
                                .map(|there| !there.iter().any(|one| one.name == step.name()))
                        })
                        .flatten();
                    let label = if step.is_disabled() {
                        // Struck through and dim: it is in the list and it will not load,
                        // which is neither of the other two states.
                        egui::RichText::new(step.name())
                            .strikethrough()
                            .color(egui::Color32::GRAY)
                    } else if missing == Some(true) {
                        egui::RichText::new(step.name())
                            .color(egui::Color32::from_rgb(220, 120, 120))
                    } else {
                        egui::RichText::new(step.name())
                    };
                    if ui.selectable_label(chosen, label).clicked() {
                        picked = Some(index);
                    }
                    // Where its file is, which decides whether the manager can resolve it at
                    // all - the same three states the add list is tagged with.
                    match placed(step.name()) {
                        Some(storage) if storage.can_autoload() => {
                            ui.weak(storage.tag()).on_hover_text(storage.means());
                        }
                        Some(storage) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), storage.tag())
                                .on_hover_text(storage.means());
                        }
                        None => {
                            ui.weak("");
                        }
                    }
                    // The instruction that precedes it. Shown because reordering carries it,
                    // and a thing that moves invisibly is a thing somebody cannot check.
                    if step.is_disabled() {
                        ui.weak("off");
                    } else {
                        match missing {
                            Some(true) => {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 120, 120),
                                    "not on the target",
                                )
                                .on_hover_text(
                                    "the manager loads this by name and will not find it - the \
                                 chain stops here at the next restart",
                                );
                            }
                            _ => {
                                ui.weak(step.before.as_deref().unwrap_or(""));
                            }
                        }
                    }
                    ui.weak(what(step.name()).unwrap_or_default());
                    reason(ui, why(step.name()));
                    ui.end_row();
                }
            });
        if boot.steps.is_empty() {
            ui.weak("the startup list is empty");
        }
        ui.small(
            "an entry is removed rather than commented out: nothing here knows whether the \
             manager accepts comments, and a line it does not understand may stop the chain",
        );

        if let Some(index) = picked {
            self.state.boot_at = Some(index);
        }
        if let Some(act) = act {
            let mut edited = boot;
            if act(&mut edited) {
                // The selection follows the row, not the position - moving something and
                // leaving the highlight behind means the next press moves a different thing.
                if let Some(was_at) = at {
                    self.state.boot_at = edited.steps.iter().position(|step| {
                        Some(&step.payload) == boot_name(self.state.boot.as_ref(), was_at)
                    });
                }
                self.state.pending_change = edited.change();
                self.state.boot = Some(edited);
            }
        }
    }

    /// The controls that reorder the startup list, and what one of them was asked to do.
    ///
    /// Returned rather than applied, because applying borrows the list these were drawn from -
    /// the same rule the rest of the window follows.
    fn boot_controls(
        &mut self,
        ui: &mut egui::Ui,
        boot: &pros_core::boot::Boot,
        at: Option<usize>,
    ) -> Option<Edit> {
        let last = boot.steps.len().saturating_sub(1);
        let mut act: Option<Edit> = None;
        // **Nothing is offered for a list this will not write.** A control that edits
        // something and then has nowhere to save it is worse than no control: it spends
        // somebody time and loses the edit.
        let editable = self.state.list().editable;
        ui.horizontal_wrapped(|ui| {
            let picked = at.is_some() && editable;
            if ui
                .add_enabled(picked && at != Some(0), egui::Button::new("up"))
                .on_hover_text("load this one earlier")
                .on_disabled_hover_text(if picked {
                    "already first"
                } else {
                    "select a row"
                })
                .clicked()
                && let Some(at) = at
            {
                act = Some(Box::new(move |boot| boot.earlier(at)));
            }
            if ui
                .add_enabled(picked && at != Some(last), egui::Button::new("down"))
                .on_hover_text("load this one later")
                .on_disabled_hover_text(if picked {
                    "already last"
                } else {
                    "select a row"
                })
                .clicked()
                && let Some(at) = at
            {
                act = Some(Box::new(move |boot| boot.later(at)));
            }
            let step = at.and_then(|at| boot.steps.get(at));
            let off = step.is_some_and(BootStep::is_disabled);
            if ui
                .add_enabled(
                    picked,
                    egui::Button::new(if off { "enable" } else { "disable" }),
                )
                .on_hover_text(
                    "keep it in the list and stop it loading - the manager logs the \
                     name it cannot find and carries on",
                )
                .on_disabled_hover_text("select a row")
                .clicked()
                && let Some(at) = at
            {
                act = Some(Box::new(move |boot| boot.disable(at, !off)));
            }
            if ui
                .add_enabled(picked, egui::Button::new("remove"))
                .on_hover_text(
                    "take it out of the startup list - the file stays on the target, and \
                     nothing is written until you say so",
                )
                .on_disabled_hover_text("select a row")
                .clicked()
                && let Some(at) = at
            {
                act = Some(Box::new(move |boot| boot.remove(at)));
            }

            ui.separator();
            // **Only what is actually on the target**, found by looking inside the
            // manager's folders rather than at the top of them: it keeps
            // `payloads/<name>/<name>_<version>.elf`, so a scan of the top level finds
            // almost nothing and the list would offer almost nothing.
            // **Three states again, and the middle one used to be invisible.** A scan that
            // has not come back and a scan that found nothing both drew an empty dropdown, so
            // "the target has no payloads" and "nobody has looked" were the same picture.
            let scanned = self.state.payloads_there.clone();
            let there = scanned.clone().unwrap_or_default();
            let known = !there.is_empty();
            egui::ComboBox::from_id_salt("add-payload")
                .selected_text(match &scanned {
                    None => "add... (looking)".to_owned(),
                    Some(found) if found.is_empty() => "add... (none found)".to_owned(),
                    Some(found) => format!("add... ({} on the target)", found.len()),
                })
                .show_ui(ui, |ui| {
                    match &scanned {
                        None => {
                            ui.weak("the target has not been asked yet - this fills in on connect");
                        }
                        Some(found) if found.is_empty() => {
                            ui.weak(format!("no .elf files under {PAYLOADS}"));
                        }
                        Some(_) => {}
                    }
                    // **The bare name, not the path.** The startup list names a filename and
                    // lets the manager resolve it; writing a path in would be writing a
                    // different file from the one that was reviewed.
                    //
                    // **Tagged, and the unreachable ones cannot be picked at all.** The manager
                    // lists payloads it can never resolve - anything on a stick outside its own
                    // folder - so offering them would be offering a way to build a list with an
                    // entry that fails at every boot. See `pros_core::payloads::Where`.
                    for one in there {
                        // Adding needs somewhere to save it, and a read-only list has none.
                        let usable = editable && one.storage.can_autoload();
                        let label = format!("{}   [{}]", one.name, one.storage.tag());
                        if ui
                            .add_enabled(usable, egui::SelectableLabel::new(false, label))
                            .on_hover_text(format!("{}\n{}", one.path, one.storage.means()))
                            .on_disabled_hover_text(one.storage.means())
                            .clicked()
                        {
                            act = Some(Box::new(move |boot| boot.add(&one.name)));
                        }
                    }
                });
            ui.weak(if known {
                "adds to the end - reorder it from there"
            } else {
                // The old text sent somebody to the payloads section to make this work. That
                // stopped being true when the scan moved to connect, and advice that is no
                // longer needed is advice that sends people somewhere for nothing.
                "only what is on the target can be started at boot"
            });
        });
        act
    }

    /// The manager's settings, under the list they belong with.
    fn settings_rows(&mut self, ui: &mut egui::Ui) {
        let Some(settings) = self.state.settings.clone() else {
            return;
        };
        // **Drawn from the pending edit when there is one, not from the file as read.**
        // The box redrew from the target's copy, so unticking one left it ticked - and
        // clicking it again asked for the same change rather than undoing it. There was no way
        // back except discarding everything.
        let pending = self
            .state
            .pending_change
            .clone()
            .filter(|change| change.into == pros_core::autoload::CONFIG);
        let shown = pending.as_ref().map_or_else(
            || settings.clone(),
            |change| pros_core::autoload::Settings::parse(&change.now),
        );
        ui.strong("settings");
        let mut change = None;
        let mut undo = false;
        egui::Grid::new("settings")
            .striped(true)
            .num_columns(2)
            .show(ui, |ui| {
                for (key, value) in shown.all() {
                    // A one-or-zero setting gets a switch; anything else is shown as it is,
                    // because guessing at the shape of a value nobody here has seen is how a
                    // config file gets rewritten into something the manager will not read.
                    if value == "0" || value == "1" {
                        let mut on = value == "1";
                        if ui.checkbox(&mut on, "").changed() {
                            let wanted = if on { "1" } else { "0" };
                            // Applied to what is pending, then diffed against what the target
                            // has - so setting a value back to the target's own clears the
                            // edit rather than recording a change to nothing.
                            let next = shown.set(key, wanted).map(|edit| edit.now);
                            match next {
                                Some(now) if now.trim() == settings.text().trim() => undo = true,
                                Some(now) => {
                                    change = Some(pros_core::autoload::Change {
                                        was: settings.text().to_owned(),
                                        now,
                                        what: format!("{key} = {wanted}"),
                                        into: pros_core::autoload::CONFIG,
                                    });
                                }
                                None => {}
                            }
                        }
                        let name = if settings.get(key) == Some(value.as_str()) {
                            egui::RichText::new(key)
                        } else {
                            // Changed and not written: marked here as well as in the panel
                            // below, because this is where somebody just clicked.
                            egui::RichText::new(key).color(egui::Color32::from_rgb(210, 190, 120))
                        };
                        ui.label(name);
                    } else {
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.label(key);
                            ui.weak(value);
                        });
                    }
                    ui.end_row();
                }
            });
        if undo {
            // Back to what the target has, so there is nothing to write and nothing to review.
            self.state.pending_change = None;
        } else if let Some(pending) = change {
            self.state.pending_change = Some(pending);
        }
    }

    /// A change waiting to be written, shown line by line.
    fn pending_write(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        let Some(change) = self.state.pending_change.clone() else {
            return;
        };
        // **A change to a list this will not write cannot exist**, and if one somehow did,
        // offering to write it would be the one thing promised not to happen.
        if change.into == pros_core::chain::PATH && !self.state.list().editable {
            self.state.pending_change = None;
            return;
        }
        ui.add_space(8.0);
        ui.separator();
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            format!("not written yet: {}", change.what),
        );
        ui.small("every change is marked in the list above, in the position it has now");
        // **No account of the change here any more.** Every added, removed and moved entry is
        // a marked row in the table above, in the position it has now - which is where
        // somebody is already looking. A second telling of the same change underneath is how
        // this panel came to describe a removal of something the table said was not there.
        //
        // The whole file, for anybody who wants to read what is actually going to be sent
        // rather than trust a summary of it.
        ui.collapsing("the file as it will be written", |ui| {
            for line in change.now.lines() {
                ui.monospace(line);
            }
        });
        let (grave, more) = self.write_hazards(ui, &change);
        self.write_buttons(ui, &change, grave, more, idle, connected);
    }

    /// What is wrong with the text about to be written, and what would answer it.
    ///
    /// **Audited on what is about to be written, not on what is there.** A warning about the
    /// current list is a warning about the past; this is the last moment at which the answer
    /// can still change what happens.
    ///
    /// Returns whether anything found is grave, and the edits that would put it right.
    fn write_hazards(
        &self,
        ui: &mut egui::Ui,
        change: &pros_core::autoload::Change,
    ) -> (bool, Vec<pros_core::recovery::Fix>) {
        let after = pros_core::chain::Chain::parse(&change.now);
        let hazards = pros_core::recovery::audit(
            &after,
            &self.catalogue,
            self.state.payloads_there.as_deref().unwrap_or_default(),
            pros_core::recovery::Kind::Manager,
            &self.chain_of_target(),
            self.loader_is_up(),
        );
        let grave = pros_core::recovery::is_dangerous(&hazards);
        if grave {
            ui.add_space(6.0);
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "WRITING THIS MAY LEAVE THE TARGET UNREACHABLE AT ITS NEXT RESTART",
            );
            for hazard in hazards
                .iter()
                .filter(|one| one.gravity() == pros_core::recovery::Gravity::Critical)
            {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), hazard.describe());
                ui.weak(hazard.remedy());
            }
        }
        // **Offered before the write, not instead of a warning.** A panel whose only action is
        // *write it anyway* has told somebody their configuration is broken and then handed
        // them the one button that keeps it broken.
        let repairs: Vec<pros_core::recovery::Fix> = hazards
            .iter()
            .filter_map(pros_core::recovery::Hazard::fix)
            .collect();
        (grave, if grave { repairs } else { Vec::new() })
    }

    /// The buttons under a pending write, and what was pressed.
    fn write_buttons(
        &mut self,
        ui: &mut egui::Ui,
        change: &pros_core::autoload::Change,
        grave: bool,
        mut more: Vec<pros_core::recovery::Fix>,
        idle: bool,
        connected: bool,
    ) {
        let mut fix_first: Vec<pros_core::recovery::Fix> = Vec::new();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            // **Named for what it does when it is dangerous.** A button that says the same
            // thing for a safe write and one that costs a jailbreak is a button that got
            // pressed the same way both times.
            if !more.is_empty()
                && ui
                    .button(format!("fix these {} first", more.len()))
                    .on_hover_text(
                        "make the edits that answer the findings above, and show the \
                         result here for review - still nothing written",
                    )
                    .clicked()
            {
                fix_first = std::mem::take(&mut more);
            }
            let label = if grave { "write it anyway" } else { "write it" };
            if ui
                .add_enabled(idle && connected, egui::Button::new(label))
                .on_hover_text("send this file to the target, replacing what is there")
                .on_disabled_hover_text("no target selected")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                self.state
                    .begin(Job::WriteAutoload(target, change.into, change.now.clone()));
                self.state.pending_change = None;
            }
            if ui.button("discard").clicked() {
                self.state.pending_change = None;
                // Re-read rather than trusting what is on screen: the checkbox was ticked, and
                // leaving it ticked over a discarded change would show a setting the target
                // does not have.
                if let Some(target) = self.state.target().cloned()
                    && idle
                {
                    self.state.begin(Job::ReadAutoload(target));
                }
            }
        });
        // After the panel, for the usual reason: applying borrows the state it was drawn from.
        if !fix_first.is_empty() {
            self.apply_fixes(&fix_first);
        }
    }

    /// What a delete would remove, before it removes it.
    ///
    /// # Why this one gets a list and the others get a sentence
    ///
    /// Every other confirm here names one thing. This can name fifty, and the number is
    /// exactly what somebody needs to check: a selection made across a fold, or left over from
    /// a listing that has since changed, is how the wrong thing gets deleted. **So it lists
    /// them, and it says which side they are on**, because *delete here* and *delete there*
    /// differ by a word and by everything else.
    fn pending_delete(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some((offer, what)) = self.state.pending_delete.clone() else {
            return;
        };
        let side = if offer == crate::listing::Offer::DeleteHere {
            self.state.local_path.clone()
        } else {
            self.state.library_path.clone()
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
        ui.small("nothing here undoes this");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(idle, egui::Button::new("delete"))
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                self.state.pending_delete = None;
                let names: Vec<String> = what.iter().map(|entry| entry.name.clone()).collect();
                for name in &names {
                    self.state.listing.chosen.remove(name);
                }
                if offer == crate::listing::Offer::DeleteHere {
                    let root = PathBuf::from(self.state.local_path.trim());
                    let paths = names.iter().map(|name| root.join(name)).collect();
                    self.state.begin(Job::DeleteHere(paths));
                } else if let Some(target) = self.state.target().cloned() {
                    let root = self.state.library_path.trim_end_matches('/').to_owned();
                    let paths = names.iter().map(|name| format!("{root}/{name}")).collect();
                    self.state.begin(Job::DeleteThere(target, paths));
                }
            }
            if ui.button("cancel").clicked() {
                self.state.pending_delete = None;
            }
        });
        ui.separator();
    }

    /// A file somebody dropped that nothing describes, and what can be done with it.
    ///
    /// # Why this is not just refused
    ///
    /// Debugging homebrew is a loop: build, run, read the log, change one line, build again.
    /// Making each turn of that loop require a manifest entry with a digest - of a file that
    /// will not exist in thirty seconds - is asking somebody to describe something in order to
    /// throw it away.
    ///
    /// So an undescribed file can be **run**, which is transient and leaves nothing behind,
    /// and it can be **kept**, which is not and therefore says so.
    fn adhoc(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.state.adhoc.clone() else {
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

        ui.add_space(6.0);
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            "nothing describes this file",
        );
        ui.monospace(path.display().to_string());
        match &shape {
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

        let runnable = shape.as_ref().is_ok_and(|shape| shape.is_payload());
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
                    .begin(Job::Send(target, name.clone(), path.clone()));
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
                    .and_then(|()| std::fs::copy(&path, into.join(&name)))
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
        if clear {
            self.state.adhoc = None;
        }
    }

    /// A package waiting to be installed, and the confirm in front of it.
    ///
    /// # Why installing gets a confirm and copying does not
    ///
    /// A copy puts a file somewhere. An install hands it to the target to unpack and register,
    /// and there is no button here that undoes it.
    ///
    /// **And nobody in this project has watched one succeed**, because finding out what
    /// success looks like means installing something on somebody's target. So the confirm
    /// names the file, and what comes back afterwards is reported in the target's own words
    /// rather than translated into a claim this cannot support.
    fn pending_install(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some(paths) = self.state.pending_install.clone() else {
            return;
        };
        if paths.is_empty() {
            self.state.pending_install = None;
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
        // **Every one named.** A count is not a list, and this is the panel somebody reads
        // before something that cannot be undone.
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
                self.state.pending_install = None;
                for path in &paths {
                    self.state
                        .queue(Job::InstallPackage(target.clone(), path.clone()));
                }
            }
            if ui.button("cancel").clicked() {
                self.state.pending_install = None;
            }
        });
        ui.separator();
    }

    /// A copy that was not attempted, why, and the one way past it.
    ///
    /// # Why this is a panel and not a greyed button
    ///
    /// The refusal happens after somebody has already asked, because whether a save needs
    /// re-signing depends on the target it is going to - which is not known until the moment
    /// of asking. So there is nothing to grey out beforehand, and the answer has to arrive
    /// where they are looking.
    ///
    /// **The override is offered and is not the default.** Somebody may know something this
    /// does not - that the accounts are the same despite the record, that they want the files
    /// there regardless - and refusing outright would make the tool the obstacle. What it will
    /// not do is copy first and let them find out later.
    fn refusal(&mut self, ui: &mut egui::Ui) {
        let Some(needs) = self.state.refused.clone() else {
            return;
        };
        let amber = egui::Color32::from_rgb(210, 190, 120);
        match &needs {
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
                let from = PathBuf::from(self.state.local_path.trim());
                let to = self.state.library_path.clone();
                self.state.refused = None;
                self.state.begin(Job::Restore(target, from, to, true));
            }
            if ui.button("leave it").clicked() {
                self.state.refused = None;
            }
        });
        ui.separator();
    }

    /// One button per place this section's things might live.
    ///
    /// **Because there is no standard, and the choice is the person's.** Three payloads keep
    /// cheats in three directories and the most-used cheat runner reads all of them, so which
    /// one is right depends on what somebody installed - and on which they prefer, when they
    /// have more than one.
    ///
    /// Each button says what the target said about that path, in three states rather than
    /// two: **here**, **not here**, or nothing at all. The third is not a gap. Probing stops
    /// at the first directory that answers, so anything after it was never asked about, and
    /// marking those absent would be inventing a measurement - the same rule the payload
    /// presence column follows.
    fn candidate_buttons(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        let candidates = self.state.section.candidates();
        if candidates.is_empty() {
            return;
        }
        let current = self.state.library_path.trim().to_owned();
        let mut go = None;
        ui.horizontal_wrapped(|ui| {
            ui.small("keep them in:");
            for place in candidates {
                let path = place.path;
                let known = self
                    .state
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
                // **The label says what the place is**, and the path is in the hover under it.
                // A button captioned with a fragment of its own path - *homebrew*, *pkg* -
                // offers a choice nobody can make, which is what these used to do.
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
            self.state.library_path = path;
            // Listed straight away rather than waiting for a refresh press: choosing a place
            // is asking what is in it, and a stale listing under a new path is the worst of
            // both - it looks like an answer about somewhere it is not.
            if connected {
                self.browse();
            }
        }
    }

    /// The right half: what is on the target.
    fn there_side(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        self.there_toolbar(ui, idle, connected);
        ui.add(
            egui::TextEdit::singleline(&mut self.state.library_path).desired_width(f32::INFINITY),
        );
        self.candidate_buttons(ui, idle, connected);
        self.locate_notice(ui);
        ui.separator();

        let section = self.state.section.name();
        let looking_at = self.state.library_path.clone();
        let rows: Vec<crate::listing::Entry> = self
            .state
            .listing
            .entries
            .iter()
            .filter(|entry| entry.there.is_some())
            .cloned()
            .collect();
        if rows.is_empty() {
            ui.weak("nothing listed here");
        }
        let mut toggled = None;
        let mut entered = None;
        let mut going_up = false;
        let mut folds = None;
        egui::Grid::new(format!("{section}-there"))
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                headings(ui, &["", "name", "size"]);
                if up_row(ui, &looking_at) {
                    going_up = true;
                }
                let (folders, files): (Vec<_>, Vec<_>) =
                    rows.iter().partition(|entry| entry.folder_there());
                for (label, group) in [("folders", &folders), ("files", &files)] {
                    if group.is_empty() {
                        continue;
                    }
                    let key = format!("{section}-there-{label}");
                    if group_row(ui, &self.state.folded, &key, label, group.len(), &mut folds) {
                        continue;
                    }
                    for entry in group {
                        // **Which of the two happened is the row's answer, not a guess from
                        // what kind of thing it is.** Deciding here meant a tick on a folder
                        // navigated instead of selecting, which made folders unselectable.
                        let known = self.state.names.get(&entry.name);
                        match listing_row(ui, entry, &self.state.listing.chosen, true, known) {
                            Some(Hit::Open) => entered = Some(entry.name.clone()),
                            Some(Hit::Tick) => toggled = Some(entry.name.clone()),
                            None => {}
                        }
                    }
                }
            });
        if let Some(name) = toggled {
            self.state.listing.toggle(&name);
        }
        // **The selection does not follow you between directories.** A tick is a name, and a
        // name means a different file in a different folder - including the folder just
        // double-clicked, which the first half of that double click ticked on the way in.
        if let Some(name) = entered {
            self.state.library_path =
                format!("{}/{name}", self.state.library_path.trim_end_matches('/'));
            self.state.listing.chosen.clear();
            self.browse();
        } else if going_up && let Some(above) = parent_of(&self.state.library_path) {
            self.state.library_path = above;
            self.state.listing.chosen.clear();
            self.browse();
        }
        fold(&mut self.state.folded, folds);
    }

    /// One list, with a column for each side.
    ///
    /// **The model drawn plainly.** The split panes are two filtered views of exactly this,
    /// which is why switching between them changes nothing about what is true - only how much
    /// of it is on screen at once.
    fn merged_view(&mut self, ui: &mut egui::Ui) {
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

                        for entry in &self.state.listing.entries {
                            let mut ticked = self.state.listing.chosen.contains(&entry.name);
                            if ui.checkbox(&mut ticked, "").changed() {
                                toggled = Some(entry.name.clone());
                            }
                            ui.label(&entry.name);
                            // **A column per side, each saying what that side has.** A tick
                            // in both is a thing in sync; one side filled and the other empty
                            // is the difference somebody opened this to find.
                            side_cell(ui, entry.here);
                            side_cell(ui, entry.there);
                            let (word, colour) = standing_of(entry);
                            ui.colored_label(colour, word);
                            ui.end_row();
                        }
                    });
                if self.state.listing.entries.is_empty() {
                    ui.weak("nothing on either side, and nothing described");
                }
            });
        if let Some(name) = toggled {
            self.state.listing.toggle(&name);
        }
    }

    /// The notice about where a section's things live, when the target has none of them.
    fn locate_notice(&mut self, ui: &mut egui::Ui) {
        if let Some((asked, pros_core::locate::Where::NoneOfThem(tried))) = &self.state.located
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
    /// **Copied rather than referenced.** The list is a listing of one folder, and an entry
    /// pointing somewhere else would vanish from it the moment somebody moved the original -
    /// with the row still there, still offering to send it.
    fn add_files(&mut self) {
        let Some(chosen) = choose_files(&self.state.local_path) else {
            return;
        };
        let into = PathBuf::from(self.state.local_path.trim());
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
    /// **Not on the worker.** It starts a program and returns; there is nothing to wait for
    /// and nothing to report but whether it started, so putting it through the one-job-at-a-
    /// time rule would make it queue behind a copy for no reason.
    fn reveal(&mut self, path: &Path) {
        match pros_core::reveal::folder(path) {
            Ok(()) => self.state.said = path.display().to_string(),
            Err(why) => self.state.trouble = Some(why),
        }
    }

    /// Lists whatever the library path currently is.
    fn browse(&mut self) {
        let where_to = self.state.library_path.clone();
        // **Already read this session, so not read again.** Six sections share one listing
        // slot, and without this, moving between them re-fetched what had just been fetched.
        // Cleared whenever a job reports it changed the target - see `Disturbs::There`.
        if let Some(known) = self.state.seen.get(&where_to) {
            self.state.library = known.clone();
            return;
        }
        if let Some(target) = self.state.target().cloned() {
            self.state.begin(Job::Browse(target, where_to));
        }
    }

    /// What is described, what can be trusted, and what is on the target.
    fn payloads_body(&mut self, ui: &mut egui::Ui) {
        // **The same two panes as every other section.** This used to be one wide table with
        // a toolbar of its own, which made it read as a different kind of screen - and it is
        // not: it is here and there, like the rest. What differs is only that the left side
        // knows more about its files than a directory listing can.
        //
        // The manifest path is no longer written across the top either. Every other section
        // reads a list from disk without announcing where the file is; payloads doing so made
        // the file look like something to manage rather than something to edit. It is in the
        // payloads menu, with the other things that act on it.
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();

        section_heading(ui, Section::Payloads);

        // The same toolbar as every other section, over the same listing. What differs is
        // only the left pane, which shows what a directory listing cannot.
        self.rebuild_listing();
        self.sync_toolbar(ui);
        ui.separator();
        self.refusal(ui);
        self.pending_install(ui, idle);
        self.pending_delete(ui, idle);
        self.adhoc(ui);

        if self.state.merged {
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
    /// **Not a directory listing, which is why this section keeps its own table.** A payload
    /// has a digest, a place in the boot order and a service that either answers or does not -
    /// none of which a file listing can show, and all of which are the reason somebody came.
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
                .button("run a file...")
                .on_hover_text("choose an ELF anywhere on this machine and run it")
                .clicked()
            {
                // **Made before the dialog opens, not waited for.** `rfd` ignores a directory
                // that is not there and opens wherever it last was, so on a machine where
                // nothing has been downloaded yet this asked for an ELF somewhere arbitrary -
                // while the folder it meant is the one every other control in this pane reads.
                let from = PathBuf::from(self.state.local_path.trim());
                if !from.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(&from);
                }
                if let Some(chosen) =
                    choose_a_file("an ELF to run", &["elf"], &self.state.local_path)
                {
                    self.state.adhoc = Some(chosen);
                }
            }
            if ui
                .button("open folder")
                .on_hover_text("show it in this machine's file browser")
                .clicked()
            {
                // **The folder this pane actually uses.** It opened the staging directory -
                // `cache_directory()/payloads` - under a comment saying that is where the
                // table sends from. It is not: the row action just below sends from
                // `local_path`, which is `data_root()/payloads`, and so does the toolbar, and
                // so does what a download is written into. Somebody checking why `run` was
                // greyed was being shown a different directory from the one being judged.
                let path = PathBuf::from(self.state.local_path.trim());
                if !path.as_os_str().is_empty() {
                    let _ = std::fs::create_dir_all(&path);
                    self.reveal(&path);
                }
            }
            self.sources_control(ui);
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
            ui.text_edit_singleline(&mut self.state.install_dir);
        });
        ui.separator();

        self.payload_rows(ui);
    }

    /// What a toolbar button says, given what is selected.
    ///
    /// **Only one offer changes its word**, and it changes it on a measurement rather than a
    /// mood: *download* becomes *update* when every selected payload already has an older copy
    /// of itself on this disk. Getting a file for the first time and replacing one you have are
    /// different acts, and a button that says the first while doing the second is describing
    /// half of what it is about to do.
    fn says(&self, offer: crate::listing::Offer) -> &'static str {
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
                self.state.listing.chosen.contains(key)
            })
            .collect();
        if picked.is_empty() {
            return offer.label();
        }
        // Every one of them, not any: a mixed selection is doing both, and *download* is the
        // word that covers both without claiming the wrong one.
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
    /// # Why it says when, and not just what
    ///
    /// The answers are cached for hours on purpose - sixty requests an hour is not much spread
    /// across thirty projects - so what is on screen is a measurement from some time ago. A
    /// column that showed the age of its own evidence nowhere would be asking to be trusted
    /// about the present on the strength of the past, which is the failure this whole column
    /// was added to catch.
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
        // **The age of the evidence, beside the button that renews it.**
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
        // **Nothing on this machine says so once, rather than as thirty dead buttons.**
        //
        // `library::here` reports a folder that does not exist as an empty one, which is right
        // - it is not a failure - but it left the table with a `run` on every row, every one of
        // them greyed, and the only explanation on a hover. A first run has downloaded nothing,
        // so that is the normal state and it deserves a sentence rather than a puzzle.
        if self.state.local.is_empty() {
            ui.add_space(4.0);
            ui.small("nothing on this machine yet - download or fetch one, and run turns on");
        }
        let rows = pros_core::payloads::survey(
            manifest,
            self.state.report.as_ref(),
            self.state.chain.as_ref(),
        );
        // **The category is a column, not a heading somebody can fold away.**
        //
        // It was a foldable heading per group, which cost more than it looked. A folded group
        // is still in `Listing::build`, so *all* ticked rows that were not on screen and the
        // toolbar above then ran, sent or installed things nobody could see - the same defect
        // as everywhere else here, a control whose result is invisible either way. Only
        // `delete` defended against it, by naming every file in its confirmation.
        //
        // A column cannot hide a row. It also sorts, stays readable when the heading has
        // scrolled off, and matches the shape of the pane beside it, which is flat.
        //
        // Drawn only when it changes from the row above, so the eye still gets the four blocks
        // the headings gave it while every row remains present, selectable and counted.
        let chosen = self.state.listing.chosen.clone();
        let on_target = self.state.payloads_there.clone().unwrap_or_default();
        let on_target = on_target.as_slice();
        let mut asked = Wanted::default();
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
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
                        here: &self.state.local,
                        idle,
                        connected,
                    },
                    &mut asked,
                );
            }
        });
        if let Some(name) = asked.ticked {
            self.state.listing.toggle(&name);
        }
        // **From the row, not from the selection.** The toolbar above acts on what is ticked,
        // and a tick is one key shared by two panes over three spellings of one payload - the
        // manifest's filename, whatever the file on this disk is called, and the directory the
        // manager keeps it in. Every one of those is a separate row, so ticking the payload
        // somebody was looking at could leave the toolbar acting on a row that has no local
        // copy, or none at all. This row knows which file it is; it does not have to be told.
        if let Some(file) = asked.run
            && let Some(target) = self.state.target().cloned()
        {
            let from = PathBuf::from(self.state.local_path.trim()).join(&file);
            self.state.begin(Job::Send(target, file, from));
        }
        // After the grid, for the usual reason: starting a job borrows what it was drawn from.
        if let Some(name) = asked.relist
            && let Some(payload) = self.described_as(&name)
        {
            self.state.begin(Job::Relist(Box::new(payload)));
        }
    }

    /// One category's worth of rows.
    ///
    /// Taken out of the body because the body now draws several of these, and a function
    /// that draws one thing several times is clearer than a loop inside a loop inside a
    /// panel.
    fn payload_group(ui: &mut egui::Ui, group: &str, what: &Shown<'_>, asked: &mut Wanted) {
        let Shown {
            rows,
            on_target,
            sources,
            chosen,
            here,
            idle,
            connected,
        } = *what;
        // No grid of its own: it draws into the caller's, which is what keeps every group's
        // columns in line with every other group's.
        for (at, row) in rows.iter().enumerate() {
            // **Worked out first, drawn second.** Every cell below is one line, so the order
            // of the columns is a list that can be read and rearranged - rather than an order
            // that emerges from where each calculation happened to sit.
            let (mark, colour, hover) = Self::running_of(row.presence);
            let (boot, boot_hover) = Self::boot_of(row.boot);
            let (there, there_colour, there_hover) = Self::on_target_of(row, on_target);
            let stale = pros_core::sources::against(row.payload, sources.get(&row.payload.name))
                .is_behind();
            let (listed, listed_colour, listed_hover) = Self::listed_of(row.payload, sources);
            let (bytes, size_hover) = Self::size_of(row.payload);

            // **The same tick as every other listing.** This table was the one pane with no
            // way to select anything, which made the toolbar above it useless here - it acts
            // on a selection, and there was no way to make one.
            //
            // Keyed by filename, because that is what the listing calls an entry: the display
            // name is often something else, and ticking one thing under two keys would put it
            // in a selection twice or in neither.
            let key = row
                .payload
                .filename
                .clone()
                .unwrap_or_else(|| row.payload.name.clone());
            let mut on = chosen.contains(&key);
            if ui.checkbox(&mut on, "").changed() {
                asked.ticked = Some(key.clone());
            }
            // **Run, on the row, for the payload the row is about.** A folder on the target is
            // how the manager stores every payload it has, and it says nothing about whether
            // the file on this machine can be sent - which is the only file this reads.
            let held = what_is_here(here, &key);
            if ui
                .add_enabled(
                    idle && connected && held,
                    egui::Button::new("run").small(),
                )
                .on_hover_text(
                    "send this file to the loader and start it now - it lives in memory until \
                     the next restart, and nothing is written to the target's disk. A payload \
                     that loads others will load them all again.",
                )
                .on_disabled_hover_text(if !connected {
                    "no target selected"
                } else if held {
                    "wait for what is already running"
                } else {
                    "not on this machine - download it first, and this can send it"
                })
                .clicked()
            {
                asked.run = Some(key);
            }
            ui.label(&row.payload.name);
            ui.weak(bytes).on_hover_text(size_hover);
            ui.colored_label(colour, mark).on_hover_text(hover);
            ui.colored_label(listed_colour, listed)
                .on_hover_text(listed_hover);
            // **The action for a stale *list*, which is not the action for a stale payload.**
            //
            // `download` fetches what the list points at, and when the list is behind that is
            // the old version - so calling it *update* there would promise the new one and
            // hand over the old. What this row needs is the entry repointed, and that is a
            // different act with a different consequence: it downloads to learn a digest
            // nobody can supply, which is the one moment this program takes something on
            // trust.
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
            // **Strong once per run, then dim - rather than blank after the first.**
            //
            // Blank cells read as a group and cost nothing, which is the usual answer, and it
            // was the first one here. It is wrong for this table: half the reason the heading
            // became a column is that a heading scrolled off the top leaves a row that cannot
            // say what it is, and a blank cell leaves exactly the same row. A hover does not
            // save it either - an empty label has no width to hover.
            //
            // So every row states its group, and the first of each run states it louder. The
            // eye still gets the blocks; no row is silent about which one it is in.
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
    /// # Why this is not the size in the description
    ///
    /// **It is the file on this disk, measured.** A description can carry a size, and printing
    /// that would put a number in the same column as the measured ones for a payload that is
    /// not here at all - a row claiming to know the size of a file nobody has. The pane beside
    /// this one lists what the target holds; this one lists what this machine holds, and an
    /// entry that is only described holds nothing.
    ///
    /// Three answers, for the usual reason: staged and measured, staged and unreadable, and
    /// not staged. The middle one is a real state - a file being written, or one whose
    /// permissions changed - and it must not read as either of the others.
    fn size_of(payload: &pros_core::manifest::Payload) -> (String, String) {
        let Some(path) = pros_core::staging::path_for(payload) else {
            return (
                "-".to_owned(),
                "the description names no file, so there is nothing to have here".to_owned(),
            );
        };
        match std::fs::metadata(&path) {
            Ok(about) => (size(about.len()), path.display().to_string()),
            // Told apart on purpose: `NotFound` is the ordinary case of a payload nobody has
            // fetched, and anything else is a file that is there and could not be read.
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
    /// A payload nothing here can see is not a payload that is absent, and putting it in the
    /// same column as the ones that were measured would have it believed.
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
    /// **A second question from whether it is running**, with an answer that looks the same. A
    /// service can be answering now and absent from the list, which means it is there until
    /// somebody turns the target off - usually the finding somebody actually needed.
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

    /// The version **the list** describes, coloured against what the project has released.
    ///
    /// # Why this column was plain until now
    ///
    /// It was the yardstick the target column is measured against, and colouring a yardstick
    /// begs the question *against what*. There is an answer now, and it is a fourth version:
    /// what the project itself has released. Only that makes a colour here mean anything.
    ///
    /// **Grey is not a pass.** A project nobody has asked about and a project whose list entry
    /// matches its latest release are drawn differently on purpose - a payload list is wrong
    /// silently, which is exactly the failure that had this program fetching a dead mirror.
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

    /// The version **the console** has, coloured against the one the list describes.
    ///
    /// # Why this is its own column rather than an arrow
    ///
    /// Three versions exist for every row - what the list describes, what is staged on this
    /// machine, and what the target holds - and the column that showed `v0.24 -> v0.25` was
    /// the last two collapsed into one cell. An arrow reads as a transition, and with three
    /// candidates for each end nobody could tell which pair it meant: an update waiting to be
    /// downloaded, or an update waiting to be sent. Both are real, and they need different
    /// buttons.
    ///
    /// So each machine gets a column and says only what it knows. `size` answers *is it on
    /// this machine*, this answers *what is on the target*, and the plain `version` beside it
    /// is what the list describes. Nothing points at anything.
    ///
    /// Read from the sidecar the manager writes beside each payload - the file itself carries
    /// no version, so that is the only thing on the target that knows. **Absent is drawn as
    /// absent, never as out of date**, and *there and unversioned* is drawn as neither.
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
            // **Amber, not green.** Two versions that cannot be ordered are still two
            // different things, and drawing it as up to date would be a guess dressed as a
            // measurement.
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
            // **On the target, and it will not say which build.** Distinct from not being
            // there at all, because one of those is answered by sending a payload and the
            // other by the manager writing a sidecar it did not write.
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

    /// Writes the player command file, so a person meeting a disabled button knows what to
    /// write and where.
    fn write_player_example(&mut self) {
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

    /// The sidebar: which target, watching it, and what to do with it.
    ///
    /// **Interaction only.** Registering moved to the menu and to the bottom of the target
    /// list, because it is a thing done once and a form for it sitting here is a form in the
    /// way of the work.
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let chosen = self
            .state
            .target()
            .map_or_else(|| "no target".to_owned(), |target| target.name.clone());
        egui::ComboBox::from_id_salt("target")
            .selected_text(chosen)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for which in 0..self.state.targets.len() {
                    let label = self.state.targets[which].name.clone();
                    ui.selectable_value(&mut self.state.chosen, Some(which), label);
                }
                // At the bottom of the list somebody is already looking at when they
                // discover the one they want is not in it.
                ui.separator();
                if ui.button("register...").clicked() {
                    self.state.showing.registering = true;
                }
            });

        ui.add_space(8.0);

        for (group, sections) in Section::GROUPS {
            ui.add_space(4.0);
            ui.small(group);
            ui.separator();
            for section in sections {
                ui.selectable_value(&mut self.state.section, *section, section.name());
            }
        }
    }

    /// What the target can currently do.
    fn check_panel(&mut self, ui: &mut egui::Ui) {
        if self.state.target().is_none() {
            section_heading(ui, Section::Check);
            ui.label("no target selected");
            ui.small("target -> register..., or pick one from the list above");
            return;
        }
        section_heading_with(ui, Section::Check, |ui| {
            if ui
                .add_enabled(self.state.is_idle(), egui::Button::new("check again"))
                .on_hover_text("ask the target what it can do right now")
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
                && let Some(target) = self.state.target().cloned()
            {
                let started = self.state.begin(Job::Check(target));
                debug_assert!(started, "a job started while the button was disabled");
            }
            // **Beside the check, because it answers what the check found.** This screen is
            // where somebody learns their console will not come back; the thing they want next
            // is not a line edit on another screen, it is a working chain. Editing one entry at
            // a time is still right when there is a working chain to change, and that stays
            // where the list is.
            if ui
                .add_enabled(self.state.is_idle(), egui::Button::new("deploy chain..."))
                .on_hover_text(
                    "set this target up from nothing: pick a chain and where it goes, read \
                     what you would end up with, then agree to it. Nothing happens before \
                     you do.",
                )
                .on_disabled_hover_text("wait for what is already running")
                .clicked()
            {
                self.state.setting_up = Some(self.state.list_at);
            }
        });

        // **Before the table, not after it.** What is answering now is the smaller question;
        // whether the machine survives its next restart is the one that costs a jailbreak, and
        // a finding put below six rows of green is a finding nobody reads. It is also drawn
        // before the report exists, because *nothing was measured* is itself a finding.
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        // Above the findings: it is the answer to most of them, and a person who has decided
        // to deploy should not have to read past six rows to get at it.
        self.configurator(ui, idle);
        self.doctor_panel(ui, idle);
        self.plan_panel(ui, idle, connected);
        ui.add_space(8.0);
        ui.separator();

        let Some(report) = &self.state.report else {
            ui.label("not asked yet - a target's capabilities are not remembered between");
            ui.label("runs, because a jailbreak does not survive a power cycle");
            return;
        };

        egui::Grid::new("services").striped(true).show(ui, |ui| {
            for finding in &report.findings {
                let (mark, colour) = if finding.reachability.open {
                    ("up", egui::Color32::from_rgb(120, 190, 120))
                } else if finding.service.required {
                    ("DOWN", egui::Color32::from_rgb(220, 120, 120))
                } else {
                    ("--", egui::Color32::GRAY)
                };
                ui.colored_label(colour, mark);
                ui.label(finding.service.name.as_ref());
                ui.label(format!(":{}", finding.service.port));
                // The third column is the point of having a table: a port number is not a
                // capability, and a reader told what it buys has been told something useful.
                ui.label(finding.service.unlocks.as_ref());
                ui.label(if finding.was_slow() {
                    format!("{}ms", finding.reachability.took.as_millis())
                } else {
                    String::new()
                });
                ui.end_row();
            }
        });

        ui.add_space(8.0);
        let verdict = report.verdict();
        let colour = match verdict {
            Verdict::Ready => egui::Color32::from_rgb(120, 190, 120),
            Verdict::Dimmed { .. } => egui::Color32::from_rgb(210, 190, 120),
            Verdict::Blocked { .. } => egui::Color32::from_rgb(220, 120, 120),
        };
        ui.colored_label(colour, say_verdict(&verdict));
    }

    /// Everything the doctor is allowed to look at, borrowed from what is already known.
    ///
    /// **One place builds it**, so a plan somebody chooses from a list of options cannot be
    /// built against a different picture from the one that offered the options.
    fn with_known<T>(&self, act: impl FnOnce(&pros_core::doctor::Known<'_>) -> T) -> T {
        // The check reads the manager's own list beside its probe, and that is what the check
        // screen audits. The startup list screen asks about whichever list it is showing.
        self.with_known_of(
            self.state.chain.as_ref(),
            pros_core::recovery::Kind::Manager,
            act,
        )
    }

    /// Whether the loader is answering, as far as the last check knows.
    ///
    /// **`None` is nobody asked**, which the audit treats as *not answering* for the one rule
    /// it decides - listing the loader is a choice worth offering unless a copy is demonstrably
    /// already holding the port.
    fn loader_is_up(&self) -> Option<bool> {
        let report = self.state.report.as_ref()?;
        let loader = report.about(pros_link::service::LOADER.name.as_ref())?;
        Some(loader.reachability.open)
    }

    /// Which chain this target is meant to be running.
    ///
    /// **The target's own answer, when it has one.** A console set up with etaHEN is not
    /// missing an FTP server, and the only thing that knows which it is, is the registration.
    /// Nobody having said falls back to the first shipped chain, which is an answer rather than
    /// a choice - and it is why this is asked in one place instead of defaulted in several.
    fn chain_of_target(&self) -> pros_core::recovery::baseline::Preset {
        self.state
            .target()
            .and_then(|target| target.chain.as_deref())
            .and_then(pros_core::recovery::baseline::named)
            .unwrap_or_else(pros_core::recovery::baseline::first)
    }

    /// The same, about a named list rather than the one the check happened to read.
    fn with_known_of<T>(
        &self,
        chain: Option<&pros_core::chain::Chain>,
        kind: pros_core::recovery::Kind,
        act: impl FnOnce(&pros_core::doctor::Known<'_>) -> T,
    ) -> T {
        let preset = self.chain_of_target();
        let nothing = Manifest::default();
        let described = self.manifest.as_ref().unwrap_or(&nothing);
        let staged: Vec<String> = described
            .payloads()
            .iter()
            .filter(|one| pros_core::staging::is_staged(one))
            .map(|one| one.name.clone())
            .collect();
        act(&pros_core::doctor::Known {
            report: self.state.report.as_ref(),
            // **Passed as it is, absent and all.** A target whose payloads have not been
            // listed is not a target with none, and flattening the two here would let a plan
            // propose downloading a file that is already on the drive.
            there: self.state.payloads_there.as_deref(),
            staged: &staged,
            described,
            chain,
            kind,
            preset: &preset,
            known: &self.catalogue,
        })
    }

    /// Every check, worst first, each with the one action that answers it.
    ///
    /// # Why this replaced two screens worth of buttons
    ///
    /// A payload that was missing used to be met with *download* here, *send* here again once
    /// it arrived, and then nothing at all - because neither button put it in a startup list,
    /// and the screen that did refused unless the file was already on internal storage. Three
    /// presses across two screens, and the last one was never offered.
    fn doctor_panel(&mut self, ui: &mut egui::Ui, idle: bool) {
        use pros_core::doctor::{Health, Remedy, Verdict, health};

        let findings = self.with_known(pros_core::doctor::examine);
        self.verify_if_due(&findings, idle);

        let light = health(&findings);
        let (word, colour) = match light {
            Health::Well => ("all well", egui::Color32::from_rgb(120, 190, 120)),
            Health::Unknown => ("not measured", egui::Color32::GRAY),
            Health::Warning => (
                "something is missing",
                egui::Color32::from_rgb(210, 190, 120),
            ),
            Health::Unwell => (
                "THIS TARGET MAY NOT COME BACK AFTER A RESTART",
                egui::Color32::from_rgb(230, 90, 90),
            ),
        };
        let mut asked: Option<crate::state::Pending> = None;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.colored_label(colour, word);
            // **Restored, and as one plan rather than a batch of actions.**
            //
            // The panel this replaced had a button that applied every hazard's edit at once,
            // and losing it was a real step backwards: a target with four things wrong made
            // somebody press four buttons and confirm four times, which is how the fourth one
            // stops being read.
            //
            // It goes through the same confirmation as a single fix, showing the combined
            // steps, because the promise is that nothing happens until a plan has been read -
            // and a *fix everything* that skipped that would be the one place the promise did
            // not hold, on the button most likely to be pressed in a hurry.
            let together: Vec<pros_core::doctor::Plan> = findings
                .iter()
                .filter_map(|finding| match &finding.verdict {
                    Verdict::Unwell {
                        remedy: Remedy::Ready(plan),
                        ..
                    } if !plan.is_settled() => Some(plan.clone()),
                    _ => None,
                })
                .collect();
            if together.len() > 1 {
                let plan = pros_core::doctor::Plan::all_of(&together);
                let steps = plan.outstanding().len();
                if ui
                    .add_enabled(
                        idle,
                        egui::Button::new(format!("fix all {}", together.len())),
                    )
                    .on_hover_text(format!(
                        "show what answering all of them takes - {steps} steps, and nothing                          happens yet"
                    ))
                    .on_disabled_hover_text("wait for what is already running")
                    .clicked()
                {
                    asked = Some(crate::state::Pending {
                        id: "everything".to_owned(),
                        label: format!("all {} of these are answered", together.len()),
                        plan,
                    });
                }
            }
        });

        let mut choose: Option<(String, String)> = None;
        egui::Grid::new("doctor")
            .striped(true)
            .num_columns(4)
            .show(ui, |ui| {
                headings(ui, &["", "check", "what was found", ""]);
                for finding in &findings {
                    let (mark, colour) = mark_of(&finding.verdict, finding.gravity);
                    ui.colored_label(colour, mark);
                    ui.label(&finding.label);
                    ui.label(finding.verdict.describe());
                    match doctor_action(ui, finding, idle) {
                        Some(Asked::Plan(one)) => asked = Some(one),
                        Some(Asked::Chose(id, name)) => choose = Some((id, name)),
                        None => {}
                    }
                    ui.end_row();
                }
            });

        // After the grid, for the usual reason: both of these borrow what it was drawn from.
        if let Some((id, name)) = choose
            && let Remedy::Ready(plan) =
                self.with_known(|known| pros_core::doctor::plan_for(known, &name))
        {
            asked = Some(crate::state::Pending {
                id,
                label: format!("{name} is in the startup list"),
                plan,
            });
        }
        if let Some(one) = asked {
            self.state.pending_plan = Some(one);
        }
    }

    /// The plan, in full, and the only place in this program where one is agreed to.
    ///
    /// **Everything above this is a suggestion.** A plan reaches the job queue through one
    /// button, having been drawn out step by step first - including the steps that are already
    /// done, so the shape of the whole job is visible rather than just its remainder.
    fn plan_panel(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        let Some(pending) = self.state.pending_plan.clone() else {
            return;
        };
        ui.add_space(8.0);
        ui.separator();
        ui.colored_label(
            egui::Color32::from_rgb(210, 190, 120),
            format!("to make sure {}:", pending.label),
        );
        ui.weak(&pending.plan.because);
        ui.add_space(4.0);

        for (at, one) in pending.plan.moves.iter().enumerate() {
            let step = format!("{}. {}", at + 1, one.step.describe());
            if one.already {
                ui.weak(format!("{step}  - already done, so it is skipped"));
            } else {
                ui.label(step);
            }
        }

        if pending.plan.rewrites_the_list() {
            ui.add_space(4.0);
            ui.colored_label(
                egui::Color32::from_rgb(210, 190, 120),
                "the startup list is not written here: the edit is prepared and opened for \
                 review on the startup list screen, where the whole file is shown and saved",
            );
        }

        ui.add_space(6.0);
        let outstanding = pending.plan.outstanding().len();
        let mut go = false;
        let mut drop_it = false;
        ui.horizontal(|ui| {
            let can = idle && (connected || !pending.plan.touches_the_target());
            if ui
                .add_enabled(can, egui::Button::new(format!("do these {outstanding}")))
                .on_hover_text("carry out the steps above, in order")
                .on_disabled_hover_text(if connected {
                    "wait for what is already running"
                } else {
                    "no target selected"
                })
                .clicked()
            {
                go = true;
            }
            if ui.button("not now").clicked() {
                drop_it = true;
            }
        });

        if drop_it {
            self.state.pending_plan = None;
        }
        if go {
            self.carry_out(&pending);
        }
    }

    /// Turns an agreed plan into queued jobs, in order.
    ///
    /// **The list edits are applied here but not written.** They change nothing outside this
    /// program until somebody saves the file on the startup list screen, which is the one place
    /// this project writes a startup list and the only place it shows the whole file first.
    /// What the transfers do is make that edit a valid one rather than a reference to a file
    /// the manager cannot resolve.
    fn carry_out(&mut self, pending: &crate::state::Pending) {
        use pros_core::doctor::Step;

        let Some(target) = self.state.target().cloned() else {
            self.state.trouble = Some("no target selected".to_owned());
            return;
        };
        let mut queued = 0_usize;
        let mut edited = 0_usize;
        let mut could_not: Vec<String> = Vec::new();

        for one in &pending.plan.moves {
            if one.already {
                continue;
            }
            match &one.step {
                Step::Fetch { payload } => {
                    if let Some(described) = self.described_as(payload) {
                        self.state.queue(Job::Fetch(Box::new(described), None));
                        queued += 1;
                    } else {
                        could_not.push(format!("{payload} has no description to fetch from"));
                    }
                }
                Step::Bring { payload, from } => {
                    if let Some(mut into) = pros_core::manifest::staging() {
                        into.push(from.rsplit('/').next().unwrap_or(payload.as_str()));
                        self.state
                            .queue(Job::Pull(target.clone(), from.clone(), into));
                        queued += 1;
                    } else {
                        could_not.push("there is nowhere on this machine to stage it".to_owned());
                    }
                }
                // **`Job::Install`, never `Job::Send`.** This was the bug that made the
                // whole feature do nothing: `Job::Send` hands the ELF to the loader and starts
                // it in memory, writing not one byte to the disk. So a plan reading *send
                // pldmgr to /data/pldmgr/payloads* ran pldmgr - which was already running -
                // left the directory exactly as it was, and the finding it was answering
                // stayed on screen with no way to tell that anything had gone wrong.
                Step::Send { payload, to } => match self.described_as(payload) {
                    Some(described) => match pros_core::staging::path_for(&described) {
                        Some(path) => {
                            self.state.queue(Job::Install(
                                target.clone(),
                                Box::new(described),
                                path,
                                (*to).to_owned(),
                            ));
                            queued += 1;
                        }
                        None => could_not.push(format!("{payload} has no filename to stage under")),
                    },
                    None => could_not.push(format!(
                        "{payload} is not described, so it cannot be laid out"
                    )),
                },
                Step::Run { path } => {
                    self.state
                        .queue(Job::RunThere(target.clone(), path.clone()));
                    queued += 1;
                }
                // Held back with the other list work, and for the same reason: the entries
                // name files that the sends above it are what put in place.
                Step::Rebuild { into, entries } => {
                    self.state.rebuild = Some((*into, entries.clone()));
                    edited += 1;
                }
                // **Held back until the files are where the list will say they are.** See
                // `State::after_transfers`: doing this now asks whether the payload is on
                // internal storage, which is what the step before it is for.
                Step::List(fix) => {
                    self.state.after_transfers.push(fix.clone());
                    edited += 1;
                }
            }
        }

        self.state.pending_plan = None;
        // **Only checked when checking could mean anything.**
        //
        // A plan that ends in a list edit is not finished when its transfers are: the file is
        // written from a panel showing the whole thing, by somebody who has read it. Asking the
        // target at that point would always report the finding as still failing - which is true
        // and useless, and reads as *the fix did not work* rather than *there is one step left
        // and it is yours*.
        // **The listing has to catch up before the edit is judged against it.**
        //
        // Adding an entry is refused unless the payload is on internal storage, and that is
        // decided by the last listing taken - which was taken before the send in this very
        // plan. Without this the deferred edit is refused for the state of the world one step
        // ago, which is the same failure as applying it too early wearing a different hat.
        if queued > 0 && !self.state.after_transfers.is_empty() {
            self.state
                .queue(Job::FindPayloads(target.clone(), PAYLOADS.to_owned()));
        }
        if queued > 0 && self.state.after_transfers.is_empty() {
            // Every job here reports on itself; none of them can say whether the finding is
            // answered, so the target is asked again and the answer is read.
            self.state.queue(Job::Check(target));
            self.state.fixing = Some(pending.id.clone());
        }
        self.state.said = summarise(queued, edited, &could_not);
        // Nothing was queued, so nothing is going to land and prompt this later.
        if queued == 0 {
            self.finish_deferred_edits();
        }
    }

    /// Makes the list edits a plan agreed, now that its transfers have landed.
    ///
    /// **Only when the line is clear and nothing went wrong.** A transfer that failed took the
    /// rest of the queue with it, and adding an entry for a file that never arrived would write
    /// exactly the startup list this program exists to prevent - one naming something the
    /// manager cannot resolve, which fails at every boot with a log line nobody reads.
    fn finish_deferred_edits(&mut self) {
        if self.state.after_transfers.is_empty() && self.state.rebuild.is_none() {
            return;
        }
        if !self.state.is_idle() || self.state.queued() > 0 {
            return;
        }
        let waiting = std::mem::take(&mut self.state.after_transfers);
        let rebuild = self.state.rebuild.take();
        if let Some(why) = self.state.trouble.clone() {
            self.state.trouble = Some(format!(
                "{why}\nso the startup list was left alone - {} edits were not made",
                waiting.len() + usize::from(rebuild.is_some())
            ));
            return;
        }
        if let Some((into, entries)) = rebuild {
            self.prepare_rebuild(into, &entries);
            return;
        }
        self.apply_fixes(&waiting);
    }

    /// Puts a whole new startup list up for the same review every write goes through.
    ///
    /// **Prepared, never written.** What comes out of the configurator is a file, and a file
    /// this program writes is one somebody has read first - which is the panel this hands to,
    /// not a promise made here.
    fn prepare_rebuild(&mut self, into: &'static str, entries: &[String]) {
        let text = entries
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n");
        let now = if text.is_empty() {
            text
        } else {
            format!("{text}\n")
        };
        let boot = pros_core::boot::Boot::parse(&now);
        // The screen follows the list being written, or it would review one file and save
        // another.
        if let Some(at) = pros_core::chain::LISTS
            .iter()
            .position(|one| one.path == into)
        {
            self.state.list_at = at;
        }
        self.state.boot = Some(boot);
        self.state.boot_at = None;
        self.state.pending_change = Some(pros_core::autoload::Change {
            // What it is replacing, so the review below draws a diff rather than a wall of
            // green - somebody about to overwrite a working chain should see which lines go.
            was: self
                .state
                .boot
                .as_ref()
                .map(pros_core::boot::Boot::to_text)
                .unwrap_or_default(),
            now: now.clone(),
            what: format!("set {into} up from nothing - {} entries", entries.len()),
            into,
        });
        self.state.section = Section::Autoload;
        self.state.said = format!(
            "{} entries ready for {into} - read the whole file below, then save it",
            entries.len()
        );
    }

    /// Records a description that now points at a newer release.
    ///
    /// **Written to the payload list, which is a file somebody owns.** So it says what changed
    /// in the status line rather than only that something did - a version and a digest moving
    /// under somebody is exactly the pair they would want to have seen go past.
    fn take_relisted(
        &mut self,
        now: pros_core::manifest::Payload,
        found: pros_core::sources::Upstream,
    ) {
        // **What the project said a moment ago replaces what a sweep recorded hours ago.**
        // Without this the version column would go on showing the old comparison against an
        // entry that has just been brought up to date, and offer to update it again.
        self.sources.put(&now.name, found);
        let _ = pros_core::sources::save(&self.sources);
        let Some(manifest) = self.manifest.as_mut() else {
            return;
        };
        let was = manifest
            .find(&now.name)
            .and_then(|one| one.version.clone())
            .unwrap_or_else(|| "-".to_owned());
        let to = now.version.clone().unwrap_or_else(|| "-".to_owned());
        let name = now.name.clone();
        manifest.absorb(now);
        match manifest.save() {
            Ok(path) => {
                self.state.said = format!(
                    "{name} in the list is now {to}, was {was} - digest taken from what \
                     downloaded, written to {}",
                    path.display()
                );
            }
            // The description is right in memory and wrong on disk, which is the one outcome
            // worth interrupting for: the next launch would silently go back to the old one.
            Err(why) => {
                self.state.trouble = Some(format!(
                    "{name} was updated here but not written down: {why}"
                ));
            }
        }
    }

    /// A payload's description, matched the way everything else matches names.
    fn described_as(&self, service: &str) -> Option<pros_core::manifest::Payload> {
        self.manifest
            .as_ref()?
            .payloads()
            .iter()
            .find(|one| {
                pros_core::chain::Chain::parse(&one.name)
                    .position(service)
                    .is_some()
            })
            .cloned()
    }

    /// Says whether a fix that has been carried out actually answered its finding.
    ///
    /// **A fix is not a result.** Every job in a plan reports on itself and none of them knows
    /// whether the thing somebody wanted is now true, so a plan ends by asking the target again
    /// and this reads the answer. Reporting *done* off the back of the jobs having succeeded is
    /// the defect this whole project is organised against.
    fn verify_if_due(&mut self, findings: &[pros_core::doctor::Finding], idle: bool) {
        if !idle || self.state.queued() > 0 {
            return;
        }
        let Some(id) = self.state.fixing.take() else {
            return;
        };
        let still = findings
            .iter()
            .find(|one| one.id == id && one.verdict.is_unwell());
        self.state.said = match still {
            None => format!("{id}: fixed, and checked"),
            Some(one) => format!(
                "{id}: the steps ran and the check still fails - {}",
                one.verdict.describe()
            ),
        };
    }

    /// Makes every edit the survival audit asked for, and shows the result for review.
    ///
    /// # Why it edits rather than writes
    ///
    /// The startup list is the one thing this program writes to a target, and it does that
    /// through a panel showing the whole file. A one-press *write* from the check screen would
    /// be the same edit with the review taken out - and the review is what stops the next bad
    /// list going on unseen.
    ///
    /// So this applies the edits, lands on the autoload screen, and leaves the save to
    /// somebody who has looked at it.
    ///
    /// # Why a name is resolved to a file
    ///
    /// The audit talks about services - *shsrv* - and a startup list names **files** -
    /// `shsrv_v0.20.elf`. The target's own payload scan is what joins them, which is why this
    /// is offered only once that scan has come back: adding a name the target does not have
    /// would write a list that fails at every boot.
    fn apply_fixes(&mut self, fixes: &[pros_core::recovery::Fix]) {
        use pros_core::recovery::Fix;

        let Some(mut boot) = self.state.boot.clone() else {
            return;
        };
        let there = self.state.payloads_there.clone().unwrap_or_default();
        let mut done = 0_usize;
        let mut could_not: Vec<String> = Vec::new();

        for fix in fixes {
            match fix {
                Fix::Remove(service) => {
                    // Found the same way the check finds it, so what is removed is what was
                    // reported - a second matching rule here could disagree with the first.
                    let at = pros_core::chain::Chain::parse(&boot.to_text()).position(service);
                    if let Some(at) = at
                        && boot.remove(at)
                    {
                        done += 1;
                    }
                }
                Fix::Add(service) => {
                    let named = |one: &&There| {
                        pros_core::chain::Chain::parse(&one.name)
                            .position(service)
                            .is_some()
                    };
                    // **Only somewhere a startup list can actually rely on.** A payload on a
                    // stick outside the manager's own folder is listed by the manager and can
                    // never be resolved by it, so adding one would build the exact failure
                    // this button exists to repair.
                    match there
                        .iter()
                        .filter(named)
                        .find(|one| one.storage == pros_core::payloads::Where::Internal)
                    {
                        Some(one) if boot.add(&one.name) => done += 1,
                        Some(_) => {}
                        None => {
                            // It may still be *somewhere* - on a stick, or on this machine.
                            // That is a copy away from being usable, and saying which is the
                            // difference between a dead end and a next step.
                            let elsewhere = there.iter().find(named);
                            could_not.push(match elsewhere {
                                Some(one) => format!(
                                    "{service} (only at {}, which the manager cannot autoload \
                                     - copy it to {} first)",
                                    one.path,
                                    pros_core::payloads::INTERNAL
                                ),
                                None => format!("{service} (not on the target at all)"),
                            });
                        }
                    }
                }
            }
        }

        // **Without this there is no review and no save button.** The edit goes in, the screen
        // looks identical to the one somebody just left, and the button has spent their trust
        // to achieve nothing they can see. Every other edit to this list sets it; these did
        // not, which is exactly how a fix button became a navigation button.
        self.state.pending_change = boot.change();
        self.state.boot = Some(boot);
        self.state.section = Section::Autoload;
        self.state.said = if could_not.is_empty() {
            format!("{done} edits made - review the list and save")
        } else {
            format!(
                "{done} edits made - review and save. Not done: {} is not on the target, so \
                 send it first",
                could_not.join(", ")
            )
        };
    }

    /// Asks the target everything the window will need, as soon as there is a target to ask.
    ///
    /// **Nobody should have to press a button to find out what a tool is for.** Selecting a
    /// target is the whole of the intent; asking is what this program does with it.
    ///
    /// It also clears what was known first, because a report is about one machine and
    /// showing the previous one's answers under a new name is worse than showing nothing.
    ///
    /// # Why it reads more than the check
    ///
    /// Because a panel that has not looked gives advice as though it had. The payload scan
    /// used to happen only when somebody opened the autoload screen, so the check - which is
    /// where the advice lives - never knew what was on the target's disk. It offered to
    /// **download** a payload that was already sitting there, and would then have offered to
    /// send a second copy of it. The screen looked confident either way.
    ///
    /// Three reads, in the order their answers are needed:
    ///
    /// 1. **the check**, because every other panel's advice is qualified by it,
    /// 2. **what payloads the target holds**, which is what stops the bad recommendation,
    /// 3. **the startup list and settings**, which say what will come back after a restart.
    /// 4. **what the target is**, so the system screen is not blank until somebody opens it.
    ///
    /// The first starts immediately and the rest queue behind it. **A failure stops the
    /// rest** - the queue's own rule - which is why the order is by importance: a target
    /// without a payload manager fails the third and still has the first two.
    fn forget_the_last_target(&mut self) {
        self.state.report = None;
        self.state.chain = None;
        self.state.located = None;
        // All of these are about the target being left behind. An answer that outlived its
        // target is the same defect as one that outlived its question - and title names were
        // the quiet case: fetched, kept, and never cleared, so a second target would have
        // shown the first one's names beside its own identifiers.
        self.state.system = None;
        self.state.settings = None;
        self.state.boot = None;
        // **This one was being kept across targets**, which is the same fault as the rest of
        // this list and worse in its consequences: one target's payloads shown as another's,
        // in the panel that decides what to recommend.
        self.state.payloads_there = None;
        self.state.names.clear();
    }

    fn survey_on_arrival(&mut self) {
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // A different machine, or the same one being asked again - and the two are not the
        // same thing to do to what is on screen.
        let elsewhere = self.state.checked_for.as_deref() != Some(target.name.as_str());
        if !elsewhere && !self.state.resurvey {
            return;
        }
        if !self.state.is_idle() {
            return;
        }
        self.state.resurvey = false;
        if elsewhere {
            self.forget_the_last_target();
        }
        self.state.checked_for = Some(target.name.clone());
        self.state.begin(Job::Check(target.clone()));
        self.state
            .queue(Job::FindPayloads(target.clone(), PAYLOADS.to_owned()));
        self.state.queue(Job::ReadAutoload(target.clone()));
        // **What the target is.** Last because it is the only one no other panel qualifies,
        // but read on connect all the same: a screen that shows nothing until it is visited
        // makes a person wait for something that could already have been asked for.
        self.state.queue(Job::ReadSystem(target));
    }

    /// Asks the target which of this section's candidate directories it has.
    ///
    /// **Only where there is a choice to make.** A section with one measured path has nothing
    /// to ask about; cheats have three, none of which is the standard, so the target decides.
    ///
    /// Asked once per section per target, and forgotten when the target changes - the same
    /// rule as the check, and for the same reason: what a target has is not a fact that
    /// survives being pointed at a different one.
    fn system_on_arrival(&mut self) {
        if self.state.section != Section::System
            || self.state.system.is_some()
            || !self.state.is_idle()
        {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // Not before the check: it is the one that says whether the shell is even answering,
        // and asking six questions of a target that is not there is six timeouts.
        if self.state.report.is_none() {
            return;
        }
        self.state.begin(Job::ReadSystem(target));
    }

    /// Reads the manager's settings on arrival, for the same reason.
    ///
    /// Then the payload scan, because the startup list cannot say which of its entries are
    /// missing until something has looked at what is there.
    fn autoload_on_arrival(&mut self) {
        if self.state.section != Section::Autoload
            || !self.state.is_idle()
            || self.state.report.is_none()
        {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        if self.state.settings.is_none() {
            self.state.begin(Job::ReadAutoload(target));
        } else if self.state.payloads_there.is_none() {
            self.state
                .begin(Job::FindPayloads(target, PAYLOADS.to_owned()));
        }
    }

    /// Starts following the log when somebody opens that screen.
    ///
    /// **The section exists to watch a log**, and a screen that shows nothing until a button
    /// is pressed has made somebody ask for the thing they already asked for by opening it.
    ///
    /// Tried once per target. A failure - the service is not loaded, the connection is
    /// refused - leaves the button, because retrying every frame would be a connection
    /// attempt sixty times a second against a target that has already said no.
    fn follow_on_arrival(&mut self) {
        if self.state.section != Section::Log || self.tail.is_some() {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        if self.state.followed_for.as_deref() == Some(target.name.as_str()) {
            return;
        }
        self.state.followed_for = Some(target.name.clone());
        match crate::tail::Tail::start(&target.name, &target.link()) {
            Ok(tail) => {
                self.state.lines.clear();
                self.tail = Some(tail);
            }
            // Left where the log screen already puts its trouble, rather than raised: a log
            // service that is not loaded is a normal state the check already reports.
            Err(why) => self.state.trouble = Some(why),
        }
    }

    fn locate_on_arrival(&mut self) {
        let candidates = self.state.section.candidates();
        let answered = self
            .state
            .located
            .as_ref()
            .is_some_and(|(asked, _)| *asked == self.state.section);
        if candidates.is_empty() || answered || !self.state.is_idle() {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // Not while the check is still the thing being waited on: one job at a time, and the
        // check is the one that says whether the file service is even up.
        if self.state.report.is_none() {
            return;
        }
        self.state.begin(Job::Locate(target, candidates));
    }

    /// Whether a service this section needs is answering, explaining in the panel if not.
    ///
    /// Answers `true` when the work can go ahead. When it cannot, **this draws the reason
    /// where a person is already looking** - which service, what it buys, whether it is here
    /// to send, and whether it will come back after a reboot - and offers the one action
    /// that would change it.
    fn needs(&mut self, ui: &mut egui::Ui, service: &str) -> bool {
        let Some(report) = &self.state.report else {
            self.unasked(ui, service);
            return false;
        };
        let Some(finding) = report.about(service) else {
            // A service nothing knows about is not one this can report on, and pretending
            // otherwise would be inventing a measurement.
            return true;
        };
        if finding.reachability.open {
            return true;
        }

        let unlocks = &finding.service.unlocks;
        let boot = self.state.chain.as_ref().map_or(Boot::Unknown, |chain| {
            chain.position(service).map_or(Boot::NotInList, Boot::At)
        });
        let staged = self
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.find(service))
            .filter(|payload| pros_core::staging::is_staged(payload))
            .and_then(pros_core::staging::path_for);

        section_heading(ui, self.state.section);
        ui.colored_label(
            egui::Color32::from_rgb(220, 120, 120),
            format!("{service} is not answering"),
        );
        ui.label(format!("this section needs it to {unlocks}"));
        ui.add_space(4.0);
        match boot {
            Boot::At(at) => {
                ui.small(format!(
                    "in the boot list at {at}, so a reboot brings it back"
                ));
            }
            Boot::NotInList => {
                ui.small("not in the boot list, so a reboot will not bring it back");
            }
            Boot::Unknown => {
                ui.small(
                    "the boot list has not been read, so what a reboot brings back is unknown",
                );
            }
        }
        ui.add_space(8.0);

        let mut send = None;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    staged.is_some() && self.state.is_idle(),
                    egui::Button::new("run it"),
                )
                .on_hover_text("load it now, through the loader")
                .on_disabled_hover_text("not staged here - the payloads section can fetch it")
                .clicked()
            {
                send.clone_from(&staged);
            }
            if ui.button("go to payloads").clicked() {
                self.state.section = Section::Payloads;
            }
        });
        if let Some(path) = send
            && let Some(target) = self.state.target().cloned()
        {
            self.state
                .begin(Job::Send(target, service.to_owned(), path));
        }
        false
    }

    /// What to say when nothing has asked the target yet.
    fn unasked(&mut self, ui: &mut egui::Ui, service: &str) {
        section_heading(ui, self.state.section);
        if self.state.target().is_none() {
            ui.label("no target selected");
            ui.small("target -> register..., or pick one from the list above");
            return;
        }
        // The check starts on its own the moment a target is selected, so by the time
        // anybody reads this it is usually already running.
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("asking the target about {service}"));
        });
        ui.small("what a target can do is not remembered between runs, because a jailbreak");
        ui.small("does not survive a power cycle - so it is asked every time");
    }

    /// Watching the target, over our own stream.
    ///
    /// # This is the stand-in, not a description of it
    ///
    /// The panel used to be two paragraphs about somebody else's client. It is now the
    /// controls the stand-in actually has: connect, watch what goes past, and drive it. The
    /// half that does not exist yet is the payload at the other end - and a socket that
    /// refuses to open says that far more precisely than a paragraph did.
    ///
    /// So nothing here is disabled pending a target. Press watch with no payload running and
    /// it says *connection refused*, naming the port, which is the truth.
    fn stream_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::Stream);

        self.watch_bar(ui);
        ui.add_space(10.0);
        self.watch_counts(ui);
        ui.add_space(12.0);
        self.watch_input(ui);
    }

    /// Start it, stop it, and say plainly where it stands.
    fn watch_bar(&mut self, ui: &mut egui::Ui) {
        let counts = self.state.watching.counts();
        let running = counts.status.is_watching();
        let target = self.state.target().cloned();

        ui.horizontal(|ui| {
            if running {
                if ui
                    .button("stop")
                    .on_hover_text("closes the player's input, which ends it cleanly")
                    .clicked()
                {
                    self.state.watching.stop();
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
            ui.add(egui::TextEdit::singleline(&mut self.state.watch_port).desired_width(56.0));

            ui.add_space(6.0);
            let (colour, said) = match &counts.status {
                pros_core::watch::Status::Watching => (
                    egui::Color32::from_rgb(120, 200, 140),
                    counts.status.describe(),
                ),
                pros_core::watch::Status::Idle => (egui::Color32::GRAY, "not watching".to_owned()),
                // **Ended and failed are both red and both said out loud.** A stream that
                // stopped and a stream that never started are different faults, and the one
                // thing they must not do is look like never having pressed the button.
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

        // Not an error and not a disabled button: a file nobody has written yet, and the
        // button beside the sentence writes it.
        ui.label("The stream is piped to whatever plays video on this machine. This project");
        ui.label("decodes nothing - it counts what goes past instead, which is how it can");
        ui.label("tell you which kind of nothing you are looking at.");
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("write the file to edit").clicked() {
                match pros_core::watch::write_example() {
                    Ok(path) => self.state.said = path.display().to_string(),
                    Err(why) => self.state.trouble = Some(why),
                }
            }
            if let Some(path) = pros_core::watch::command_path() {
                ui.small(path.display().to_string());
            }
        });
    }

    /// What has gone past, and what to make of it.
    ///
    /// # Why a panel that decodes nothing still counts everything
    ///
    /// A media player answers one question - is there a picture - and gives the same answer to
    /// at least four different faults. This read the bytes on the way through, so it can say
    /// which: nothing arrived, something arrived that was not this kind of stream, units
    /// arrived with no keyframe in them, or all of that was fine and the player went away.
    ///
    /// The third is why the whole arrangement is worth it. A stream carrying nothing but
    /// dependent pictures decodes to nothing and **looks exactly like no stream at all**.
    fn watch_counts(&mut self, ui: &mut egui::Ui) {
        let counts = self.state.watching.counts();
        if matches!(counts.status, pros_core::watch::Status::Idle) {
            return;
        }

        egui::Grid::new("watch-counts")
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                // **The rate leads.** Every other figure here only ever climbs, so a stream at
                // sixty a second and one at two look identical in them - and that is the
                // difference between a stream and a slideshow.
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
                    // Not zero. Nobody has measured a second yet, and showing that as a
                    // stalled stream would accuse a healthy one.
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
                    // A held figure that climbs and never falls is a stream that stopped
                    // producing boundaries, which otherwise looks like a stream that stopped.
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
    /// **Not a separate journey.** Watching a target and playing it are one thing to a person,
    /// and putting the controller behind another click would make the stand-in feel like two
    /// features that happen to sit next to each other.
    fn watch_input(&mut self, ui: &mut egui::Ui) {
        let driving = self.state.pads.filled();
        ui.horizontal(|ui| {
            ui.strong("input");
            let sending = self.state.feed.status.is_sending();
            ui.colored_label(
                if sending {
                    egui::Color32::from_rgb(120, 200, 140)
                } else {
                    egui::Color32::GRAY
                },
                self.state.feed.status.describe(),
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
    /// # Why this opens the input feed too
    ///
    /// They are two sockets and one experience. Somebody who pressed *watch* is trying to play
    /// a target, and leaving the controller behind a second connect on a second screen makes
    /// the stand-in two features that happen to be adjacent.
    ///
    /// **Opening it is not required to work.** The two halves fail independently - one payload
    /// serves both ports, but a payload that got video working and input not is exactly the
    /// state this project expects to be in - so a feed that will not open says so and the
    /// picture still comes up.
    fn begin_watching(&mut self, link: &pros_link::Link) {
        let Some(command) = pros_core::watch::configured() else {
            self.state.trouble =
                Some("no player named yet - the button below writes the file to edit".to_owned());
            return;
        };
        // A port somebody typed badly falls back to the documented one rather than refusing:
        // the field is a convenience for a payload built on a different port, not a gate.
        let port = self
            .state
            .watch_port
            .trim()
            .parse()
            .unwrap_or(pros_core::watch::PORT);
        self.state.watching = pros_core::watch::Watching::start(&link.address, port, &command);

        if !self.state.feed.status.is_sending() {
            let port = self
                .state
                .feed_port
                .trim()
                .parse()
                .unwrap_or(pros_link::feed::PORT);
            // The failure is left in the feed's own status rather than raised as trouble: it
            // belongs beside the input line, where somebody can see which half is missing.
            let _ = self.state.feed.open(&link.address, port);
        }
    }

    /// The target's log, as plain scrollable text.
    ///
    /// # Why it scrolls to a measured rect instead of `stick_to_bottom`
    ///
    /// `stick_to_bottom` scrolls to the bottom of the content size the scroll area recorded on
    /// the **previous** frame. That is fine for content that does not change. A log grows
    /// while it is being watched, so every frame the widget is taller than the number being
    /// scrolled against - and the view lands short of the end, with a band of nothing and the
    /// last line clipped at the edge. Which is exactly what it did, through six attempts at
    /// fixing everything except this.
    ///
    /// So the text is added first, its response gives its real rect **this** frame, and the
    /// view is scrolled to the bottom of that. Nothing is computed, nothing is remembered
    /// between frames, and there is no number that can be a frame out of date.
    fn log_panel(&mut self, ui: &mut egui::Ui) {
        if self.state.target().is_none() {
            section_heading(ui, Section::Log);
            ui.label("no target selected");
            return;
        }

        let following = self.tail.is_some();
        self.log_toolbar(ui, following);

        let text = self
            .state
            .kept_lines()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("
");
        if text.is_empty() {
            ui.weak(if self.state.log_filter.trim().is_empty() {
                if following {
                    "nothing yet - a quiet log is a fact about the target, not a fault"
                } else {
                    "not following"
                }
            } else {
                "nothing matches that filter - the lines are still arriving"
            });
            return;
        }

        // **One widget. No scroll area, no nesting, no second surface.**
        //
        // A `TextEdit` given a size scrolls itself, which is the whole of what a log view
        // needs. Wrapping one in a `ScrollArea` made two things that both believed they owned
        // the height, and the picture showed it: the panel's rect ended above the status bar
        // while the scroll area's viewport sat below it, so text was painted into both and one
        // of them was a sliver behind the status bar.
        //
        // Seven attempts were spent adjusting the relationship between those two boxes. There
        // is no relationship to get right once there is only one box.
        let _ = following;
        ui.add_sized(
            ui.available_size(),
            egui::TextEdit::multiline(&mut text.as_str()).font(egui::TextStyle::Monospace),
        );
    }

    /// The log screen controls: following, filtering, copying, and where it is kept.
    #[allow(
        clippy::too_many_lines,
        reason = "one toolbar row, and splitting it would put half the controls in a \
                  different function from the state they all read"
    )]
    fn log_toolbar(&mut self, ui: &mut egui::Ui, following: bool) {
        // Re-read here rather than passed in: the panel above it has already established
        // there is one, and threading it through a closure that also borrows `self` is what
        // the rest of this window avoids by re-asking.
        let known = self.state.target().cloned();
        section_heading_with(ui, Section::Log, |ui| {
            if following {
                if ui
                    .button("stop")
                    .on_hover_text("close the connection and stop following")
                    .clicked()
                {
                    self.tail = None;
                }
                ui.colored_label(egui::Color32::from_rgb(120, 190, 120), "following");
            } else {
                if ui
                    .button("follow")
                    .on_hover_text("open the log and show lines as they arrive")
                    .clicked()
                {
                    match known.as_ref().map_or_else(
                        || Err("no target selected".to_owned()),
                        |target| crate::tail::Tail::start(&target.name, &target.link()),
                    ) {
                        Ok(tail) => {
                            self.state.lines.clear();
                            self.tail = Some(tail);
                        }
                        Err(why) => self.state.trouble = Some(why),
                    }
                }
                ui.weak("not following");
            }
            if ui
                .add_enabled(!self.state.lines.is_empty(), egui::Button::new("clear"))
                .on_hover_text("forget what has been shown - the log keeps arriving")
                .clicked()
            {
                self.state.lines.clear();
            }
            // **Said, not shown by an absence of lines.** A log that has ended and one that
            // has gone quiet look identical on screen and mean opposite things.
            if self.tail.as_ref().is_some_and(crate::tail::Tail::has_ended) {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 120, 120),
                    "the target closed the connection",
                );
            }
            ui.label("filter");
            ui.add(
                egui::TextEdit::singleline(&mut self.state.log_filter)
                    .desired_width(160.0)
                    .hint_text("text to keep"),
            )
            .on_hover_text("only lines containing this, ignoring case - the log keeps arriving");
            // **Counted after filtering, and both numbers shown.** A filter that hid ninety
            // lines and then said `10 lines` would be describing a target that is quiet.
            let kept = self.state.kept_lines().count();
            let all = self.state.lines.len();
            if self.state.log_filter.trim().is_empty() {
                ui.weak(format!("{all} lines"));
            } else {
                ui.weak(format!("{kept} of {all} lines"));
            }
            // **Said, not just done.** A log kept somewhere nobody is told about is a log
            // nobody reads afterwards, which is the whole reason for keeping it.
            if let Some(target) = known.as_ref()
                && let Some(path) = crate::tail::kept_at(&target.name)
            {
                ui.weak("kept").on_hover_text(format!(
                    "{}
every line is appended here as it arrives, and the previous file is kept beside it",
                    path.display()
                ));
                if ui
                    .small_button("open folder")
                    .on_hover_text("show the kept logs in this machine file browser")
                    .clicked()
                    && let Some(at) = path.parent()
                {
                    self.reveal(at);
                }
            }
            if ui
                .add_enabled(kept > 0, egui::Button::new("copy"))
                .on_hover_text("copy what is shown to the clipboard, filter and all")
                .on_disabled_hover_text("nothing to copy")
                .clicked()
            {
                let text: String = self
                    .state
                    .kept_lines()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.output_mut(|out| out.copied_text = text);
            }
        });

        egui::ScrollArea::vertical()
            .id_salt("log")
            .auto_shrink([false, false])
            // **Pinned to the bottom while following.** A log somebody is watching is one
            // where the newest line is the interesting one, and a view that stayed where it
            // was would show the moment before the thing they are waiting for.
            .stick_to_bottom(following)
            .show(ui, |ui| {
                let mut shown = 0;
                for line in self.state.kept_lines() {
                    ui.monospace(line);
                    shown += 1;
                }
                if shown == 0 && !self.state.log_filter.trim().is_empty() {
                    ui.weak("nothing matches that filter - the lines are still arriving");
                } else if self.state.lines.is_empty() {
                    ui.weak(if following {
                        "nothing yet - a quiet log is a fact about the target, not a fault"
                    } else {
                        "not following"
                    });
                }
            });
    }

    /// A command, and what it printed.
    fn shell_panel(&mut self, ui: &mut egui::Ui) {
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

    /// A screen whose first answer is still on its way.
    ///
    /// **Nothing to press.** The panel is not drawn at all rather than drawn with its controls
    /// greyed: there is no content behind them, and a table of empty rows with disabled buttons
    /// over the top is a worse picture of *waiting* than a sentence saying so.
    ///
    /// A re-read of a screen that already has something never gets here - see
    /// [`crate::state::State::still_arriving`].
    fn arriving(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, self.state.section);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            // Which question, not just *loading*: the answers arrive in sequence, and a person
            // watching four screens fill in one at a time can see which one is theirs.
            ui.label(
                self.state
                    .waiting
                    .as_ref()
                    .map_or_else(|| "waiting its turn".to_owned(), |one| one.job.describe()),
            );
        });
        let queued = self.state.queued();
        if queued > 0 {
            ui.add_space(4.0);
            ui.weak(format!("{queued} more to ask after this one"));
        }
        ui.add_space(6.0);
        ui.weak("this screen opens as soon as its answer arrives - later refreshes leave it up");
    }

    /// Draws whichever section the sidebar has selected.
    fn section(&mut self, ui: &mut egui::Ui) {
        // **One gate, in front of everything.** A section says what it needs; this asks the
        // last check about it and explains in the panel rather than greying a control and
        // hoping somebody hovers. Nothing here probes for itself.
        if let Some(needed) = self.state.section.requires()
            && !self.needs(ui, needed)
        {
            return;
        }
        // **The second gate: a screen whose first answer has not arrived.**
        //
        // Every panel used to draw itself over an absence and say something like *not read
        // yet*, which is true of a screen nobody has asked about and false of one whose answer
        // is second in a queue - and the two read identically. On arrival four jobs are queued
        // at once, so for a few seconds three of the four screens were telling somebody to go
        // and fetch something that was already on its way.
        //
        // It is drawn in one place rather than in each panel so that every screen answers this
        // the same way, and so that a new screen gets the behaviour by saying what it needs
        // rather than by remembering to write it.
        if self.state.still_arriving(self.state.section) {
            self.arriving(ui);
            return;
        }
        // **The other half of the same rule.** A screen with something on it is left alone
        // while it is asked again - but said, quietly, so that what is on screen is not taken
        // for the answer to the question somebody just asked. Without this the panel is
        // indistinguishable from one that is finished, which is how a stale reading gets acted
        // on as a fresh one.
        if self.state.re_reading(self.state.section) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("reading again - what is below is from a moment ago");
            });
        }
        match self.state.section {
            Section::Check => self.check_panel(ui),
            Section::Stream => self.stream_panel(ui),
            Section::Autoload => self.autoload_panel(ui),
            Section::System => self.system_panel(ui),
            Section::Controllers => self.controllers_panel(ui),
            Section::Payloads => {
                // Settled like every other two-sided section, so its target pane starts at
                // the payload directory rather than wherever the last section was looking.
                self.settle(Section::Payloads);
                self.payloads_body(ui);
            }
            Section::Log => self.log_panel(ui),
            Section::Shell => self.shell_panel(ui),
            // Five views over one thing. They differ in where each side starts and in
            // nothing else, and pretending otherwise would be five copies of one view.
            section @ (Section::Packages
            | Section::Titles
            | Section::Saves
            | Section::Cheats
            | Section::Filesystem) => {
                self.settle(section);
                self.sync_body(ui);
            }
        }
    }

    /// What is happening, and what went wrong.
    ///
    /// **Waiting is shown with its own clock.** A window that is working and a window that
    /// has hung look identical otherwise, which is this project's recurring defect in its
    /// most literal form.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        // **The record opens above the line rather than replacing it.** What is happening now
        // is the thing somebody glances at; what happened before is the thing they go looking
        // for. Swapping one for the other would lose the glance.
        if self.state.journal.open {
            self.activity(ui);
            ui.separator();
        }
        ui.horizontal(|ui| {
            let troubles = self.state.journal.troubles();
            let count = self.state.journal.all().len();
            let arrow = if self.state.journal.open { "v" } else { ">" };
            // The count and the trouble count on the closed bar, so it is worth opening
            // without being opened first.
            let label = if troubles > 0 {
                format!("{arrow} activity  ({count}, {troubles} failed)")
            } else {
                format!("{arrow} activity  ({count})")
            };
            if ui
                .selectable_label(self.state.journal.open, label)
                .on_hover_text("everything this program has done this session")
                .clicked()
            {
                self.state.journal.open = !self.state.journal.open;
            }
            ui.separator();

            if let Some(waiting) = &self.state.waiting {
                ui.spinner();
                ui.label(format!(
                    "{} … {:.1}s",
                    waiting.job.describe(),
                    waiting.elapsed().as_secs_f32()
                ));
                // **Beside the thing it stops.** A long copy is started by one click and can
                // turn out to be far larger than whoever clicked expected; without this the
                // only way out is to kill the process, which loses the account of what was
                // copied along with the copying.
                if ui
                    .small_button("stop")
                    .on_hover_text("finish the file in flight, then stop and say what was left")
                    .clicked()
                {
                    self.worker.stop();
                }
                // **A queue nobody can see is the bug this was written to fix, moved.** If a
                // press started four things, the bar has to say four - and say when one of
                // them is dropped.
                let waiting_turn = self.state.queued();
                if waiting_turn > 0 {
                    ui.weak(format!("{waiting_turn} queued"));
                    if ui
                        .small_button("clear queue")
                        .on_hover_text(
                            "forget what has not started - what is running now is not touched",
                        )
                        .clicked()
                    {
                        let dropped = self.state.drop_queued();
                        self.state.said = format!("{dropped} were dropped before starting");
                    }
                }
                // What is going across right now, when there is one. A clock says time is
                // passing; this says something is happening.
                if let Some(progress) = &self.state.progress {
                    ui.weak(format!(
                        "{} files, {} - {}",
                        progress.files,
                        size(progress.bytes),
                        progress.current
                    ));
                }
            } else if let Some(trouble) = &self.state.trouble {
                ui.colored_label(egui::Color32::from_rgb(220, 120, 120), trouble);
            } else {
                ui.weak("idle");
            }
        });
    }

    /// Everything this program has done this session, newest first.
    ///
    /// # Why newest first here and oldest first in the record
    ///
    /// The record keeps them in the order they happened, because that is what they are. This
    /// shows them reversed, because the thing somebody opens this panel to see is almost
    /// always the last one - and a list that puts it at the bottom asks them to scroll to
    /// find out what just happened.
    fn activity(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("activity");
            if ui
                .small_button("clear")
                .on_hover_text("forget what has finished - anything still running stays")
                .clicked()
            {
                self.state.journal.clear();
            }
            ui.weak("this program's own actions - the target's log is under diagnose");
        });

        egui::ScrollArea::vertical()
            .id_salt("activity")
            .max_height(180.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("activity-rows")
                    .striped(true)
                    .num_columns(5)
                    .show(ui, |ui| {
                        for entry in self.state.journal.all().iter().rev() {
                            let colour = match &entry.ending {
                                crate::journal::Ending::Failed(_) => {
                                    egui::Color32::from_rgb(220, 120, 120)
                                }
                                crate::journal::Ending::Refused(_) => {
                                    egui::Color32::from_rgb(210, 190, 120)
                                }
                                crate::journal::Ending::Running => {
                                    egui::Color32::from_rgb(140, 180, 220)
                                }
                                _ => egui::Color32::GRAY,
                            };
                            ui.colored_label(colour, entry.ending.word());
                            ui.label(&entry.what);
                            ui.weak(entry.target.as_deref().unwrap_or(""));
                            ui.weak(format!("{:.1}s", entry.elapsed().as_secs_f32()));
                            // The result last and dimmed: it is the part somebody reads when
                            // one of the rows above has already caught their eye.
                            ui.weak(entry.ending.said().unwrap_or(""));
                            ui.end_row();
                        }
                    });
                if self.state.journal.all().is_empty() {
                    ui.weak("nothing yet this session");
                }
            });
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match self.worker.collect() {
            // Progress ends nothing: it says how far, and the status bar shows it.
            Some(crate::work::Update::Progress(progress)) => self.state.progress = Some(progress),
            Some(crate::work::Update::Finished(done)) => {
                self.state.progress = None;
                self.state.finish(done);
            }
            None => {}
        }
        // A repository that arrived from the target becomes the manifest on show. Taken
        // here rather than stored by the worker so that the rule about a failed job clearing
        // its own panel keeps applying: the state machine owns what is displayed.
        // Somewhere the target told us to go, once it had been asked.
        if let Some(path) = self.state.go_to.take() {
            self.state.library_path = path;
            self.browse();
        }
        if let Some((payload, found)) = self.state.relisted.take() {
            self.take_relisted(payload, found);
        }
        if let Some((repository, from)) = self.state.repository_read.take() {
            self.absorb(&repository, &from);
        }
        // **Once, on the first frame.** The manifest is already loaded by then, and doing it
        // here rather than in `new` keeps a window that opens instantly: the sweep is spaced
        // out and waits out refusals, so starting it before there is anything on screen would
        // look like a program that will not launch.
        if !self.asked_at_launch {
            self.asked_at_launch = true;
            self.check_sources(false);
        }
        // Answers from the projects, as they come.
        //
        // **Started above rather than below, and this is not a style choice.** With the two the
        // other way round, the frame that starts a sweep asks for no repaint - so a window
        // nobody then touches never runs `update` again, never drains a single answer, and
        // never writes one down. The sweep completes on its thread and everything it learnt is
        // dropped on the floor: a feature that works perfectly and produces nothing, which is
        // this project's own recurring defect written into its newest code.
        if self.sweep.is_some() {
            self.take_sweep_answers();
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        // Lines that arrived since the last frame. Repainting is asked for only when
        // something came, so a quiet log costs nothing.
        if let Some(tail) = &mut self.tail {
            if tail.drain(&mut self.state.lines) {
                ctx.request_repaint();
            }
            // A tail belongs to the target it was opened against. Switching target leaves it
            // showing another machine's log under this one's name.
            if self
                .state
                .target()
                .is_none_or(|now| now.name != tail.target)
            {
                self.tail = None;
            }
        }
        // **Whatever the last job disturbed is read again.** Not a list of special cases
        // here: the job said what it touched, and this does what it was told.
        for what in std::mem::take(&mut self.state.disturbed) {
            match what {
                crate::state::Disturbs::Here => self.read_local(),
                crate::state::Disturbs::There => {
                    // Everything cached about the target is a claim from before this job.
                    self.state.seen.clear();
                    self.browse();
                }
                // **Asked again, and left on screen while it is.**
                //
                // This used to empty the panel first, and the reason given was that a report
                // left up while a new one is fetched is a stale claim for another second. That
                // was true when a blank panel was the only way to say *this is being re-read* -
                // and it cost the screen somebody was reading, every time anything was sent.
                // Saying it is now a sentence at the top of the panel, which is the same
                // honesty for none of the loss.
                //
                // Both take the same route: the survey asks the target everything, and which
                // of the two was disturbed only decides which panel notices first.
                crate::state::Disturbs::Report | crate::state::Disturbs::Autoload => {
                    self.state.resurvey = true;
                }
            }
        }
        // **After the queue, not inside it.** A plan's list edits wait for its transfers, and
        // this is the moment that becomes true - see `finish_deferred_edits`.
        self.finish_deferred_edits();
        // Keep repainting while something runs, so the clock in the status bar advances.
        if self.state.waiting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // **And while a stream is running**, because its counters live on another thread and
        // a window that repaints only when something is clicked would show a working stream
        // as a frozen set of numbers - which is this project's recurring defect exactly: a
        // thing that is working and a thing that has died, rendered identically.
        if self.state.watching.counts().status.is_watching() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // **Input runs from here, not from the panel that draws it**, so it keeps going while
        // somebody is looking at the stream - which is the only screen they are certainly on
        // while playing. See `drive_pads`.
        self.drive_pads(ctx);
        // And a held key produces no events, so without this a direction held down would send
        // one record and then stop until something else woke the window. A pad that reports a
        // held button once is a pad that appears to drop inputs.
        if self.state.pads.filled() > 0 {
            ctx.request_repaint();
        }

        self.survey_on_arrival();
        self.locate_on_arrival();
        self.system_on_arrival();
        self.autoload_on_arrival();
        self.follow_on_arrival();
        self.take_dropped(ctx);
        self.menu_bar(ctx);
        self.register_dialog(ctx);
        self.about_window(ctx);
        self.docs.show(ctx, DOCS);
        egui::TopBottomPanel::bottom("build")
            .show_separator_line(false)
            .show(ctx, |ui| ui.small(&self.stamp));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
        egui::SidePanel::left("sidebar")
            .default_width(190.0)
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default().show(ctx, |ui| {
            // **Not gated on a target.** Only the check has nothing at all to say without
            // one; everything else is about this machine as much as that one, and telling
            // somebody organising their own files to register target first is telling
            // them the tool is for something else.
            // **Only where the section does not scroll itself.** Wrapping the ones that do
            // would give their inner areas unlimited height, which is how a scroll area stops
            // scrolling: it grows to fit and never needs a bar.
            if self.state.section.scrolls_itself() {
                self.section(ui);
            } else {
                // **Both directions.** The startup list has grown to seven columns and a window
                // narrower than the table simply cut the last of them off - with no bar to
                // say there was more, which is the same defect as content running off the
                // bottom, turned ninety degrees.
                egui::ScrollArea::both()
                    .id_salt(self.state.section.name())
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.section(ui));
            }
        });

        // **Unconditionally, after everything that could have begun a job.** Nothing here
        // depends on when in the frame it was begun, or on another frame arriving soon.
        if let Some(job) = self.state.pending.take() {
            self.worker.start(job);
            ctx.request_repaint();
        }
    }
}

/// The verdict as a sentence.
///
/// The same words the command line uses, because two front ends over one library disagreeing
/// about what a finding means is how a shared library stops being shared.
fn say_verdict(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Ready => "ready".to_owned(),
        Verdict::Dimmed { names } => format!(
            "usable, but {} {} not loaded, so something will be invisible if a run goes wrong",
            names.join(" and "),
            were(names.len())
        ),
        Verdict::Blocked {
            remedy: Remedy::RerunTheJailbreak,
        } => "the loader is not answering, so nothing can be sent or started from here. \
              This says nothing about the target: a console can run its whole chain with \
              9021 unreachable. Getting it back means loading elfldr through the exploit's \
              own loader, which means re-running the exploit"
            .to_owned(),
        Verdict::Blocked {
            remedy: Remedy::LoadThese { names },
        } => format!(
            "blocked: {} {} not loaded. The loader is up, so {} can be sent again",
            names.join(" and "),
            were(names.len()),
            if names.len() == 1 { "it" } else { "they" }
        ),
    }
}

/// A byte count somebody can read at a glance.
///
/// Powers of two, because that is what a filesystem counts in, and one decimal place,
/// because two is more precision than a directory listing has earned.
///
/// Integer arithmetic throughout: a size can exceed what a float represents exactly, and a
/// library listing is one of the few places where numbers that large turn up.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    const STEP: u64 = 1024;

    let mut amount = bytes;
    let mut remainder = 0;
    let mut unit = 0;
    while amount >= STEP && unit + 1 < UNITS.len() {
        remainder = amount % STEP;
        amount /= STEP;
        unit += 1;
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 {
        format!("{amount} {name}")
    } else {
        format!("{amount}.{} {name}", remainder * 10 / STEP)
    }
}

/// Agreement, because a list of two that says "is" reads as a tool that has never had two.
const fn were(count: usize) -> &'static str {
    if count == 1 { "is" } else { "are" }
}

#[cfg(test)]
mod tests {
    use pros_core::check::{Remedy, Verdict};

    use crate::state::Section;

    use super::say_verdict;

    /// The window and the command line say the same thing about the same finding.
    ///
    /// Two front ends over one library that word a verdict differently is how a shared
    /// library quietly stops being shared - and the loader's remedy is the one sentence that
    /// must not drift, because it is the one that changes what a person does next.
    #[test]
    fn the_loader_remedy_is_worded_as_the_command_line_words_it() {
        let said = say_verdict(&Verdict::Blocked {
            remedy: Remedy::RerunTheJailbreak,
        });
        assert!(said.contains("re-running the exploit"), "{said}");
        // **And it says only what was measured.** A console can run its whole chain with 9021
        // unreachable - one was measured doing exactly that - so this sentence is about what
        // this program can do, not a diagnosis of the machine.
        assert!(said.contains("says nothing about the target"), "{said}");
        assert!(
            !said.contains("can be sent again"),
            "it offers the remedy for a different failure: {said}"
        );
    }

    /// Two absent services read as two, not as one.
    #[test]
    fn a_pair_of_missing_services_agrees_with_itself() {
        let said = say_verdict(&Verdict::Dimmed {
            names: vec!["klogsrv".to_owned(), "pldmgr".to_owned()],
        });
        assert!(said.contains("klogsrv and pldmgr are not loaded"), "{said}");
    }

    /// **Every place says what it is and why, and no two in a section say the same.**
    ///
    /// # The property this is not
    ///
    /// It first read *no label is a fragment of its own path*, because the labels used to be
    /// derived from the path and that derivation gave the packages section two buttons
    /// reading *homebrew* and *pkg* - words that name nothing and cannot be chosen between.
    ///
    /// **That rule was wrong and running it said so.** `LinkDev` and `cheatrunner` are
    /// perfectly good labels that happen to appear in their own paths; what made *homebrew*
    /// and *pkg* useless was that they name neither a tool nor a purpose, and no rule here can
    /// tell those two cases apart. So what is pinned is what a machine can actually check -
    /// a label and a reason exist, and a section's labels are distinct - while the labels that
    /// matter are pinned by name in the two tests below.
    ///
    /// What found the real fault was somebody looking at the window and asking what the
    /// difference between the two buttons was.
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

    /// **Every cheat location a person might already be using has a button.**
    ///
    /// There is no standard one - the most-used cheat runner documents three and reads all of
    /// them - so the section cannot pick for somebody. Pinned because a candidate quietly
    /// dropped from this list becomes a place the tool will not look and will not offer.
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

    /// A section with one measured path offers no buttons, because there is no choice.
    ///
    /// `/user/app` and `/user/home` are properties of the machine. Packages are **not** on
    /// this list: where they are kept depends on which upload tool somebody installed.
    #[test]
    fn a_section_with_a_measured_path_offers_no_alternatives() {
        assert!(Section::Titles.candidates().is_empty());
        assert!(Section::Saves.candidates().is_empty());
    }

    /// **Both places packages were found on a real target have a button.**
    ///
    /// Neither is made by the machine - one tool's upload root is `/data/homebrew` and its
    /// install staging is `/data/pkg`, so a target running something else has neither.
    ///
    /// Pinned because the default was `/data/homebrew`, which is the **parent** of one of
    /// these and held no packages at all: the section opened on a folder showing a single
    /// subfolder, which reads exactly like a folder with nothing in it.
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
        // **They are two directories, not one under two names** - measured with the target's
        // own `file`, which uses `lstat` and would have said *symbolic link*. So the choice
        // the buttons offer is a real one, and each says which it is.
        assert_eq!(places[0].label, "uploads");
        assert_eq!(places[1].label, "install staging");
    }

    /// **Going up stops at the root**, rather than producing a path above it.
    ///
    /// A listing of somewhere above the root is a request the server answers with a refusal,
    /// and a button that produced one would look like navigation that failed.
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

#[cfg(test)]
mod row_tests {
    use super::{Hit, hit_of};

    /// **A double click opens, and does not also tick.**
    ///
    /// egui defines `double_clicked()` as `clicked && is_double`, so a real double click
    /// arrives with **both** flags set. This is the case that was broken: the ticking branch
    /// ran second and overwrote the opening one, so no folder could ever be entered while the
    /// row went on highlighting as though it had worked.
    #[test]
    fn a_double_click_opens_even_though_it_is_also_a_click() {
        assert_eq!(hit_of(true, true), Some(Hit::Open));
    }

    /// A single click selects, which is what it does everywhere else in the window.
    #[test]
    fn a_single_click_selects() {
        assert_eq!(hit_of(false, true), Some(Hit::Tick));
    }

    /// Nothing happened, and nothing is reported.
    #[test]
    fn no_click_is_no_hit() {
        assert_eq!(hit_of(false, false), None);
    }
}

/// The pages this build ships, and their order in the reader.
///
/// `include_str!` puts them in the binary, so they cannot disagree with the build somebody is
/// running - there is no version to keep in step and nothing to fetch. What is listed is the
/// *manual*: `DECISIONS.md` and `WORKLOG.md` are development record and stay in the repository.
const DOCS: &[oops_docs::Doc] = &[
    oops_docs::Doc::new(
        "targets",
        "Targets",
        "Registering a console, and asking what it can currently do",
        include_str!("../../docs/features/targets.md"),
    ),
    oops_docs::Doc::new(
        "payloads",
        "Payloads",
        "Why none are bundled, and what is checked before one runs",
        include_str!("../../docs/features/payloads.md"),
    ),
    oops_docs::Doc::new(
        "library",
        "The target's storage",
        "Titles, saves and packages; the log; moving files",
        include_str!("../../docs/features/library.md"),
    ),
];

#[cfg(test)]
mod docs_tests {
    /// `include_str!` proves a file *exists*. It cannot notice one truncated to nothing, two
    /// entries claiming a slug, or a page with no heading - and all three ship silently,
    /// because a documentation window showing an empty page looks like a page nobody wrote yet.
    #[test]
    fn the_registry_is_sound() {
        assert_eq!(oops_docs::check(super::DOCS), Vec::<String>::new());
    }
}

#[cfg(test)]
mod glyph_tests {
    /// **Nothing this window draws is outside plain ASCII.**
    ///
    /// The default font has no arrows and no triangles, so `->`, `▸` and `▾` all rendered as
    /// replacement boxes - and the fold arrows sat next to real checkboxes, which is why they
    /// read as broken checkboxes rather than as missing glyphs.
    ///
    /// Checked over the source rather than by looking, because looking is what missed it: a
    /// box that means *this font cannot draw that* looks like a box that means something.
    #[test]
    fn the_window_draws_nothing_a_font_might_not_have() {
        let source = include_str!("app.rs");
        for (number, line) in source.lines().enumerate() {
            // The escape form, which is how every one of them was written.
            assert!(
                !line.contains(concat!("\\", "u{2")),
                "app.rs:{} draws a glyph the font may not have: {}",
                number + 1,
                line.trim()
            );
        }
    }
}

#[cfg(test)]
mod role_tests {
    use pros_link::service::{LOADER, SERVICES};

    use super::role_of;

    /// **The loader's role is the one that explains the order.**
    ///
    /// Measured from the manager's source: it sends every entry to the loader, so nothing
    /// after that point loads unless the loader is already up.
    #[test]
    fn the_loader_says_why_it_comes_first() {
        let said = role_of(&LOADER).expect("the loader has a role");
        assert!(said.contains("loaded through it"), "{said}");
        assert!(said.contains("before them"), "{said}");
    }

    /// **A service with a structural part to play says so; one without stays quiet.**
    ///
    /// The loader, the file service, the shell and the manager each hold the chain up in a way
    /// this project enforces, so each can say why it belongs. `klogsrv` cannot: it is not
    /// required, it is not a way back, and it does not run the list. It is in a chain because
    /// somebody wants to be able to see what happened - a preference, and not something to be
    /// dressed up as a rule.
    #[test]
    fn only_a_structural_part_is_claimed_as_one() {
        for service in SERVICES {
            let structural = service.required || service.recovers || service.runs_lists;
            assert_eq!(
                role_of(service).is_some(),
                structural,
                "{} disagrees with its own flags",
                service.name
            );
        }
        let log = SERVICES
            .iter()
            .find(|service| service.name == "klogsrv")
            .expect("the log service");
        assert!(
            role_of(log).is_none(),
            "wanting a log is a preference, not a rule this can derive"
        );
    }

    /// **A payload with no role says nothing rather than something invented.** It is in the
    /// list because somebody put it there, and only they can say why.
    #[test]
    fn a_payload_with_no_role_says_nothing() {
        let plain = pros_link::service::Service::declared(
            "nanodns".to_owned(),
            53,
            "resolve names".to_owned(),
            false,
            false,
            false,
        );
        assert!(role_of(&plain).is_none());
    }

    /// A role declared in the catalogue reads the same as one compiled in - which is the
    /// point of the flags being config.
    #[test]
    fn a_declared_service_gets_the_same_explanation() {
        let rival = pros_link::service::Service::declared(
            "zftpd".to_owned(),
            2121,
            "move files".to_owned(),
            true,
            false,
            false,
        );
        let said = role_of(&rival).expect("required is a role");
        assert!(said.contains("no workflow without it"), "{said}");
    }
}

/// Roughly how long ago, for a person rather than for arithmetic.
///
/// **Rounded down and named in the largest unit that fits.** Nobody reading a payload table
/// wants to know that it was four thousand and twelve seconds ago.
fn how_long(seconds: u64) -> String {
    match seconds {
        0..=90 => "just now".to_owned(),
        91..=5400 => format!("{}m ago", seconds / 60),
        5401..=172_800 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}
