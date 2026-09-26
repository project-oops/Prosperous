//! One list of things, each of which may be here, on the target, or only described.
//!
//! The model is a single list of entries, each knowing which sides it is on. The split view
//! projects it (left pane: entries with a `here`, right: with a `there`); the merged view
//! shows both columns at once.
//!
//! Actions belong to the selection, not to rows, so the rules live here and are tested without
//! a window. An action that does not apply to everything selected is offered and refused, with
//! the reason naming what is in the way, rather than hidden.

use std::collections::{BTreeMap, BTreeSet};

use pros_core::library::{Item, Kind};
use pros_core::manifest::{Manifest, Payload};

use crate::state::Section;

/// One side's knowledge of a thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Side {
    /// What this side calls it. The sides spell one payload differently (see
    /// [`Listing::build`]), so a path on a side is built from this, not from the row's name.
    pub name: String,
    /// How big, when the listing said.
    pub size: Option<u64>,
    /// Whether it is something to look inside rather than to copy.
    pub folder: bool,
}

/// One thing, and where it is.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    /// What it is called. The key both sides are matched on.
    pub name: String,
    /// On this machine.
    pub here: Option<Side>,
    /// On the target.
    pub there: Option<Side>,
    /// What a tracked list says about it, when it says anything.
    ///
    /// Present also for things on neither side, which is how a list of things worth having
    /// appears.
    pub described: Option<Payload>,
}

/// Which sides an entry is on.
///
/// "Described" is distinct from missing: a payload nobody has fetched is not a deleted file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    /// On both sides.
    Both,
    /// Only on this machine.
    OnlyHere,
    /// Only on the target.
    OnlyThere,
    /// On neither, and described.
    Described,
}

impl Entry {
    /// Which sides this is on.
    pub(crate) const fn standing(&self) -> Standing {
        match (self.here.is_some(), self.there.is_some()) {
            (true, true) => Standing::Both,
            (true, false) => Standing::OnlyHere,
            (false, true) => Standing::OnlyThere,
            (false, false) => Standing::Described,
        }
    }

    /// Whether the copy on the target is a folder. Asked per side: the payload manager keeps
    /// each payload in a directory, while the local copy is a file.
    pub(crate) fn folder_there(&self) -> bool {
        self.there.as_ref().is_some_and(|side| side.folder)
    }

    /// Whether a download of this could be checked when it arrived.
    pub(crate) fn is_fetchable(&self) -> bool {
        self.described
            .as_ref()
            .is_some_and(|payload| payload.url.is_some() && payload.is_verifiable())
    }
}

/// Something that can be done to a selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Load it now, through the loader.
    ///
    /// Distinct from [`Self::Send`]: running puts it in memory until the next power cycle,
    /// sending puts a file on a disk.
    Run,
    /// Copy from here to the target.
    Send,
    /// Copy from the target to here.
    Fetch,
    /// Get it from wherever the list says it is published.
    Download,
    /// Have the target read and register it.
    Install,
    /// Start it on the target.
    ///
    /// [`Self::Run`] sends ELF bytes to a loader; this sends an identifier to the target's own
    /// system service, which boots the installed application. No file crosses the link. See
    /// `pros_core::launch`.
    Launch,
    /// Remove it from this machine.
    ///
    /// One delete per side, so a thing on both sides never leaves the side to a guess.
    DeleteHere,
    /// Remove it from the target.
    DeleteThere,
}

impl Offer {
    /// Every action, in the order a toolbar should show them.
    pub(crate) const ALL: [Self; 8] = [
        Self::Run,
        Self::Send,
        Self::Fetch,
        Self::Download,
        Self::Install,
        Self::Launch,
        Self::DeleteHere,
        Self::DeleteThere,
    ];

    /// Whether this action can ever apply on that screen.
    ///
    /// Elsewhere a control that does not apply is greyed with a reason, because a selection
    /// change could enable it. Where no selection ever could, as `launch` on the payloads
    /// screen, the control is absent instead.
    pub(crate) const fn applies_to(self, section: Section) -> bool {
        match self {
            Self::Launch => matches!(section, Section::Titles | Section::Filesystem),
            Self::Install => matches!(section, Section::Packages | Section::Filesystem),
            _ => true,
        }
    }

    /// Whether this destroys something.
    ///
    /// Such an action is confirmed first and drawn apart from the buttons that move data.
    pub(crate) const fn is_destructive(self) -> bool {
        matches!(self, Self::DeleteHere | Self::DeleteThere)
    }

    /// What the button says.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Send => "send >",
            Self::Fetch => "< fetch",
            Self::Download => "download",
            Self::Install => "install",
            Self::Launch => "launch",
            Self::DeleteHere => "delete here",
            Self::DeleteThere => "delete there",
        }
    }

    /// What it does, for a hover.
    pub(crate) const fn describes(self) -> &'static str {
        match self {
            Self::Run => {
                "start the selected payload. A copy on this machine is sent to the loader and \
                 run from memory; one that is only on the target is started where it already \
                 is, through the shell"
            }
            Self::Send => "copy the selected items onto the target's disk",
            Self::Fetch => "copy the selected items to this machine",
            Self::Download => "fetch the selected items from where the list says they are",
            Self::Install => {
                "hold the package out for the target to fetch, and have it register what it \
                 finds - after this it is an installed title, not a file"
            }
            Self::Launch => {
                "ask the target to start an application it already has installed, by \
                 identifier - this sends no file and runs nothing from this machine"
            }
            Self::DeleteHere => "remove the selected items from this machine - not undoable",
            Self::DeleteThere => "remove the selected items from the target - not undoable",
        }
    }

    /// Why one entry cannot take part.
    ///
    /// `None` when it can. [`Listing::offers`] names only the first entry that cannot.
    fn refuses(self, entry: &Entry) -> Option<String> {
        let name = &entry.name;
        match self {
            Self::Send if entry.here.is_none() => Some(format!("{name} is not on this machine")),
            Self::Fetch if entry.there.is_none() => Some(format!("{name} is not on the target")),
            Self::Download if entry.described.is_none() => {
                Some(format!("nothing describes where to get {name}"))
            }
            Self::Download if !entry.is_fetchable() => Some(format!(
                "{name} has no url, or a digest this cannot check - it will not be fetched"
            )),
            // The target fetches the package from here; measured, a path on its own disk gives
            // the installer nothing it can read.
            Self::Install if entry.here.is_none() => Some(format!(
                "{name} is only on the target, and the target fetches a package from here -                  so it has to be here"
            )),
            Self::Install if !pros_core::install::is_a_package(name) => {
                Some(format!("{name} is not a package"))
            }
            Self::Launch if !pros_core::launch::is_an_app_id(name) => {
                Some(format!("{name} is not an application identifier"))
            }
            Self::Launch if entry.there.is_none() => {
                Some(format!("{name} is not installed on the target"))
            }
            Self::DeleteHere if entry.here.is_none() => {
                Some(format!("{name} is not on this machine"))
            }
            Self::DeleteThere if entry.there.is_none() => {
                Some(format!("{name} is not on the target"))
            }
            _ => None,
        }
    }
}

/// Which row a file belongs in: the described payload it is a copy of, or itself.
///
/// Matched through [`pros_core::chain::Chain::position`], as everywhere else: a name matches
/// when it is the whole entry or is followed by a separator and a version, so `kstuff` does not
/// match `kstuff-lite_v1.09`. Both the described filename and the payload's name are tried; a
/// directory on the target is named after the latter.
fn one_payload(described: &Manifest, name: &str) -> String {
    for payload in described.payloads() {
        let file = payload.filename.as_deref().unwrap_or(&payload.name);
        if is_a_copy_of(name, file) || is_a_copy_of(name, &payload.name) {
            return file.to_owned();
        }
    }
    name.to_owned()
}

/// Whether `name` is that payload, allowing a version on the end.
fn is_a_copy_of(name: &str, payload: &str) -> bool {
    pros_core::chain::Chain::parse(name)
        .position(payload)
        .is_some()
}

/// A list of entries and what is selected in it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Listing {
    /// Everything, in name order.
    pub entries: Vec<Entry>,
    /// What is ticked, by name.
    ///
    /// By name, not index: the list is rebuilt whenever either side changes.
    pub chosen: BTreeSet<String>,
}

impl Listing {
    /// Builds one from a tracked list, a local folder and a target listing.
    ///
    /// One payload is one row whatever each side calls it. Names match case-insensitively. A
    /// described entry is keyed by its filename, and the local and target sides are matched to
    /// it by payload rather than spelling: one measured setup had `elfldr-ps5.elf` in the
    /// description, `elfldr_v0.25.elf` on disk and a directory `elfldr` on the target.
    pub(crate) fn build(described: &Manifest, local: &[Item], remote: &[Item]) -> Self {
        /// The entry for a name, made if this is the first mention of it. A function because a
        /// closure cannot return a reference into the map it borrows.
        fn at<'a>(by_key: &'a mut BTreeMap<String, Entry>, name: &str) -> &'a mut Entry {
            by_key.entry(name.to_lowercase()).or_insert_with(|| Entry {
                name: name.to_owned(),
                here: None,
                there: None,
                described: None,
            })
        }

        /// What a listing entry looks like from one side.
        fn side(item: &Item) -> Side {
            Side {
                name: item.name.clone(),
                size: item.size,
                folder: item.kind == Kind::Folder || item.kind == Kind::Title,
            }
        }

        let mut by_key: BTreeMap<String, Entry> = BTreeMap::new();

        for payload in described.payloads() {
            let file = payload.filename.as_deref().unwrap_or(&payload.name);
            at(&mut by_key, file).described = Some(payload.clone());
        }
        for item in local {
            at(&mut by_key, &one_payload(described, &item.name)).here = Some(side(item));
        }
        for item in remote {
            at(&mut by_key, &one_payload(described, &item.name)).there = Some(side(item));
        }

        let mut entries: Vec<Entry> = by_key.into_values().collect();
        entries.sort_by_key(|entry| entry.name.to_lowercase());
        Self {
            entries,
            chosen: BTreeSet::new(),
        }
    }

    /// Everything ticked.
    pub(crate) fn picked(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| self.chosen.contains(&entry.name))
            .collect()
    }

    /// Ticks or unticks one entry.
    pub(crate) fn toggle(&mut self, name: &str) {
        if !self.chosen.remove(name) {
            self.chosen.insert(name.to_owned());
        }
    }

    /// Drops any tick for something no longer in the list.
    ///
    /// Called after rebuilding, so an action never runs on a stale name.
    pub(crate) fn forget_what_left(&mut self) {
        let present: BTreeSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.chosen.retain(|name| present.contains(name.as_str()));
    }

    /// Whether an action can be taken on what is selected, and why not when it cannot.
    ///
    /// `Ok(())` when every selected entry can take part.
    ///
    /// # Errors
    ///
    /// The reason, naming the first entry in the way; nothing selected has its own wording.
    pub(crate) fn offers(&self, offer: Offer) -> Result<(), String> {
        let picked = self.picked();
        if picked.is_empty() {
            return Err("nothing is selected".to_owned());
        }
        // Run takes one row: its side decides how it runs (loader from here, shell on the
        // target). This is the only condition that greys it.
        if offer == Offer::Run && picked.len() != 1 {
            return Err(format!(
                "{} are selected - run starts one payload, so select one",
                picked.len()
            ));
        }
        for entry in picked {
            if let Some(why) = offer.refuses(entry) {
                return Err(why);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Entry, Listing, Offer, Standing};
    use pros_core::library::{Item, Kind};
    use pros_core::manifest::Manifest;

    fn item(name: &str) -> Item {
        Item {
            name: name.to_owned(),
            id: None,
            kind: Kind::File,
            size: Some(1),
        }
    }

    fn folder(name: &str) -> Item {
        Item {
            name: name.to_owned(),
            id: None,
            kind: Kind::Folder,
            size: None,
        }
    }

    /// One name is one row, whichever sides it is on.
    #[test]
    fn both_sides_of_one_thing_are_one_entry() {
        let listing = Listing::build(
            &Manifest::default(),
            &[item("a.elf"), item("b.elf")],
            &[item("b.elf"), item("c.elf")],
        );

        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.elf", "b.elf", "c.elf"]);
        assert_eq!(listing.entries[0].standing(), Standing::OnlyHere);
        assert_eq!(listing.entries[1].standing(), Standing::Both);
        assert_eq!(listing.entries[2].standing(), Standing::OnlyThere);
    }

    /// Names match case-insensitively.
    #[test]
    fn one_thing_spelled_two_ways_is_still_one_thing() {
        let described = Manifest::from_json(r#"[{ "name": "elfldr", "filename": "ELFLDR.ELF" }]"#)
            .expect("reads");
        let listing = Listing::build(&described, &[item("elfldr.elf")], &[]);
        assert_eq!(listing.entries.len(), 1, "{:?}", listing.entries);
        assert_eq!(listing.entries[0].standing(), Standing::OnlyHere);
        assert!(listing.entries[0].described.is_some());
    }

    /// Described and on neither side is its own standing.
    #[test]
    fn something_described_and_nowhere_is_not_the_same_as_missing() {
        let described = Manifest::from_json(r#"[{ "name": "shsrv", "filename": "shsrv.elf" }]"#)
            .expect("reads");
        let listing = Listing::build(&described, &[], &[]);
        assert_eq!(listing.entries[0].standing(), Standing::Described);
    }

    fn two_sided() -> Listing {
        let described = Manifest::from_json(
            r#"[{ "name": "elfldr", "filename": "elfldr.elf", "url": "https://example.com/e",
                  "checksum": "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824" },
                { "name": "nothing", "filename": "nothing.elf" }]"#,
        )
        .expect("reads");
        Listing::build(
            &described,
            &[item("here.elf")],
            &[item("there.pkg"), folder("games")],
        )
    }

    /// Nothing selected refuses every action, in its own words.
    #[test]
    fn an_empty_selection_refuses_every_action() {
        let listing = two_sided();
        for offer in Offer::ALL {
            let refused = listing.offers(offer).expect_err("nothing is selected");
            assert!(refused.contains("nothing is selected"), "{refused}");
        }
    }

    /// An action applies when every selected thing can take part.
    #[test]
    fn sending_needs_everything_selected_to_be_here() {
        let mut listing = two_sided();
        listing.toggle("here.elf");
        assert!(listing.offers(Offer::Send).is_ok());
        assert!(listing.offers(Offer::Fetch).is_err());

        listing.toggle("there.pkg");
        let refused = listing.offers(Offer::Send).expect_err("mixed selection");
        assert!(
            refused.contains("there.pkg"),
            "the reason should name what is in the way: {refused}"
        );
    }

    /// A refusal names only the first thing in the way.
    #[test]
    fn a_refusal_names_the_first_thing_in_the_way() {
        let mut listing = two_sided();
        listing.toggle("here.elf");
        listing.toggle("games");
        let refused = listing
            .offers(Offer::Fetch)
            .expect_err("here.elf is not there");
        assert!(refused.contains("here.elf"), "{refused}");
        assert!(!refused.contains("games"), "one name is enough: {refused}");
    }

    /// Downloading needs a url and a checkable digest before it is offered.
    #[test]
    fn downloading_needs_somewhere_to_get_it_and_a_way_to_check_it() {
        let mut listing = two_sided();
        listing.toggle("elfldr.elf");
        assert!(listing.offers(Offer::Download).is_ok());

        listing.chosen.clear();
        listing.toggle("nothing.elf");
        let refused = listing.offers(Offer::Download).expect_err("no url");
        assert!(refused.contains("nothing.elf"), "{refused}");
    }

    /// Installing needs the package on this machine: `pkg_install` takes a url, and a package
    /// in the target's `/data/pkg` measured the same empty answer as a missing file.
    #[test]
    fn installing_needs_a_package_on_this_machine_to_hold_out() {
        let nothing = Manifest::default();

        let mut here = Listing::build(&nothing, &[item("thing.pkg")], &[]);
        here.toggle("thing.pkg");
        assert!(
            here.offers(Offer::Install).is_ok(),
            "it is here to hold out"
        );

        let mut there = Listing::build(&nothing, &[], &[item("thing.pkg")]);
        there.toggle("thing.pkg");
        let refused = there
            .offers(Offer::Install)
            .expect_err("it is only on the target");
        assert!(refused.contains("has to be here"), "{refused}");
    }

    /// Installing refuses a file that is not a package.
    #[test]
    fn installing_still_needs_a_package() {
        let mut listing = two_sided();
        listing.toggle("here.elf");
        let refused = listing
            .offers(Offer::Install)
            .expect_err("an elf is not a package");
        assert!(refused.contains("not a package"), "{refused}");
    }

    /// A tick for a row that is gone is dropped after a rebuild.
    #[test]
    fn a_selection_does_not_outlive_the_rows_it_named() {
        let mut listing = two_sided();
        listing.toggle("here.elf");
        listing.toggle("there.pkg");

        listing.entries.retain(|entry| entry.name != "there.pkg");
        listing.forget_what_left();

        assert!(listing.chosen.contains("here.elf"));
        assert!(
            !listing.chosen.contains("there.pkg"),
            "a tick outlived its row"
        );
    }

    /// The three measured spellings of one payload make one row, and `run` is offered on it.
    #[test]
    fn one_payload_is_one_row_however_each_side_spells_it() {
        let described =
            Manifest::from_json(r#"[{ "name": "elfldr", "filename": "elfldr-ps5.elf" }]"#)
                .expect("reads");
        let mut listing =
            Listing::build(&described, &[item("elfldr_v0.25.elf")], &[folder("elfldr")]);
        assert_eq!(listing.entries.len(), 1, "{:?}", listing.entries);
        let only = &listing.entries[0];
        assert_eq!(only.name, "elfldr-ps5.elf", "keyed by what describes it");
        assert!(only.here.is_some(), "the copy on this disk found it");
        assert!(
            only.there.is_some(),
            "and so did the directory on the target"
        );

        // The payload table ticks the row by the description's filename.
        listing.toggle("elfldr-ps5.elf");
        assert!(
            listing.offers(Offer::Run).is_ok(),
            "{:?}",
            listing.offers(Offer::Run)
        );
    }

    /// A name that only starts the same (`kstuff`, `kstuff-lite`) is a different payload.
    #[test]
    fn a_longer_name_is_not_a_version_of_a_shorter_one() {
        let described = Manifest::from_json(r#"[{ "name": "kstuff", "filename": "kstuff.elf" }]"#)
            .expect("reads");
        let listing = Listing::build(&described, &[item("kstuff-lite_v1.10.elf")], &[]);
        assert_eq!(listing.entries.len(), 2, "{:?}", listing.entries);
    }

    /// Being a folder is known per side.
    #[test]
    fn a_folder_is_known_on_the_side_that_has_it() {
        let listing = two_sided();
        let games: &Entry = listing
            .entries
            .iter()
            .find(|entry| entry.name == "games")
            .expect("there");
        assert!(games.folder_there(), "it is a directory on the target");
        assert!(
            games.here.is_none(),
            "and this machine does not have it at all"
        );
    }

    /// A local file whose target side is a directory (`/data/pldmgr/payloads/<name>/`) can run.
    #[test]
    fn a_payload_the_target_keeps_in_a_directory_can_still_be_run() {
        let mut listing = Listing::build(
            &Manifest::from_json("[]").expect("reads"),
            &[item("pldmgr_v0.5.1.elf")],
            &[folder("pldmgr_v0.5.1.elf")],
        );
        listing.toggle("pldmgr_v0.5.1.elf");
        assert!(
            listing.offers(Offer::Run).is_ok(),
            "the local copy is a file, and that is the one that gets sent"
        );
        // A directory on the target is removed by `pros_core::remove`'s guarded walk.
        assert!(
            listing.offers(Offer::DeleteThere).is_ok(),
            "a directory on the target is removed by walking it, not refused"
        );
    }
}
