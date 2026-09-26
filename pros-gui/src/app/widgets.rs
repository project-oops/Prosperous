//! Drawing helpers shared by every panel: rows, headings, panes, the splitter and the
//! file dialogs.

use std::path::PathBuf;

use super::GAP;
use crate::state::Section;

/// One row of a listing: a tick, a name, and what that side knows about it.
///
/// Returns what the row was asked to do, if anything. There are no action buttons: what can be
/// done depends on the whole selection, so it lives in the toolbar.
pub(super) fn listing_row(
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
pub(super) fn up_row(ui: &mut egui::Ui, path: &str) -> bool {
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
pub(super) fn entered(ui: &mut egui::Ui, field: egui::TextEdit<'_>) -> bool {
    let response = ui.add(field.desired_width(f32::INFINITY));
    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
}

/// The directory above this one, when it is not already the root.
pub(super) fn parent_of(path: &str) -> Option<String> {
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
pub(super) enum Hit {
    /// Select it, or stop selecting it.
    Tick,
    /// Look inside it.
    Open,
}

/// What one side knows, for the merged view's columns.
pub(super) fn side_cell(ui: &mut egui::Ui, side: Option<&crate::listing::Side>) {
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
pub(super) fn standing_of(entry: &crate::listing::Entry) -> (&'static str, egui::Color32) {
    match entry.standing() {
        crate::listing::Standing::Both => ("both", egui::Color32::from_rgb(120, 190, 120)),
        crate::listing::Standing::OnlyHere => ("here only", egui::Color32::from_rgb(140, 180, 220)),
        crate::listing::Standing::OnlyThere => {
            ("target only", egui::Color32::from_rgb(210, 190, 120))
        }
        crate::listing::Standing::Described => ("described", egui::Color32::GRAY),
    }
}

/// The top of a section: its name, what it is for, and a rule under both.
///
/// One function for every section, so all headings carry the same explanation.
pub(super) fn section_heading(ui: &mut egui::Ui, section: Section) {
    section_heading_with(ui, section, |_| ());
}

/// The same top, with controls beside the name.
pub(super) fn section_heading_with(
    ui: &mut egui::Ui,
    section: Section,
    controls: impl FnOnce(&mut egui::Ui),
) {
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
pub(super) fn role_of(service: &pros_link::service::Service) -> Option<String> {
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
pub(super) fn reason(ui: &mut egui::Ui, known: Option<String>) {
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
pub(super) fn headings(ui: &mut egui::Ui, titles: &[&str]) {
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
pub(super) fn group_row(
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
pub(super) fn fold(folded: &mut std::collections::BTreeSet<String>, toggled: Option<String>) {
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
pub(super) fn pane(
    ui: &mut egui::Ui,
    salt: &str,
    size: egui::Vec2,
    body: impl FnOnce(&mut egui::Ui),
) {
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
pub(super) fn half_of(ui: &egui::Ui) -> egui::Vec2 {
    egui::vec2(
        ((ui.available_width() - GAP) * 0.5).max(120.0),
        ui.available_height().max(120.0),
    )
}

/// The same, split where somebody dragged it to.
///
/// `share` is the left pane's fraction of the usable width, so the split survives a resize; a
/// split held in pixels creeps towards one edge as the window shrinks.
pub(super) fn split_at(ui: &egui::Ui, share: f32) -> (egui::Vec2, egui::Vec2) {
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
pub(super) fn splitter(ui: &mut egui::Ui, height: f32) -> f32 {
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
pub(super) fn choose_a_file(what: &str, kinds: &[&str], from: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(what)
        .add_filter(what, kinds)
        .set_directory(from.trim())
        .pick_file()
}

/// Asks for any number of files, of any kind.
pub(super) fn choose_files(from: &str) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_title("files to copy in")
        .set_directory(from.trim())
        .pick_files()
}

/// Asks where to save a file, suggesting a name.
///
/// `None` when the dialog was dismissed without choosing.
pub(super) fn choose_where_to_save(suggested: &str) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("save the log")
        .add_filter("log", &["log"])
        .set_file_name(suggested)
        .save_file()
}

/// A byte count somebody can read at a glance.
///
/// Powers of two with one decimal place. Integer arithmetic throughout, because a size can
/// exceed what a float represents exactly.
pub(super) fn size(bytes: u64) -> String {
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
pub(super) fn how_long(seconds: u64) -> String {
    match seconds {
        0..=90 => "just now".to_owned(),
        91..=5400 => format!("{}m ago", seconds / 60),
        5401..=172_800 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}
