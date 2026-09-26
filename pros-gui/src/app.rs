//! Drawing, and nothing else.
//!
//! This module reads state and draws it. Every decision (what a registration is, what a
//! missing loader means, which files a loader accepts) lives in a crate below this one and is
//! reachable from `pros` too. The rules for changing state live in [`crate::state`], where
//! they can be tested without a window.
//!
//! Immediate mode fits because a check is a table replaced wholesale on every run, not a form
//! edited field by field.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pros_core::boot::Step as BootStep;
use pros_core::check::Verdict;
use pros_core::library::Kind as LibraryKind;
use pros_core::manifest::Manifest;
use pros_core::payloads::{Boot, Presence, Standing, There, Trust};
use pros_core::target;

use crate::state::{Job, Section, State};
use crate::work::Worker;

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
enum ProcSort {
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

/// How the log filter box is being read: plain text, or a regular expression.
///
/// Built once per frame from the box and its regex toggle, so the pattern compiles once rather
/// than per line.
enum LogMatch {
    /// An empty box: every line is kept.
    All,
    /// Plain text, matched without regard to ASCII case.
    Text(String),
    /// A compiled regular expression.
    Regex(regex_lite::Regex),
    /// The box holds a regular expression that does not compile.
    ///
    /// Keeps every line, so a half-typed pattern shows the log unfiltered while the toolbar
    /// says the pattern is invalid, rather than blanking it on each keystroke.
    Invalid,
}

impl LogMatch {
    /// Reads the filter box into a matcher.
    fn build(filter: &str, as_regex: bool) -> Self {
        let text = filter.trim();
        if text.is_empty() {
            return Self::All;
        }
        if as_regex {
            regex_lite::Regex::new(text).map_or(Self::Invalid, Self::Regex)
        } else {
            Self::Text(text.to_owned())
        }
    }

    /// Whether a line is kept by the current filter.
    fn keeps(&self, line: &str) -> bool {
        match self {
            Self::All | Self::Invalid => true,
            Self::Text(needle) => contains_ignore_ascii_case(line, needle),
            Self::Regex(regex) => regex.is_match(line),
        }
    }

    /// Whether the box holds a regex that will not compile.
    const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid)
    }
}

/// An ASCII-case-insensitive substring test that allocates nothing.
///
/// The filter runs over every kept line on each frame a line arrives, so it must not allocate
/// per line. Logs are ASCII, so folding only the ASCII range is enough.
fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let (haystack, needle) = (haystack.as_bytes(), needle.as_bytes());
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    haystack
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
}

/// One row of a listing: a tick, a name, and what that side knows about it.
///
/// Returns what the row was asked to do, if anything. There are no action buttons: what can be
/// done depends on the whole selection, so it lives in the toolbar.
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

    let side = if target_side {
        entry.there.as_ref()
    } else {
        entry.here.as_ref()
    };
    let folder = target_side && entry.folder_there();
    if folder {
        // A double click opens, so a single click can still select the folder.
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
/// Not a listing entry: `..` is filtered out on the way in so a walk cannot climb out of the
/// directory asked about. This row puts the navigation back, apart from the copying.
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
    // One click: there is nothing to select here. In egui a double click is also a click.
    up.clicked()
}

/// Draws a path box, and says whether Return was pressed in it.
///
/// Focus lost and Return, not the key alone: a focused text box reports the key on every frame
/// it is held, which would start the same navigation repeatedly.
fn entered(ui: &mut egui::Ui, field: egui::TextEdit<'_>) -> bool {
    let response = ui.add(field.desired_width(f32::INFINITY));
    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
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
/// The order matters: in egui `double_clicked()` is `clicked && is_double`, so both arrive on
/// the same frame and the double click has to be tested first.
const fn hit_of(double_clicked: bool, clicked: bool) -> Option<Hit> {
    if double_clicked {
        Some(Hit::Open)
    } else if clicked {
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
fn side_cell(ui: &mut egui::Ui, side: Option<&crate::listing::Side>) {
    match side {
        Some(crate::listing::Side { folder: true, .. }) => {
            ui.weak("dir");
        }
        Some(crate::listing::Side { size: bytes, .. }) => {
            ui.label(bytes.map_or_else(|| "yes".to_owned(), size));
        }
        // Empty rather than a symbol: the full-or-empty contrast with the other column is the
        // information.
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
/// The offer differs per verdict.
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
        // Several would do, so this asks: how a target boots is the user's choice.
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
        // Every step is already done and the check still fails, so there is nothing to
        // offer.
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
/// Counts what was started, not what worked: nothing has finished yet at this point, and the
/// result is checked and reported separately at the end.
fn summarise(queued: usize, edited: usize, could_not: &[String]) -> String {
    let mut said = match (queued, edited) {
        (0, 0) => "nothing to do".to_owned(),
        (0, _) => format!("{edited} list edits ready - review and save them"),
        (_, 0) => format!("{queued} steps started"),
        // The edits wait on the transfers, so they are not called ready.
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
/// A boxed closure rather than an enum of the operations, so each is named once, as the call
/// it makes.
type Edit = Box<dyn FnOnce(&mut pros_core::boot::Boot) -> bool>;

/// The payload at a position in the list as it was before an edit.
fn boot_name(boot: Option<&pros_core::boot::Boot>, at: usize) -> Option<&String> {
    boot?.steps.get(at).map(|step| &step.payload)
}

/// The top of a section: its name, what it is for, and a rule under both.
///
/// One function for every section, so all headings carry the same explanation.
fn section_heading(ui: &mut egui::Ui, section: Section) {
    section_heading_with(ui, section, |_| ());
}

/// The same top, with controls beside the name.
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
/// Derived from catalogue flags rather than written per payload, so a service declared in
/// `services.json` gets the same explanation as a built-in one. `None` for a payload with no
/// role.
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
/// Only ever a recorded note. The column beside it says what a payload is; this one says why
/// it is at this point in the list, which nothing on the target records. It never falls back
/// to the description, so an empty cell means nobody has recorded a reason.
fn reason(ui: &mut egui::Ui, known: Option<String>) {
    match known {
        Some(text) => {
            ui.weak(text);
        }
        // Nothing to edit here: a reason belongs in `data/chain.json`, beside every other
        // fact about a payload.
        None => {
            ui.weak("-")
                .on_hover_text("no ordering requirement recorded for this payload");
        }
    }
}

/// The title row of a listing grid.
///
/// Drawn as a row of the grid rather than above it, so it shares the grid's column widths.
fn headings(ui: &mut egui::Ui, titles: &[&str]) {
    for title in titles {
        ui.small(egui::RichText::new(*title).weak());
    }
    ui.end_row();
}

/// A group heading inside a listing grid, and whether its rows are hidden.
///
/// A row in the grid rather than a widget around it, so every group's columns line up.
///
/// Returns `true` when the group is folded and its rows should be skipped. The set records
/// what is folded, not what is open, so a group that appears later starts open.
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
/// `set_width` alone is a request: a pane grows to fit wide content and pushes its neighbour
/// off the edge. Each pane gets a hard maximum and scrolls both ways inside it.
fn pane(ui: &mut egui::Ui, salt: &str, size: egui::Vec2, body: impl FnOnce(&mut egui::Ui)) {
    // Top-down explicitly: a plain `allocate_ui` inherits the parent's horizontal direction.
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
/// equal. The height is what is left, which bounds the scroll area; an unbounded one grows to
/// fit its content and never scrolls.
fn half_of(ui: &egui::Ui) -> egui::Vec2 {
    egui::vec2(
        ((ui.available_width() - GAP) * 0.5).max(120.0),
        ui.available_height().max(120.0),
    )
}

/// The same, split where somebody dragged it to.
///
/// `share` is the left pane's fraction of the usable width, so the split survives a resize; a
/// split held in pixels creeps towards one edge as the window shrinks.
fn split_at(ui: &egui::Ui, share: f32) -> (egui::Vec2, egui::Vec2) {
    let usable = (ui.available_width() - GAP - HANDLE).max(240.0);
    let height = ui.available_height().max(120.0);
    let left = (usable * share).clamp(120.0, usable - 120.0);
    (egui::vec2(left, height), egui::vec2(usable - left, height))
}

/// How wide the draggable divider is.
///
/// The grabbable area, wider than the line drawn inside it.
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
/// `None` when the dialog was closed without choosing, which is an answer and not a failure.
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

/// Asks where to save a file, suggesting a name.
///
/// `None` when the dialog was dismissed without choosing.
fn choose_where_to_save(suggested: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("save the log")
        .add_filter("log", &["log"])
        .set_file_name(suggested)
        .save_file()
}

/// The filter, copy and save controls a captured log carries - the log screen's, and the probe
/// screen's, which is the same view over a different capture.
///
/// Returns what the caller has to say: `Ok` for news, `Err` for trouble.
fn filter_controls(
    ui: &mut egui::Ui,
    lines: &[String],
    filter: &mut String,
    regex: &mut bool,
    matcher: &LogMatch,
    save_as: &str,
) -> Option<Result<String, String>> {
    ui.label("filter");
    ui.add(
        egui::TextEdit::singleline(filter)
            .desired_width(160.0)
            .hint_text(if *regex {
                "regex to keep"
            } else {
                "text to keep"
            }),
    )
    .on_hover_text(
        "keep only the lines this matches - plain text ignoring case, or a regular expression \
         with the box ticked. What is hidden is still kept.",
    );
    ui.checkbox(regex, "regex")
        .on_hover_text("match the filter as a regular expression instead of plain text");
    // An invalid pattern is not applied; this says why every line is shown.
    if matcher.is_invalid() {
        ui.colored_label(egui::Color32::from_rgb(220, 170, 90), "invalid regex")
            .on_hover_text("the pattern does not compile yet, so every line is shown");
    }
    // Both numbers when filtered, so a filtered view does not read as a quiet target.
    let kept = lines.iter().filter(|line| matcher.keeps(line)).count();
    if filter.trim().is_empty() {
        ui.weak(format!("{} lines", lines.len()));
    } else {
        ui.weak(format!("{kept} of {} lines", lines.len()));
    }
    let shown = || {
        lines
            .iter()
            .filter(|line| matcher.keeps(line))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    };
    if ui
        .add_enabled(kept > 0, egui::Button::new("copy"))
        .on_hover_text("copy what is shown to the clipboard, filter and all")
        .on_disabled_hover_text("nothing to copy")
        .clicked()
    {
        let text = shown();
        ui.output_mut(|out| out.copied_text = text);
    }
    // Saves exactly what is shown, filter and all, like `copy`.
    if ui
        .add_enabled(kept > 0, egui::Button::new("save"))
        .on_hover_text("write what is shown to a .log file you choose, filter and all")
        .on_disabled_hover_text("nothing to save")
        .clicked()
        && let Some(path) = choose_where_to_save(save_as)
    {
        // A trailing newline, so a later append does not run onto the last line.
        let mut text = shown();
        text.push('\n');
        return Some(match std::fs::write(&path, text) {
            Ok(()) => Ok(format!("saved {kept} lines to {}", path.display())),
            Err(why) => Err(format!("could not save the log: {why}")),
        });
    }
    None
}

/// A captured log's lines, filtered, in a view that lays out only the rows on screen.
///
/// Virtualized like the log panel, and pinned to the bottom while `live`.
fn filtered_rows(ui: &mut egui::Ui, salt: &str, lines: &[String], matcher: &LogMatch, live: bool) {
    let shown: Vec<&str> = lines
        .iter()
        .filter(|line| matcher.keeps(line))
        .map(String::as_str)
        .collect();
    if shown.is_empty() {
        if !lines.is_empty() {
            ui.weak("nothing matches that filter - the lines are still kept");
        }
        return;
    }
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .stick_to_bottom(live)
        .show_rows(ui, row_height, shown.len(), |ui, range| {
            for row in range {
                ui.monospace(shown[row]);
            }
        });
}

/// Where the manager keeps the payload files it loads.
///
/// Measured on a target: one folder per payload, with the file inside it.
const PAYLOADS: &str = "/data/pldmgr/payloads";

/// What the separator between two panes costs, with its padding.
const GAP: f32 = 24.0;

/// How often the system panel re-reads the target when auto-refresh is on.
///
/// Each tick is a shell round trip. The same interval as the default of `pros top`.
const SYSTEM_REFRESH: Duration = Duration::from_secs(3);

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
    /// Which services exist and what each is for: defaults, then this project's own file.
    ///
    /// Read once, at start: it says what a service means, not what a target is doing, so it
    /// does not expire on a power cycle.
    catalogue: pros_core::catalogue::Catalogue,
    /// The log being followed, when one is.
    ///
    /// Beside the worker rather than inside it: the worker runs one job at a time, and a
    /// subscription would block everything else the window can do.
    tail: Option<crate::tail::Tail>,
    /// The probe running, or the last one, when there is one.
    ///
    /// Beside the worker for the same reason as the log: a probe is a launch followed by a
    /// subscription lasting up to its cap.
    probe: Option<crate::probe::Run>,
    /// What each payload's own project has released, as far as anything has asked.
    ///
    /// Read from disk at start and written back as answers arrive, so the next launch does not
    /// ask again and hit the rate limit.
    sources: pros_core::sources::Sources,
    /// Whether the sweep that runs on its own has been started.
    ///
    /// A flag rather than starting it in `new`, so the window opens before any asking starts.
    asked_at_launch: bool,
    /// A sweep of those projects, while one is running.
    ///
    /// Beside the worker because it is deliberately slow (spaced out, waiting out refusals) and
    /// would block the one-job queue for its whole length.
    sweep: Option<crate::sweep::Sweep>,
    /// How the system panel's process list is ordered: a view choice, not a fact about the
    /// target.
    system_sort: ProcSort,
    /// Whether the system panel re-reads the target on its own.
    ///
    /// With it on, the panel re-runs `ReadSystem` when idle and the interval has passed. Off by
    /// default: reading the target is a round trip, not something to do unasked.
    system_auto: bool,
    /// When the panel last asked the target, for the auto-refresh interval.
    system_asked_at: Option<std::time::Instant>,
}

impl App {
    /// Opens with whatever is registered on this machine.
    #[must_use]
    pub(crate) fn new() -> Self {
        // An unreadable registry does not stop the window: it can still register a target.
        let targets = target::load().unwrap_or_default();
        // The manifest in the usual place, or the built-in recommended list, so a fresh install
        // shows what a target ought to be running.
        let manifest = pros_core::manifest::Tracked::Payloads
            .read()
            .ok()
            .or_else(|| {
                let seed = pros_core::manifest::recommended();
                let _ = seed.save();
                Some(seed)
            });

        Self {
            state: State::new(targets),
            worker: Worker::new(),
            stamp: pros_core::build::line(),
            docs: oops_docs::DocsWindow::default(),
            tail: None,
            probe: None,
            sweep: None,
            asked_at_launch: false,
            sources: pros_core::sources::load(),
            manifest,
            // Defaults when no file exists, the normal case (`pros_core::catalogue`).
            catalogue: pros_core::catalogue::load()
                .unwrap_or_else(|_| pros_core::catalogue::Catalogue::builtin()),
            system_sort: ProcSort::default(),
            system_auto: false,
            system_asked_at: None,
        }
    }

    /// Takes in whatever the sweep has answered, and keeps it.
    ///
    /// Written to disk as answers arrive, not at the end, so an interrupted sweep keeps what it
    /// learnt.
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
            // Said, because nothing visible would otherwise happen.
            self.state.said = if wanted == 0 {
                "every project with a release page was asked recently - nothing to ask".to_owned()
            } else {
                "nothing to ask".to_owned()
            };
        }
    }

    /// The menu bar.
    ///
    /// Every control that does not apply is disabled rather than hidden, and says why on
    /// hover: a control that vanishes reads as a bug, a greyed one reads as a state.
    fn menu_bar(&mut self, ctx: &egui::Context) {
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
                        self.state.editing = None;
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
                            self.state.name.clone_from(&target.name);
                            self.state.address.clone_from(&target.address);
                            self.state.editing = Some(target.name.clone());
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
    fn register_dialog(&mut self, ctx: &egui::Context) {
        let editing = self.state.editing.clone();
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
                        ui.text_edit_singleline(&mut self.state.name);
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("address");
                    ui.text_edit_singleline(&mut self.state.address);
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
                    .unwrap_or_else(|| self.state.name.trim().to_owned());
                let can = !name.trim().is_empty() && !self.state.address.trim().is_empty();
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
                    match target::register(name.trim(), self.state.address.trim()) {
                        Ok(_) => {
                            self.state.targets = target::load().unwrap_or_default();
                            // Keep the just-saved target selected rather than jumping to the first.
                            self.state.chosen = self
                                .state
                                .targets
                                .iter()
                                .position(|one| one.name == name.trim());
                            self.state.address.clear();
                            self.state.editing = None;
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
            self.state.editing = None;
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

    /// Stages anything dropped on the window.
    ///
    /// A dropped file is checked against the manifest entry whose file name it matches before
    /// it is kept.
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
                None => self.state.adhoc = Some(path.clone()),
            }
        }
    }

    /// Puts each section's two sides where they belong, the first time it is shown.
    ///
    /// Only the first time, so navigation survives a visit to another section.
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
    /// Beside the registry, one directory per section, so the payload staging directory and
    /// the payloads section are the same folder.
    fn local_place(section: Section) -> Option<PathBuf> {
        Some(target::directory()?.join(section.name()))
    }

    /// Reads the local side.
    ///
    /// Synchronously, unlike the target side: it is a local directory read.
    fn read_local(&mut self) {
        let path = PathBuf::from(self.state.local_path.trim());
        match pros_core::library::here(&path) {
            Ok(items) => self.state.local = items,
            Err(why) => {
                self.state.local.clear();
                self.state.trouble = Some(why.to_string());
            }
        }
    }

    /// This machine on the left, the target on the right, and the traffic between them.
    ///
    /// Both at once because the question is comparative. The left side works with no target
    /// registered.
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
                // Kept as a fraction of the usable width, so it survives a resize.
                let usable = (left.x + right.x).max(1.0);
                self.state.split = (self.state.split + dragged / usable).clamp(0.15, 0.85);
            }
        }
    }

    /// Rebuilds the merged listing from the two sides, keeping what is still selected.
    ///
    /// Rebuilt every frame from the sides rather than patched, so it cannot drift from them.
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
    /// Disabled with the reason on hover rather than hidden.
    fn sync_toolbar(&mut self, ui: &mut egui::Ui) {
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        let mut act = None;
        ui.horizontal_wrapped(|ui| {
            // Only what could ever apply to this section (`Offer::applies_to`).
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
            // One list or two: the split is a projection of the merged model.
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
        // Both deletes take the whole selection in one job, so the confirm is asked once.
        if offer.is_destructive() {
            self.state.pending_delete = Some((offer, picked));
            return;
        }
        let local = PathBuf::from(self.state.local_path.trim());
        let remote = self.state.library_path.trim_end_matches('/').to_owned();

        // Install is a confirm, not a job: one panel names every selected package.
        if offer == Offer::Install {
            self.state.pending_install =
                Some(picked.iter().map(|entry| local.join(&entry.name)).collect());
            self.state.listing.chosen.clear();
            return;
        }

        // Every selected entry is queued; the worker still runs one job at a time.
        let mut asked = 0_usize;
        for entry in picked {
            let job = match offer {
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
                        on_target(&remote, &entry, there),
                    )),
                    (None, None) => None,
                },
                Offer::Send => {
                    let Some(here) = entry.here.as_ref() else {
                        continue;
                    };
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
                            remote.clone(),
                        ))
                    } else {
                        // Not a described payload, so there is no folder name: a bare file is
                        // copied where the browser is pointed.
                        let to = format!("{remote}/{}", here.name);
                        Some(Job::Push(target.clone(), from, to))
                    }
                }
                Offer::Fetch => {
                    let Some(there) = entry.there.as_ref() else {
                        continue;
                    };
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
                    .map(|payload| Job::Fetch(Box::new(payload), Some(local.clone()))),
                Offer::Launch => Some(Job::Launch(target.clone(), entry.name.clone())),
                // Both handled above, each in one go for the whole selection.
                Offer::Install | Offer::DeleteHere | Offer::DeleteThere => None,
            };
            if let Some(job) = job {
                self.state.queue(job);
                // Unticked as it is queued, so what stays ticked is what was not taken up.
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
        // Return navigates, so an edited path never sits on screen looking applied.
        if entered(ui, egui::TextEdit::singleline(&mut self.state.local_path)) {
            self.state.listing.chosen.clear();
            self.read_local();
        }
        ui.separator();

        // Entries this side knows about, plus anything described and on neither side: that is
        // something to fetch onto this machine.
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
            // Which device. The places under each come from `pros_core::places`, a table of
            // measured paths with the payload that owns each one.
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
                        // A device with nothing measured is shown disabled, not left out: an
                        // absent entry reads as a device that is not there.
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
            // Only where there are titles to name.
            let titles: Vec<String> = self
                .state
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
        });
    }

    /// Reads the keyboard and sends a pad record, every frame.
    ///
    /// Ticks from `update` unconditionally, not from the panel that draws pads, so input keeps
    /// flowing while another section (the stream above all) is shown.
    fn drive_pads(&mut self, ctx: &egui::Context) {
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

        let asking = self.state.binding.take();
        if let Some(button) = asking {
            // The first key pressed while waiting takes the binding; Escape abandons it.
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
        // A feed that is not open counts these as dropped, which separates a broken
        // connection from a broken mapping.
        self.state.feed.send(&records);
    }

    /// Controllers presented to the target from this machine.
    ///
    /// The keyboard drives up to four slots, each sending under its own number. A slot set to
    /// a physical controller says nothing can read it: the workspace forbids unsafe code, so
    /// the platform APIs would need a dependency.
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
    /// Not connected, sending and connection ended are drawn differently, because each needs
    /// different action.
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
                // The error is kept in the feed's status, which the line below draws.
                let _ = self.state.feed.open(&target.address, port);
            }
            ui.small("port:");
            ui.add(egui::TextEdit::singleline(&mut self.state.feed_port).desired_width(60.0));

            let colour = match &self.state.feed.status {
                pros_link::feed::Status::Sending => egui::Color32::from_rgb(120, 200, 140),
                pros_link::feed::Status::Idle => egui::Color32::GRAY,
                pros_link::feed::Status::Lost(_) | pros_link::feed::Status::Refused(_) => {
                    egui::Color32::from_rgb(220, 120, 120)
                }
            };
            ui.colored_label(colour, self.state.feed.status.describe());
        });

        if sending {
            ui.small(format!("{} records sent", self.state.feed.sent));
        } else {
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
    /// Shown rather than resolved: silently unbinding one would leave a dead button nobody was
    /// told about.
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
            self.state.binding = Some(button);
        }
    }

    /// One slot's key layout, and a way to change it.
    ///
    /// Per slot, not shared: two people on one keyboard need two layouts.
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

    /// What the target is: firmware, storage, and what is running.
    ///
    /// Nothing is filled in from anything else: a field the target did not answer stays
    /// empty, because a plausible value is indistinguishable from a measured one.
    fn system_panel(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, Section::System);

        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
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
        ui.add_space(8.0);

        // Auto-refresh: the timestamp keeps this from asking every frame, `idle` keeps it from
        // stacking reads, and `request_repaint_after` wakes the window to check.
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

    /// What the target loads at startup, and the manager's settings.
    ///
    /// The file written here decides what loads at boot; a wrong one leaves the target without
    /// its file service or loader, and recovery is re-running the entry point by hand. So an
    /// edit produces a diff, and the write happens on a second explicit press with those lines
    /// on screen.
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
                // Not emptied first: the list stays up while it is re-read, with a notice.
                self.state.followed_for = None;
                self.state.begin(Job::ReadAutoload(target));
            }
            // Which list. The manager keeps one at a fixed path; the autoloader that runs
            // before it looks in several places, and its list decides whether the manager runs
            // at all. They are audited by opposite rules.
            let held = self.state.list();
            egui::ComboBox::from_id_salt("which-list")
                .selected_text(&held.label)
                .show_ui(ui, |ui| {
                    for (at, one) in self.state.lists.clone().iter().enumerate() {
                        if ui
                            .selectable_label(self.state.list_at == at, &one.label)
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
                                self.state.begin(Job::ReadList(target, one.clone()));
                            }
                        }
                    }
                });
            // Export: reads a working list out as a chain preset, the reverse of deploying one.
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
                self.begin_export(&held);
            }
            ui.weak(held.path);
            if !held.editable {
                ui.colored_label(egui::Color32::from_rgb(210, 190, 120), "read only");
            }
        });
        ui.separator();

        self.list_findings(ui, &self.state.list(), idle);
        self.boot_list(ui, connected);
        ui.add_space(10.0);
        // The settings belong to the manager only. Under an autoloader's list they would
        // change a different file from the one on screen, and `AUTOLOAD_ENABLED` under the
        // wrong list can leave the target unable to start its services.
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
    /// A step holds the line as written (`kstuff-lite_v1.09.elf`, `#` in front when off); a
    /// preset entry is the bare name. The chain parser does that translation, and is the one
    /// answer to whether two lines name the same payload.
    fn begin_export(&mut self, held: &pros_core::chain::Held) {
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
                if held.autoloader {
                    "autoloader"
                } else {
                    "manager"
                }
            ),
            preset,
            notes,
            // The companion files are a round trip, so the panel opens now and they fill in
            // when the read below returns. Nothing is written while this is set.
            capturing: true,
            disabled,
            into: pros_core::recovery::baseline::path().map_or_else(
                || "nowhere on this machine".to_owned(),
                |at| at.display().to_string(),
            ),
            taken: pros_core::recovery::baseline::all()
                .0
                .into_iter()
                .map(|one| one.name)
                .collect(),
        });
        // Reads the declared companion files off the target so the chain carries them.
        if let Some(target) = self.state.target().cloned() {
            self.state.queue(Job::CaptureConfig(target));
        } else if let Some(export) = self.state.exporting.as_mut() {
            export.capturing = false;
        }
    }

    /// The warning under the name field: what is wrong with the typed name, or what it replaces.
    fn export_name_notice(
        ui: &mut egui::Ui,
        name: &str,
        is_shipped: bool,
        usable: bool,
        already_taken: bool,
    ) {
        if name.is_empty() {
            ui.colored_label(egui::Color32::from_rgb(230, 160, 90), "it needs a name");
        } else if is_shipped {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                format!(
                    "'{name}' is a built-in chain provided by Prosperous and cannot be overwritten. \
                     Choose a custom name for your chain."
                ),
            );
        } else if !usable {
            ui.colored_label(
                egui::Color32::from_rgb(230, 90, 90),
                "a preset name is one word - a target's registry line is whitespace-delimited, \
                 so half of this would be read as an address",
            );
        } else if already_taken {
            ui.colored_label(
                egui::Color32::from_rgb(230, 160, 90),
                format!("there is already a custom preset called {name}, and this replaces it."),
            );
        }
    }

    /// The files the chain carries beside its list: settings files read off the target and put
    /// back verbatim on deploy, shown by path and size before anything is written.
    fn export_files_shown(ui: &mut egui::Ui, export: &crate::state::Exporting) {
        ui.add_space(4.0);
        if export.capturing {
            ui.weak("    reading the files this chain carries...");
        } else if export.preset.files.is_empty() {
            ui.weak("    no settings files carried - just the payload order");
        } else {
            ui.label("and it carries these files, put back as they are on deploy:");
            for file in &export.preset.files {
                ui.weak(format!(
                    "    {} - {} ({} bytes)",
                    file.label,
                    file.path,
                    file.content.len()
                ));
            }
        }
    }

    /// What would be written down, where, and what it could not know.
    ///
    /// It asks before writing because a preset replaces by name, so the typed name decides
    /// whether an existing preset is replaced.
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
        let is_shipped = pros_core::recovery::baseline::is_shipped_name(&name);
        // One word: the registry line is whitespace-delimited, so a name with a space would be
        // written as `chain=<half>` and the rest read as an address.
        let usable = !name.is_empty() && !name.contains(char::is_whitespace) && !is_shipped;
        Self::export_name_notice(ui, &name, is_shipped, usable, export.taken.contains(&name));

        ui.weak(format!("into {}", export.into));
        ui.add_space(4.0);
        ui.label(format!(
            "{} entries, in this order:",
            export.preset.entries.len()
        ));
        // In preset order, not read order: for a manager's list they can differ, and the
        // preset order is what will be written.
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
                if export.disabled == 1 {
                    "line"
                } else {
                    "lines"
                }
            ));
        }

        Self::export_files_shown(ui, export);

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
        // Not while the files are still being read, or the chain would carry the list without
        // its files.
        let ready = usable && !export.capturing;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new("write it"))
                .on_disabled_hover_text(if export.capturing {
                    "still reading the files this chain carries"
                } else {
                    "give it a one-word name first"
                })
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
        // Straight to the filesystem: one small local file, and the queue is for target work.
        self.state.said = match pros_core::recovery::baseline::keep(&preset) {
            Ok(at) => {
                self.state.exporting = None;
                format!(
                    "{} written to {} - it is offered as a chain from the next start",
                    preset.name,
                    at.display()
                )
            }
            // Kept open on failure: the panel holds the only copy of what was read.
            Err(why) => format!("not written: {why}"),
        };
    }

    /// Setting a target up from nothing: which list, what would go in it, and a warning.
    ///
    /// It replaces a whole startup list, possibly the removable one kept as the way back in, so
    /// it asks twice: the plan says what will be fetched and sent, then the resulting file goes
    /// through the whole-file review that says what the target will try to run.
    fn configurator(&mut self, ui: &mut egui::Ui, idle: bool) {
        let Some(at) = self.state.setting_up else {
            return;
        };
        let chosen = self.state.lists.get(at).cloned();
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

        self.setup_choices(ui, at, &held);
        // What is there now, counted only for the list being shown: this panel makes no
        // requests.
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
            self.plan_a_setup(&held);
        }
    }

    /// The two questions the configurator asks before it will plan anything.
    ///
    /// Which chain, then which list: what the target runs, then where the file goes.
    fn setup_choices(&mut self, ui: &mut egui::Ui, at: usize, held: &pros_core::chain::Held) {
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
                .selected_text(&held.label)
                .show_ui(ui, |ui| {
                    for (which, one) in self.state.lists.clone().iter().enumerate() {
                        if ui
                            .selectable_label(which == at, &one.label)
                            .on_hover_text(&one.path)
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
        // An unreadable chains file is reported, not silently replaced by the shipped presets.
        if let Some(why) = trouble {
            ui.colored_label(egui::Color32::from_rgb(230, 90, 90), why);
        }
        if let Some(one) = presets.iter().find(|one| one.name == self.state.preset) {
            ui.weak(&one.about);
            if !one.result.is_empty() {
                ui.add_space(4.0);
                ui.label("what you end up with:");
                // Printed exactly as the chains file states it.
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
    fn plan_a_setup(&mut self, held: &pros_core::chain::Held) {
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        let preset = pros_core::recovery::baseline::named(&self.state.preset)
            .unwrap_or_else(pros_core::recovery::baseline::first);
        // Recorded on the registration, because later checks and fixes are judged against the
        // chain the target is meant to run. Recorded when the plan is made: it is the decision,
        // whether or not the plan is carried out.
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
                // Not fatal: the plan still holds, but the next check will not know the chain.
                Err(why) => {
                    self.state.trouble = Some(format!("the chain was not recorded: {why}"));
                }
            }
        }
        // Every list the chain has: a chain that runs the manager has the autoloader's list and
        // the manager's own, and writing one leaves the target half configured. The chosen list
        // goes where it was chosen; any other is written only when it has exactly one possible
        // path (the manager's is compiled in, an autoloader's has several candidates).
        let mut writing = vec![(held.path.clone(), kind)];
        for one in &preset.lists {
            let its_kind = if one.autoloader {
                pros_core::recovery::Kind::Autoloader
            } else {
                pros_core::recovery::Kind::Manager
            };
            if its_kind == kind || one.at.len() != 1 {
                continue;
            }
            let only = &one.at[0];
            if !writing.iter().any(|(path, _)| path == only) {
                writing.push((only.clone(), its_kind));
            }
        }

        let of = preset.clone();
        let planned = writing.clone();
        let (plan, left_out) = self.with_known(move |known| {
            let mut plans = Vec::new();
            let mut missed = Vec::new();
            for (path, kind) in &planned {
                let (one, out) = pros_core::doctor::provision(known, path, *kind, &of);
                plans.push(one);
                missed.extend(out);
            }
            missed.sort_unstable();
            missed.dedup();
            (pros_core::doctor::Plan::all_of(&plans), missed)
        });
        self.state.setting_up = None;
        // A payload with no route is left out (an entry the loader cannot find fails at every
        // boot), and named here.
        if !left_out.is_empty() {
            self.state.said = format!("left out, with no way to get them: {}", left_out.join("; "));
        }
        self.state.pending_plan = Some(crate::state::Pending {
            id: format!("set up {}", held.path),
            label: if writing.len() > 1 {
                format!(
                    "{} runs the {} chain - {} lists",
                    held.label,
                    preset.name,
                    writing.len()
                )
            } else {
                format!("{} runs the {} chain", held.label, preset.name)
            },
            plan,
        });
    }

    /// What is wrong with this list, on the screen where it is edited.
    ///
    /// The check screen audits only the manager's own list; this one audits whichever list is
    /// shown, while it is being edited. List checks only, not what is answering now.
    fn list_findings(&mut self, ui: &mut egui::Ui, held: &pros_core::chain::Held, idle: bool) {
        let kind = if held.autoloader {
            pros_core::recovery::Kind::Autoloader
        } else {
            pros_core::recovery::Kind::Manager
        };
        // Parsed from what is on screen, so an unsaved edit is audited as it is made.
        let shown = self
            .state
            .boot
            .as_ref()
            .map(|boot| pros_core::chain::Chain::parse(&boot.to_text()));
        let findings = self.with_known_of(
            shown.as_ref(),
            kind,
            Some(held.path.as_str()),
            pros_core::doctor::examine_list,
        );
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
        // A list with nothing wrong says so once, below the table.
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
    /// One row is selected, not several: moving a step moves one step, and moving several has
    /// an order nobody stated.
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
        // Only a change to this file: a settings edit is also a pending change, and belongs to
        // the settings panel.
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
                // What a payload is comes from its publisher, most specific first: the sidecar
                // the manager wrote at install, the payload list, then the catalogue. Why it is
                // at this point in the chain is a separate column (`why` below).
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
                // The audit of the list as it would be, so a proposed change can give its reason.
                let hazards = pros_core::recovery::audit(
                    &pros_core::chain::Chain::parse(&boot.to_text()),
                    &known,
                    there.as_deref().unwrap_or_default(),
                    // The rules invert: the loader is required in an autoloader list and
                    // impossible in the manager's own.
                    if held.autoloader {
                        pros_core::recovery::Kind::Autoloader
                    } else {
                        pros_core::recovery::Kind::Manager
                    },
                    &chain_here,
                    loader_here,
                );
                // Why an entry is here, most specific first: a recorded note, the audit finding
                // that proposed the change, the tracked recommendation, then the role derived
                // from the catalogue's flags.
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
                            // The tracked recommendation, a fact about the payloads.
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
                // The list as it stands, with the change marked in place. A removed entry keeps
                // its row and number: it is still in the list on the target.
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
                    // Selection works on the pending list, so a removed entry cannot be picked.
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
                    // A removed entry has no step: it is not in the pending list.
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
                    // Three states: a set nobody has read is not an empty one. A disabled entry
                    // is off on purpose, never missing.
                    let missing = (!step.is_disabled())
                        .then(|| {
                            there
                                .as_ref()
                                .map(|there| !there.iter().any(|one| one.name == step.name()))
                        })
                        .flatten();
                    let label = if step.is_disabled() {
                        // In the list, and will not load.
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
                    // Where its file is, which decides whether the manager can resolve it.
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
                    // The instruction that precedes it, shown because reordering carries it.
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
                // The selection follows the row, not the position.
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
    /// Returned rather than applied, because applying borrows the list these were drawn from.
    fn boot_controls(
        &mut self,
        ui: &mut egui::Ui,
        boot: &pros_core::boot::Boot,
        at: Option<usize>,
    ) -> Option<Edit> {
        let last = boot.steps.len().saturating_sub(1);
        let mut act: Option<Edit> = None;
        // Nothing is offered for a list this will not write, so no edit is made and lost.
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
            // Only what is on the target, found inside the manager's folders (it keeps
            // `payloads/<name>/<name>_<version>.elf`). Not scanned yet and found nothing are
            // drawn differently.
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
                    // The bare name, not the path: the manager resolves filenames. A payload it
                    // can never resolve (on removable storage outside its folder) is tagged and
                    // cannot be picked (`pros_core::payloads::Where`).
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
        // Drawn from the pending edit when there is one, so a second click undoes the first.
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
                    // A one-or-zero setting gets a switch; anything else is shown read-only,
                    // so no value of unknown shape is rewritten.
                    if value == "0" || value == "1" {
                        let mut on = value == "1";
                        if ui.checkbox(&mut on, "").changed() {
                            let wanted = if on { "1" } else { "0" };
                            // Applied to what is pending and diffed against the target's copy,
                            // so setting a value back clears the edit.
                            let next = shown.set(key, wanted).map(|edit| edit.now);
                            match next {
                                Some(now) if now.trim() == settings.text().trim() => undo = true,
                                Some(now) => {
                                    change = Some(pros_core::autoload::Change {
                                        was: settings.text().to_owned(),
                                        now,
                                        what: format!("{key} = {wanted}"),
                                        into: pros_core::autoload::CONFIG.to_owned(),
                                    });
                                }
                                None => {}
                            }
                        }
                        let name = if settings.get(key) == Some(value.as_str()) {
                            egui::RichText::new(key)
                        } else {
                            // Changed and not written.
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
        // A change to a read-only list is dropped rather than offered for writing.
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
        // The change itself is marked in the table above; this shows the whole file as sent.
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
    /// Audited on what is about to be written, not on what is there.
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
        // Repairs are offered alongside the warning, so writing anyway is not the only action.
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
            // The write button is named differently when the write is dangerous.
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
                self.state.begin(Job::WriteAutoload(
                    target,
                    change.into.clone(),
                    change.now.clone(),
                ));
                self.state.pending_change = None;
            }
            if ui.button("discard").clicked() {
                self.state.pending_change = None;
                // Re-read, so the screen shows the target's settings rather than the discarded
                // edit.
                if let Some(target) = self.state.target().cloned()
                    && idle
                {
                    self.state.begin(Job::ReadAutoload(target));
                }
            }
        });
        // After the panel: applying borrows the state it was drawn from.
        if !fix_first.is_empty() {
            self.apply_fixes(&fix_first);
        }
    }

    /// What a delete would remove, before it removes it.
    ///
    /// Lists every selected entry and names the side, because a selection made across a fold
    /// or left over from a changed listing is how the wrong thing gets deleted.
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
        // A folder takes everything under it, so folders are called out.
        let folders = what
            .iter()
            .filter(|entry| offer == crate::listing::Offer::DeleteThere && entry.folder_there())
            .count();
        if folders > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(220, 120, 120),
                format!(
                    "{folders} of these {} a folder - everything inside goes too",
                    if folders == 1 { "is" } else { "are" }
                ),
            );
        }
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
                    let paths = what
                        .iter()
                        .map(|entry| (format!("{root}/{}", entry.name), entry.folder_there()))
                        .collect();
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
    /// Not refused, because the build-run-read loop of homebrew development should not need a
    /// manifest entry per build. An undescribed file can be run, which leaves nothing behind,
    /// or kept, which does and says so.
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
    /// An install, unlike a copy, has the target unpack and register the package, and nothing
    /// here undoes it. The confirm names each file, and the target's answer is reported in its
    /// own words.
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
    /// A panel rather than a greyed button: whether a copy is refused depends on its
    /// destination, known only when it is asked for. The override is offered but is never the
    /// default.
    fn refusal(&mut self, ui: &mut egui::Ui) {
        if let Some(refusal) = self.state.guard_refusal.clone() {
            let amber = egui::Color32::from_rgb(210, 190, 120);
            ui.colored_label(
                amber,
                "not copied: the destination is a system path the console silently ignores",
            );
            ui.small(format!("source      {}", refusal.from.display()));
            ui.small(format!("target      {}", refusal.target_path));
            ui.small(format!("issue       {}", refusal.explanation));
            ui.small(format!("remedy      {}", refusal.remedy));
            ui.horizontal(|ui| {
                let suggested = refusal.suggested_path.clone();
                if ui
                    .button(format!("Use '{suggested}' instead"))
                    .on_hover_text(
                        "copy to the canonical homebrew directory scanned by the console",
                    )
                    .clicked()
                    && let Some(target) = self.state.target().cloned()
                {
                    self.state.library_path.clone_from(&refusal.suggested_path);
                    self.state.guard_refusal = None;
                    self.state.begin(Job::Restore(
                        target,
                        refusal.from.clone(),
                        refusal.suggested_path.clone(),
                        false,
                    ));
                }
                if ui
                    .button("copy anyway")
                    .on_hover_text("send it regardless - having read the above")
                    .clicked()
                    && let Some(target) = self.state.target().cloned()
                {
                    self.state.guard_refusal = None;
                    self.state.begin(Job::Restore(
                        target,
                        refusal.from.clone(),
                        refusal.target_path.clone(),
                        true,
                    ));
                }
                if ui.button("leave it").clicked() {
                    self.state.guard_refusal = None;
                }
            });
            ui.separator();
        }

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
            self.state.library_path = path;
            // Listed straight away, so a stale listing never sits under the new path.
            if connected {
                self.browse();
            }
        }
    }

    /// The right half: what is on the target.
    fn there_side(&mut self, ui: &mut egui::Ui, idle: bool, connected: bool) {
        self.there_toolbar(ui, idle, connected);
        // Return navigates, as on the local side.
        if entered(ui, egui::TextEdit::singleline(&mut self.state.library_path)) {
            self.state.listing.chosen.clear();
            self.state.seen.clear();
            self.browse();
        }
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
                        // The row reports which gesture happened, so a folder stays selectable.
                        let known = self.state.names.get(&entry.name);
                        match listing_row(ui, entry, &self.state.listing.chosen, true, known) {
                            // Opens the directory by the target's name for it (`elfldr`), not
                            // the row's description-based name (`elfldr_v0.25.elf`); see
                            // `listing::Side`. The tick still keys off the row name.
                            Some(Hit::Open) => {
                                entered =
                                    Some(entry.there.as_ref().map_or_else(
                                        || entry.name.clone(),
                                        |side| side.name.clone(),
                                    ));
                            }
                            Some(Hit::Tick) => toggled = Some(entry.name.clone()),
                            None => {}
                        }
                    }
                }
            });
        if let Some(name) = toggled {
            self.state.listing.toggle(&name);
        }
        // The selection is cleared on navigation: a tick is a name, and a name means a
        // different file in another folder. That includes the folder the double click ticked.
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
    /// The model drawn plainly; the split panes are two filtered views of it.
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
                            side_cell(ui, entry.here.as_ref());
                            side_cell(ui, entry.there.as_ref());
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
    /// Copied rather than referenced: the list is a listing of one folder.
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
    /// Not on the worker: it starts a program and returns, so it has nothing to wait for.
    fn reveal(&mut self, path: &Path) {
        match pros_core::reveal::folder(path) {
            Ok(()) => self.state.said = path.display().to_string(),
            Err(why) => self.state.trouble = Some(why.to_string()),
        }
    }

    /// Lists the library path.
    fn browse(&mut self) {
        let where_to = self.state.library_path.clone();
        // A path already read this session is not read again; the sections share one listing
        // slot. Cleared whenever a job reports it changed the target (`Disturbs::There`).
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
                // `local_path` (`data_root()/payloads`), the folder the row actions, the toolbar
                // and downloads all use, not the staging directory.
                let path = PathBuf::from(self.state.local_path.trim());
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
            ui.weak(&self.state.local_path)
                .on_hover_text("what `run`, `send` and `delete here` are judged against");
            ui.weak(format!(
                "({} file{})",
                self.state.local.len(),
                if self.state.local.len() == 1 { "" } else { "s" }
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
            ui.text_edit_singleline(&mut self.state.install_dir);
        });
        ui.separator();

        self.payload_rows(ui);
    }

    /// What a toolbar button says, given what is selected.
    ///
    /// Only `download` changes: it says `update` when every selected payload already has an
    /// older copy on this disk.
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
        if self.state.local.is_empty() {
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
        let chosen = self.state.listing.chosen.clone();
        let on_target = self.state.payloads_there.clone().unwrap_or_default();
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
            self.state.listing.toggle(&name);
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
    /// Registering is in the menu and at the bottom of the target list, not a form here.
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
                // At the bottom of the list, where a missing target is noticed.
                ui.separator();
                if ui.button("register...").clicked() {
                    self.state.editing = None;
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

    /// What the target can do now.
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
            // Beside the check, because a working chain is the answer to what it finds.
            // Single-entry edits stay on the autoload screen.
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

        // Before the services table: whether the target survives its next restart matters
        // more than what answers now. Drawn before the report exists too, because nothing
        // measured is itself a finding.
        let idle = self.state.is_idle();
        let connected = self.state.target().is_some();
        // The configurator goes above the findings, since it answers most of them.
        self.configurator(ui, idle);
        self.doctor_panel(ui, idle);
        self.plan_panel(ui, idle, connected);
        ui.add_space(8.0);
        ui.separator();

        let Some(report) = &self.state.report else {
            ui.label("not asked yet - a target's capabilities are not remembered between");
            ui.label("runs, because the entry point does not survive a power cycle");
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
                // What the service provides, since a port number is not a capability.
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
        ui.colored_label(colour, verdict.to_string());
    }

    /// Everything the doctor is allowed to look at, borrowed from what is already known.
    ///
    /// One place builds it, so a chosen plan is built against the same picture that offered
    /// the options.
    fn with_known<T>(&self, act: impl FnOnce(&pros_core::doctor::Known<'_>) -> T) -> T {
        // The check screen audits the manager's own list, which the check reads beside its
        // probe.
        self.with_known_of(
            self.state.chain.as_ref(),
            pros_core::recovery::Kind::Manager,
            Some(pros_core::chain::PATH),
            act,
        )
    }

    /// Whether the loader is answering, as far as the last check knows.
    ///
    /// `None` means not asked, which the audit treats as not answering: listing the loader is
    /// offered unless a copy is known to hold the port.
    fn loader_is_up(&self) -> Option<bool> {
        let report = self.state.report.as_ref()?;
        let loader = report.about(pros_link::service::LOADER.name.as_ref())?;
        Some(loader.reachability.open)
    }

    /// Which chain this target is meant to be running.
    ///
    /// The registration's answer when it has one (a target set up with etaHEN is not missing
    /// an FTP server); otherwise the first shipped chain. Decided here only, not defaulted in
    /// several places.
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
        list: Option<&str>,
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
            // Passed as it is: not listed is not the same as none.
            there: self.state.payloads_there.as_deref(),
            staged: &staged,
            described,
            chain,
            kind,
            // An autoloader resolves entries against its own list's directory, so a plan has
            // to know which list it is for.
            list,
            preset: &preset,
            known: &self.catalogue,
        })
    }

    /// Every check, worst first, each with the one action that answers it.
    ///
    /// Each action is a whole plan (fetch, send, list), not one step of it.
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
            // Fix all: one combined plan, through the same confirmation as a single fix.
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

        // After the grid: both of these borrow what it was drawn from.
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
    /// A plan reaches the job queue only through this button, after every step is drawn out,
    /// including the steps already done.
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
    /// List edits are applied but not written: the startup list screen is the one place a list
    /// is written, with the whole file shown first. The transfers put the files the edits name
    /// in place.
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
                // `Job::Install`, never `Job::Send`: `Send` runs the ELF in memory and writes
                // nothing to the disk.
                Step::Send { payload, to } => match self.described_as(payload) {
                    Some(described) => match pros_core::staging::path_for(&described) {
                        Some(path) => {
                            self.state.queue(Job::Install(
                                target.clone(),
                                Box::new(described),
                                path,
                                (*to).clone(),
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
                // Held back with the other list work: its entries name files the sends put in
                // place.
                Step::Rebuild { into, entries } => {
                    // Each list once, though several findings can name the same file.
                    if !self.state.rebuild.iter().any(|(kept, _)| kept == into) {
                        self.state.rebuild.push((into.clone(), entries.clone()));
                        edited += 1;
                    }
                }
                // Held back until the files are where the list will say they are
                // (`State::after_transfers`).
                Step::List(fix) => {
                    self.state.after_transfers.push(fix.clone());
                    edited += 1;
                }
                // Turns autoload on so the deployed list is read. Queued like a transfer: it
                // writes the settings file, not the list, and is a no-op when already on.
                Step::Enable { into: _ } => {
                    self.state.queue(Job::EnableAutoload(target.clone()));
                    queued += 1;
                }
                // Puts a file the chain carries back, verbatim. Queued like a transfer: it
                // writes beside the list, not the list.
                Step::Place { into, content } => {
                    self.state.queue(Job::PlaceFile(
                        target.clone(),
                        into.clone(),
                        content.clone(),
                    ));
                    queued += 1;
                }
            }
        }

        self.state.pending_plan = None;
        // With deferred list edits, the payloads are re-listed first: adding an entry needs
        // the payload on internal storage, judged by a listing taken after the sends.
        if queued > 0 && !self.state.after_transfers.is_empty() {
            self.state
                .queue(Job::FindPayloads(target.clone(), PAYLOADS.to_owned()));
        }
        if queued > 0 && self.state.after_transfers.is_empty() {
            // Re-checked only when no list edit is pending: until the list is saved the
            // finding would still fail.
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
    /// Only when the queue is clear and nothing failed: an entry for a file that never arrived
    /// fails at every boot.
    fn finish_deferred_edits(&mut self) {
        if self.state.after_transfers.is_empty() && self.state.rebuild.is_empty() {
            return;
        }
        if !self.state.is_idle() || self.state.queued() > 0 {
            return;
        }
        let waiting = std::mem::take(&mut self.state.after_transfers);
        if let Some(why) = self.state.trouble.clone() {
            let dropped = waiting.len() + self.state.rebuild.len();
            self.state.rebuild.clear();
            self.state.trouble = Some(format!(
                "{why}\nso the startup list was left alone - {dropped} edits were not made"
            ));
            return;
        }
        // One file at a time, in plan order: the review panel shows one whole file, so the
        // next waits until this one is saved.
        if !self.state.rebuild.is_empty() {
            let (into, entries) = self.state.rebuild.remove(0);
            let left = self.state.rebuild.len();
            self.prepare_rebuild(&into, &entries);
            if left > 0 {
                self.state.said = format!(
                    "{} - {left} more list{} to review after this one is saved",
                    self.state.said,
                    if left == 1 { "" } else { "s" }
                );
            }
            return;
        }
        self.apply_fixes(&waiting);
    }

    /// Puts a whole new startup list up for the same review every write goes through.
    ///
    /// Prepared, never written: the review panel writes it once it has been read.
    fn prepare_rebuild(&mut self, into: &str, entries: &[String]) {
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
        // The screen follows the list being written, so the file reviewed is the file saved.
        if let Some(at) = self.state.lists.iter().position(|one| one.path == into) {
            self.state.list_at = at;
        }
        self.state.boot = Some(boot);
        self.state.boot_at = None;
        self.state.pending_change = Some(pros_core::autoload::Change {
            // What it replaces, so the review draws a diff showing which lines go.
            was: self
                .state
                .boot
                .as_ref()
                .map(pros_core::boot::Boot::to_text)
                .unwrap_or_default(),
            now: now.clone(),
            what: format!("set {into} up from nothing - {} entries", entries.len()),
            into: into.to_owned(),
        });
        self.state.section = Section::Autoload;
        self.state.said = format!(
            "{} entries ready for {into} - read the whole file below, then save it",
            entries.len()
        );
    }

    /// Records a description that now points at a newer release.
    ///
    /// The payload list is a user-owned file, so the status line names the old and new
    /// versions.
    fn take_relisted(
        &mut self,
        now: pros_core::manifest::Payload,
        found: pros_core::sources::Upstream,
    ) {
        // The fresh answer replaces the sweep's, so the column stops offering this update.
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
            // Right in memory and wrong on disk: the next launch would revert it silently.
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
    /// A plan ends by checking the target again, and this reads that answer: jobs succeeding
    /// does not mean the finding is answered.
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
    /// It edits and opens the autoload screen rather than writing, so the save goes through
    /// the whole-file review.
    ///
    /// The audit names services (`shsrv`) and a startup list names files (`shsrv_v0.20.elf`);
    /// the target's payload scan joins them.
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
                    // Found the way the check finds it, so what is removed is what was reported.
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
                    // Internal storage only: the manager lists a payload on removable storage
                    // outside its folder but can never resolve it.
                    match there
                        .iter()
                        .filter(named)
                        .find(|one| one.storage == pros_core::payloads::Where::Internal)
                    {
                        Some(one) if boot.add(&one.name) => done += 1,
                        Some(_) => {}
                        None => {
                            // Where else it is, if anywhere, so the message gives a next step.
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

        // The pending change is what brings up the review and the save button.
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

    /// Clears everything known about the previous target, so no answer is shown under
    /// another target's name.
    fn forget_the_last_target(&mut self) {
        self.state.report = None;
        self.state.chain = None;
        self.state.located = None;
        self.state.system = None;
        self.state.settings = None;
        self.state.boot = None;
        self.state.payloads_there = None;
        self.state.names.clear();
        // Another machine has other titles installed.
        self.state.probing.titles = None;
        self.state.probing.id = None;
    }

    /// Asks the target everything the window will need, as soon as one is selected.
    ///
    /// Four reads, in the order their answers are needed: the check, which qualifies every
    /// panel's advice; the payloads the target holds, so no panel recommends fetching one that
    /// is there; the startup list and settings; and the system report. The first starts now
    /// and the rest queue behind it; a failure stops the rest, so the order is by importance.
    fn survey_on_arrival(&mut self) {
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        // A different target clears what is on screen; a re-survey of the same one does not.
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
        // Last, because no other panel depends on it.
        self.state.queue(Job::ReadSystem(target));
    }

    /// Reads the system report when the system screen is opened and none is held.
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
        // Not before the check, which says whether the shell answers at all.
        if self.state.report.is_none() {
            return;
        }
        self.state.begin(Job::ReadSystem(target));
    }

    /// Reads the manager's settings when the autoload screen is opened, then the payload scan,
    /// which the list needs to mark missing entries.
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
    /// Tried once per target; a failure leaves the button rather than retrying every frame.
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
            // A log service that is not loaded is a normal state the check already reports.
            Err(why) => self.state.trouble = Some(why),
        }
    }

    /// The probe's steps and lines since the last frame, the way the log's are taken.
    ///
    /// Polled while it runs, not only when a line arrives, so a silent title's end is drawn.
    fn take_probe(&mut self, ctx: &egui::Context) {
        let Some(run) = &mut self.probe else {
            return;
        };
        if run.drain(
            &mut self.state.probing.lines,
            &mut self.state.probing.status,
        ) {
            ctx.request_repaint();
        }
        if run.is_running() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        // A probe belongs to the target it launched on, like the log.
        if self.state.target().is_none_or(|now| now.name != run.target) {
            self.probe = None;
        }
    }

    /// Lists what is installed when the probe screen is opened, so it can offer titles.
    ///
    /// Asked once per target, like the log; the refresh button asks again.
    fn titles_on_arrival(&mut self) {
        if self.state.section != Section::Probe
            || self.state.probing.titles.is_some()
            || !self.state.is_idle()
        {
            return;
        }
        let Some(target) = self.state.target().cloned() else {
            return;
        };
        if self.state.probing.titles_for.as_deref() == Some(target.name.as_str()) {
            return;
        }
        self.state.probing.titles_for = Some(target.name.clone());
        self.state.begin(Job::Titles(target));
    }

    /// Asks the target which of this section's candidate directories it has.
    ///
    /// Only for sections with more than one candidate and no standard among them. Asked once
    /// per section per target.
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
        // Not before the check, which says whether the file service is up.
        if self.state.report.is_none() {
            return;
        }
        self.state.begin(Job::Locate(target, candidates));
    }

    /// Whether a service this section needs is answering, explaining in the panel if not.
    ///
    /// Answers `true` when the work can go ahead. Otherwise it draws which service, what it
    /// provides, whether it is staged here and whether a reboot brings it back, and offers the
    /// action that would change it.
    fn needs(&mut self, ui: &mut egui::Ui, service: &str) -> bool {
        let Some(report) = &self.state.report else {
            self.unasked(ui, service);
            return false;
        };
        let Some(finding) = report.about(service) else {
            // An unknown service is not reported on.
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
        // The check starts on its own when a target is selected, so it is usually running.
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("asking the target about {service}"));
        });
        ui.small("what a target can do is not remembered between runs, because the entry point");
        ui.small("does not survive a power cycle - so it is asked every time");
    }

    /// Watching the target, over our own stream.
    ///
    /// Connect, watch what goes past, and drive it. Nothing is disabled pending a payload:
    /// with none running, watch reports the refused connection and names the port.
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
        let counts = self.state.watching.counts();
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
            // The failure stays in the feed's status, drawn beside the input line.
            let _ = self.state.feed.open(&link.address, port);
        }
    }

    /// The target's log, virtualized so the buffer can be large without the view paying for it.
    ///
    /// [`egui::ScrollArea::show_rows`] lays out only the rows on screen, so a full buffer costs
    /// what a screenful costs; one text box over every line would be laid out in full every
    /// frame.
    fn log_panel(&mut self, ui: &mut egui::Ui) {
        if self.state.target().is_none() {
            section_heading(ui, Section::Log);
            ui.label("no target selected");
            return;
        }

        let following = self.tail.is_some();
        // Built once for both the toolbar and the rows.
        let matcher = LogMatch::build(&self.state.log_filter, self.state.log_regex);
        self.log_toolbar(ui, following, &matcher);

        if self.state.lines.is_empty() {
            ui.weak(if following {
                "nothing yet - a quiet log is a fact about the target, not a fault"
            } else {
                "not following"
            });
            return;
        }
        filtered_rows(ui, "log", &self.state.lines, &matcher, following);
    }

    /// The log screen controls: following, filtering, copying, and where it is kept.
    #[allow(
        clippy::too_many_lines,
        reason = "one toolbar row, and splitting it would put half the controls in a \
                  different function from the state they all read"
    )]
    fn log_toolbar(&mut self, ui: &mut egui::Ui, following: bool, matcher: &LogMatch) {
        // Re-read rather than passed in, so the closure below does not also borrow `self`.
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
            // Said in words: an ended log and a quiet one look identical otherwise.
            if self.tail.as_ref().is_some_and(crate::tail::Tail::has_ended) {
                ui.colored_label(
                    egui::Color32::from_rgb(220, 120, 120),
                    "the target closed the connection",
                );
            }
            // Where the log is kept, so it can be found afterwards.
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
            // Shared with the probe screen.
            let suggested = known
                .as_ref()
                .map_or_else(|| "log".to_owned(), |target| format!("{}.log", target.name));
            match filter_controls(
                ui,
                &self.state.lines,
                &mut self.state.log_filter,
                &mut self.state.log_regex,
                matcher,
                &suggested,
            ) {
                Some(Ok(said)) => self.state.said = said,
                Some(Err(why)) => self.state.trouble = Some(why),
                None => {}
            }
        });
    }

    /// An installed title, launched with the log already attached, and what it said.
    ///
    /// The `pros probe` loop without the deploy: close what the title left running, attach to
    /// the log, launch, and follow until it parks, exits or the cap passes (`pros_core::probe`).
    fn probe_panel(&mut self, ui: &mut egui::Ui) {
        let Some(target) = self.state.target().cloned() else {
            section_heading(ui, Section::Probe);
            ui.label("no target selected");
            return;
        };
        let running = self
            .probe
            .as_ref()
            .is_some_and(crate::probe::Run::is_running);
        self.probe_toolbar(ui, &target, running);
        self.probe_capture(ui, &target, running);
    }

    /// The probe screen's heading: which title, for how long, and the button that starts it.
    fn probe_toolbar(&mut self, ui: &mut egui::Ui, target: &target::Target, running: bool) {
        let titles = self.state.probing.titles.clone().unwrap_or_default();
        section_heading_with(ui, Section::Probe, |ui| {
            let label = |about: &pros_core::titles::Metadata| match &about.name {
                Some(name) => format!("{}  {name}", about.id),
                None => about.id.clone(),
            };
            let chosen = self
                .state
                .probing
                .id
                .as_ref()
                .and_then(|id| titles.iter().find(|about| &about.id == id))
                .map_or_else(|| "choose a title".to_owned(), label);
            ui.add_enabled_ui(!running, |ui| {
                egui::ComboBox::from_id_salt("probe-title")
                    .selected_text(chosen)
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for about in &titles {
                            ui.selectable_value(
                                &mut self.state.probing.id,
                                Some(about.id.clone()),
                                label(about),
                            );
                        }
                    });
            });
            if ui
                .add_enabled(
                    self.state.is_idle() && !running,
                    egui::Button::new("refresh"),
                )
                .on_hover_text("ask the target what is installed again")
                .clicked()
            {
                self.state.probing.titles_for = Some(target.name.clone());
                self.state.begin(Job::Titles(target.clone()));
            }
            ui.label("for");
            ui.add_enabled(
                !running,
                egui::DragValue::new(&mut self.state.probing.seconds)
                    .range(5..=3600)
                    .suffix("s"),
            )
            .on_hover_text(
                "the longest it follows the log - it stops sooner if the title parks or exits",
            );
            if running {
                if ui
                    .button("stop")
                    .on_hover_text(
                        "stop following - the title is left running; the dashboard Close, or \
                         the system screen, ends it",
                    )
                    .clicked()
                    && let Some(run) = &self.probe
                {
                    run.stop();
                }
                ui.spinner();
            } else if ui
                .add_enabled(
                    self.state.probing.id.is_some(),
                    egui::Button::new("launch and capture"),
                )
                .on_hover_text(
                    "close it if it is running, attach to the log, launch it, and keep what it \
                     says until it parks, exits, or the time is up",
                )
                .on_disabled_hover_text("choose a title first")
                .clicked()
                && let Some(id) = self.state.probing.id.clone()
            {
                self.state.probing.lines.clear();
                self.state.probing.status = format!("starting {id}");
                self.probe = Some(crate::probe::Run::start(
                    target,
                    &id,
                    self.state.probing.seconds,
                ));
            }
        });
    }

    /// What the last probe captured, drawn the way the log screen draws its lines.
    fn probe_capture(&mut self, ui: &mut egui::Ui, target: &target::Target, running: bool) {
        let matcher = LogMatch::build(&self.state.probing.filter, self.state.probing.regex);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !running && !self.state.probing.lines.is_empty(),
                    egui::Button::new("clear"),
                )
                .on_hover_text("forget the last capture")
                .clicked()
            {
                self.state.probing.lines.clear();
                self.state.probing.status.clear();
            }
            let suggested = self.probe.as_ref().map_or_else(
                || "probe.log".to_owned(),
                |run| format!("{}-{}.log", target.name, run.id),
            );
            match filter_controls(
                ui,
                &self.state.probing.lines,
                &mut self.state.probing.filter,
                &mut self.state.probing.regex,
                &matcher,
                &suggested,
            ) {
                Some(Ok(said)) => self.state.said = said,
                Some(Err(why)) => self.state.trouble = Some(why),
                None => {}
            }
        });
        // Progress or ending in words: a stopped capture and a waiting one look the same.
        if !self.state.probing.status.is_empty() {
            ui.label(&self.state.probing.status);
        }
        ui.separator();

        if self
            .state
            .probing
            .titles
            .as_ref()
            .is_some_and(Vec::is_empty)
        {
            ui.weak(format!(
                "nothing under {} looks like an installed title",
                pros_core::titles::APPMETA
            ));
            return;
        }
        if self.state.probing.lines.is_empty() {
            ui.weak(if running {
                "waiting for the title to say something"
            } else {
                "choose a title and launch it - its log is captured here, from before it starts"
            });
            return;
        }
        filtered_rows(ui, "probe", &self.state.probing.lines, &matcher, running);
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
    /// A sentence instead of the panel with greyed controls over empty rows. A re-read of a
    /// screen that already has content never gets here
    /// ([`crate::state::State::still_arriving`]).
    fn arriving(&mut self, ui: &mut egui::Ui) {
        section_heading(ui, self.state.section);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.spinner();
            // Which question, not just loading: answers arrive in sequence.
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
        // First gate: a section names the service it needs, and the last check says whether
        // it answers. Nothing here probes for itself.
        if let Some(needed) = self.state.section.requires()
            && !self.needs(ui, needed)
        {
            return;
        }
        // Second gate: a screen whose first answer is queued but not arrived says so, rather
        // than reading as never asked. One place, so every screen behaves the same.
        if self.state.still_arriving(self.state.section) {
            self.arriving(ui);
            return;
        }
        // A screen with content stays up while it is re-read, with a note so the old reading
        // is not taken for the new one.
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
                // Settled like every two-sided section, so the target pane starts at the
                // payload directory.
                self.settle(Section::Payloads);
                self.payloads_body(ui);
            }
            Section::Log => self.log_panel(ui),
            Section::Probe => self.probe_panel(ui),
            Section::Shell => self.shell_panel(ui),
            // One view; these sections differ only in where each side starts.
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
    /// Waiting is shown with its own clock, so a working window does not look hung.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        // The activity record opens above the status line rather than replacing it.
        if self.state.journal.open {
            self.activity(ui);
            ui.separator();
        }
        ui.horizontal(|ui| {
            let troubles = self.state.journal.troubles();
            let count = self.state.journal.all().len();
            let arrow = if self.state.journal.open { "v" } else { ">" };
            // Both counts on the closed bar.
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
                // Stops a long copy after the file in flight, keeping the account of what
                // was copied.
                if ui
                    .small_button("stop")
                    .on_hover_text("finish the file in flight, then stop and say what was left")
                    .clicked()
                {
                    self.worker.stop();
                }
                // The queue is shown, so every queued job is visible and clearable.
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
                // What is going across right now, when there is one.
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
    /// The record keeps them in order; this shows them reversed, because the last one is
    /// usually what the panel is opened for.
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
        // Somewhere the target told us to go, once it had been asked.
        if let Some(path) = self.state.go_to.take() {
            self.state.library_path = path;
            self.browse();
        }
        if let Some((payload, found)) = self.state.relisted.take() {
            self.take_relisted(payload, found);
        }
        // Once, on the first frame rather than in `new`, so the window opens before the slow
        // sweep starts.
        if !self.asked_at_launch {
            self.asked_at_launch = true;
            self.check_sources(false);
        }
        // Answers from the projects, as they come. This must follow the start above, so the
        // frame that starts a sweep also requests the repaint that drains it.
        if self.sweep.is_some() {
            self.take_sweep_answers();
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        // Lines since the last frame; a repaint only when something came.
        if let Some(tail) = &mut self.tail {
            if tail.drain(&mut self.state.lines) {
                ctx.request_repaint();
            }
            // A tail belongs to the target it was opened against, and closes when that changes.
            if self
                .state
                .target()
                .is_none_or(|now| now.name != tail.target)
            {
                self.tail = None;
            }
        }
        self.take_probe(ctx);
        // Whatever the last job reports it disturbed is read again.
        for what in std::mem::take(&mut self.state.disturbed) {
            match what {
                crate::state::Disturbs::Here => self.read_local(),
                crate::state::Disturbs::There => {
                    // Everything cached about the target is a claim from before this job.
                    self.state.seen.clear();
                    self.browse();
                }
                // Re-surveyed, with the panel left on screen and marked as being re-read.
                crate::state::Disturbs::Report | crate::state::Disturbs::Autoload => {
                    self.state.resurvey = true;
                }
            }
        }
        // A plan's list edits wait for its transfers (`finish_deferred_edits`).
        self.finish_deferred_edits();
        // Keep repainting while something runs, so the clock in the status bar advances.
        if self.state.waiting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // And while a stream runs, because its counters update on another thread.
        if self.state.watching.counts().status.is_watching() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        // Input runs from here, not from the panel that draws it (`drive_pads`).
        self.drive_pads(ctx);
        // A held key produces no events, so the window repaints to keep sending it.
        if self.state.pads.filled() > 0 {
            ctx.request_repaint();
        }

        self.survey_on_arrival();
        self.locate_on_arrival();
        self.system_on_arrival();
        self.autoload_on_arrival();
        self.follow_on_arrival();
        self.titles_on_arrival();
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
            // Not gated on a target: most sections are about this machine too. Wrapped in a
            // scroll area only where the section does not scroll itself, since a nested one
            // gets unlimited height and never scrolls.
            if self.state.section.scrolls_itself() {
                self.section(ui);
            } else {
                // Both directions, so a table wider than the window gets a bar.
                egui::ScrollArea::both()
                    .id_salt(self.state.section.name())
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.section(ui));
            }
        });

        // Unconditionally, after everything that could have begun a job this frame.
        if let Some(job) = self.state.pending.take() {
            self.worker.start(job);
            ctx.request_repaint();
        }
    }
}

/// A byte count somebody can read at a glance.
///
/// Powers of two with one decimal place. Integer arithmetic throughout, because a size can
/// exceed what a float represents exactly.
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

#[cfg(test)]
mod row_tests {
    use super::{Hit, hit_of};

    /// A double click, which egui also reports as a click, opens and does not tick.
    #[test]
    fn a_double_click_opens_even_though_it_is_also_a_click() {
        assert_eq!(hit_of(true, true), Some(Hit::Open));
    }

    /// A single click selects.
    #[test]
    fn a_single_click_selects() {
        assert_eq!(hit_of(false, true), Some(Hit::Tick));
    }

    /// No click reports no hit.
    #[test]
    fn no_click_is_no_hit() {
        assert_eq!(hit_of(false, false), None);
    }
}

/// The pages this build ships, and their order in the reader.
///
/// `include_str!` puts them in the binary, so they match the build. Only the user manual is
/// listed; development records stay in the repository.
const DOCS: &[oops_docs::Doc] = &[
    oops_docs::Doc::new(
        "user-guide",
        "User Guide",
        "Paths, portable mode, network daemons, and local-first storage",
        include_str!("../../docs/features/user-guide.md"),
    ),
    oops_docs::Doc::new(
        "targets",
        "Targets",
        "Registering a console, and asking what it can currently do",
        include_str!("../../docs/features/targets.md"),
    ),
    oops_docs::Doc::new(
        "logs",
        "Kernel Logs",
        "Streaming live system and title telemetry unbuffered",
        include_str!("../../docs/features/logs.md"),
    ),
    oops_docs::Doc::new(
        "files",
        "Remote Storage",
        "Browsing files, transferring saves, and staging titles",
        include_str!("../../docs/features/files.md"),
    ),
    oops_docs::Doc::new(
        "titles",
        "Titles & Execution",
        "Supervising running processes and launching BIG_APPs",
        include_str!("../../docs/features/titles.md"),
    ),
    oops_docs::Doc::new(
        "shell",
        "Command Shell",
        "Executing remote commands directly on the target",
        include_str!("../../docs/features/shell.md"),
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
    /// No page is empty or headingless, and no two entries share a slug.
    #[test]
    fn the_registry_is_sound() {
        assert_eq!(oops_docs::check(super::DOCS), Vec::<String>::new());
    }
}

#[cfg(test)]
mod glyph_tests {
    /// The source holds no escaped glyph the default font cannot draw (arrows, triangles).
    #[test]
    fn the_window_draws_nothing_a_font_might_not_have() {
        let source = include_str!("app.rs");
        for (number, line) in source.lines().enumerate() {
            // Such glyphs are written in the escape form.
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

    /// The loader's role explains why it loads first (the manager sends every entry to it).
    #[test]
    fn the_loader_says_why_it_comes_first() {
        let said = role_of(&LOADER).expect("the loader has a role");
        assert!(said.contains("loaded through it"), "{said}");
        assert!(said.contains("before them"), "{said}");
    }

    /// Only a service whose flags give it a structural part claims a role.
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

    /// A payload with no role gets no invented one.
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

    /// A service declared in the catalogue gets the same role text as a built-in one.
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
/// Rounded down, in the largest unit that fits.
fn how_long(seconds: u64) -> String {
    match seconds {
        0..=90 => "just now".to_owned(),
        91..=5400 => format!("{}m ago", seconds / 60),
        5401..=172_800 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}
