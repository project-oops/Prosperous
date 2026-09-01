//! One list of things, each of which may be here, on the target, or only described.
//!
//! # Why the merged view is the real model
//!
//! Every sync section shows two panes and asks one question: **what is on each side, and what
//! do I want to do about the difference?** Two lists drawn separately answer that badly - a
//! name in the left pane and the same name in the right are one thing, and the eye has to do
//! the joining.
//!
//! So the model is a single list of entries, each knowing which sides it is on. The split view
//! is a *projection* of that: the left pane is the entries with a `here`, the right the ones
//! with a `there`. The merged view is the same data with both columns shown at once. Neither
//! is the source of truth for the other, because they are the same thing.
//!
//! That is also why this module exists rather than the drawing code doing it twice. It was
//! twice: the local pane merged a tracked list against a folder, the target pane listed a
//! directory, and neither knew about the other.
//!
//! # Why actions belong here and not on rows
//!
//! A button on every row is a button repeated fifty times, and it decides for one thing what
//! the person may want for twenty. **What can be done depends on what is selected**, so it is
//! a property of the selection - which lives here, with the rules about it, where it can be
//! checked without a window.
//!
//! An action that does not apply to everything selected is **offered and refused, with the
//! reason naming what is in the way**. Hiding it would leave somebody looking for a control
//! that is not there, unable to tell whether they are wrong about the tool or the tool is
//! wrong about them.

use std::collections::{BTreeMap, BTreeSet};

use pros_core::library::{Item, Kind};
use pros_core::manifest::{Manifest, Payload};

use crate::state::Section;

/// One side's knowledge of a thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Side {
    /// What **this** side calls it.
    ///
    /// # Why the row's name is not enough
    ///
    /// A row is one payload and the sides disagree about its spelling: a description saying
    /// `elfldr-ps5.elf`, a disk holding `elfldr_v0.25.elf`, a target keeping a directory called
    /// `elfldr`. The row is named after the description, so a path built from the row's name
    /// asks a side for a file it does not have under that name.
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
    /// **Present for things that are on neither side**, which is how a list of things worth
    /// having appears at all.
    pub described: Option<Payload>,
}

/// Which sides an entry is on.
///
/// **Four states, not two.** A thing on both sides, a thing on one side or the other, and a
/// thing on neither that somebody has written down as worth having. Collapsing the last into
/// *missing* would put a payload nobody has fetched in the same box as a file that was deleted.
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

    /// Whether the copy on this machine is a folder.
    ///
    /// # Why each side is asked separately
    ///
    /// There was one answer for both, true when *either* side was a directory, and every caller
    /// used it for a decision about one particular side. Two of them used it for the wrong one:
    /// running and sending read the file on this machine and refused when the **target** held a
    /// directory of that name - which is how the payload manager stores every payload it has,
    /// so `run` was refused for all of them on the strength of a side it never reads.
    /// Whether the copy on the target is a folder.
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
    /// **Distinct from copying it there.** Running a payload puts it in memory until the next
    /// power cycle; copying puts a file on a disk. Both are useful and they are not the same
    /// act, which is why they are not the same button.
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
    /// **Not the same as running a payload, and not the same machinery.** [`Self::Run`] sends
    /// ELF bytes to a loader that spawns them. This sends an identifier to the target's own
    /// system service, which finds the installed application and boots its own executable -
    /// no file crosses the link and nothing here is executed.
    ///
    /// See `pros_core::launch` for the call it is, and `docs/DECISIONS.md` for the three ways
    /// something can end up running on a target and why they are three buttons.
    Launch,
    /// Remove it from this machine.
    ///
    /// **Two delete actions, not one.** A thing on both sides would otherwise leave the button
    /// to guess which side was meant, and the guess would sometimes be the one that could not
    /// be undone.
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
    /// # Why some controls are absent rather than greyed
    ///
    /// **The rule everywhere else here is to disable and say why**, because a control that
    /// vanishes reads as a bug while a greyed one reads as a state. That rule assumes the
    /// control could become live - it is telling somebody what to change.
    ///
    /// These cannot. `launch` sends an application identifier to the target's own system
    /// service; a payload is not an application and never will be, so on the payloads screen
    /// the button was permanently grey, explaining a state that no action could leave. That is
    /// not a state, it is furniture - and it sat next to `run`, which is the control somebody
    /// actually wants, inviting exactly the wrong guess about which one starts a payload.
    pub(crate) const fn applies_to(self, section: Section) -> bool {
        match self {
            // Installed applications, by identifier. Only the screens that list them.
            Self::Launch => matches!(section, Section::Titles | Section::Filesystem),
            // Registering a package with the target, which is what the packages screen is.
            Self::Install => matches!(section, Section::Packages | Section::Filesystem),
            _ => true,
        }
    }

    /// Whether this destroys something.
    ///
    /// Used to put a confirm in front of it, and to draw it apart from the rest - a button
    /// that loses data should not sit in the run of buttons that move it.
    pub(crate) const fn is_destructive(self) -> bool {
        matches!(self, Self::DeleteHere | Self::DeleteThere)
    }

    /// What the button says.
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Send => "send >",
            Self::Fetch => "< fetch",
            // The one label that is not fixed: replacing an older copy that is already here is
            // a different act from getting a file for the first time. See `Offer::says`.
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
    /// `None` when it can. **The first entry that cannot is the one named**, because a reason
    /// mentioning fifty things is one nobody reads.
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
            // **On this machine, not on the target.** Installing means holding the file out
            // for the target to fetch, so the file has to be here to hold out. A package
            // already on its disk cannot be installed from there - measured: a local path
            // gives the target nothing it can read.
            Self::Install if entry.here.is_none() => Some(format!(
                "{name} is only on the target, and the target fetches a package from here -                  so it has to be here"
            )),
            Self::Install if !pros_core::install::is_a_package(name) => {
                Some(format!("{name} is not a package"))
            }
            // **One at a time.** A target shows one thing at once, and asking it to start
            // three is two requests that go nowhere and one that might.
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
            // A directory has to be empty for the server to remove it, and emptying one is a
            // walk that deletes things nobody listed. Refused here so the reason arrives
            // before the press rather than as a server's refusal after it.
            Self::DeleteThere if entry.folder_there() => Some(format!(
                "{name} is a folder - empty it first, a file at a time"
            )),
            _ => None,
        }
    }
}

/// Which row a file belongs in: the described payload it is a copy of, or itself.
///
/// **Matched the way every other comparison in this program matches a payload**, through
/// [`pros_core::chain::Chain::position`]: a name matches when it is the whole entry, or when
/// what follows it is a separator and then a version. That rule is not a convenience here - it
/// is the one that already knows `kstuff` must not swallow `kstuff-lite_v1.09`, which is two
/// payloads reported as one in the column that says what comes back after a reboot.
///
/// Both spellings of the description are tried, because either can be the one a side used: the
/// filename it names, and the payload's own name, which is what a directory on the target is
/// called.
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
    /// **By name rather than by index**, because the list is rebuilt from the two sides every
    /// time either changes, and an index would silently come to mean a different row.
    pub chosen: BTreeSet<String>,
}

impl Listing {
    /// Builds one from a tracked list, a local folder and a target listing.
    ///
    /// # One payload is one row, whatever each side calls it
    ///
    /// Matching is **by name, case-insensitively**: a list writing `ELFLDR.ELF` and a disk
    /// holding `elfldr.elf` are one thing, and showing them as two invites somebody to fetch
    /// what they already have.
    ///
    /// A described entry is keyed by its filename when it has one, because that is what both
    /// sides would call it - its display name is often something else entirely. **The other two
    /// sides are then matched to it by payload rather than by spelling**, because the three of
    /// them routinely disagree:
    ///
    /// | | elfldr, on one real machine |
    /// |---|---|
    /// | the description | `elfldr-ps5.elf` |
    /// | this disk | `elfldr_v0.25.elf` |
    /// | the target | `elfldr`, a directory the manager keeps it in |
    ///
    /// Keyed by exact name that is **three rows for one payload**, and the row a person ticks
    /// decides which of the three the toolbar acts on. Ticking the described one offered no
    /// `run`, because the file on this disk was a different row - and said so, in a message
    /// naming a file that was sitting in the folder it had just been told to look in.
    pub(crate) fn build(described: &Manifest, local: &[Item], remote: &[Item]) -> Self {
        /// The entry for a name, made if this is the first mention of it.
        ///
        /// A function rather than a closure: one that borrowed the map and handed back a
        /// reference into it cannot be a closure at all, and writing it as one only produces
        /// a borrow error with the reason hidden in it.
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
    /// **Called after rebuilding**, so a selection cannot name rows that are gone - an action
    /// on a stale name either does nothing or does it to the wrong thing, and both are silent.
    pub(crate) fn forget_what_left(&mut self) {
        let present: BTreeSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.chosen.retain(|name| present.contains(name.as_str()));
    }

    /// Whether an action can be taken on what is selected, and why not when it cannot.
    ///
    /// `Ok(())` when every selected entry can take part. **Nothing selected is a refusal with
    /// its own wording**, rather than an action that appears available and does nothing.
    pub(crate) fn offers(&self, offer: Offer) -> Result<(), String> {
        let picked = self.picked();
        if picked.is_empty() {
            return Err("nothing is selected".to_owned());
        }
        // **One, because a run starts a payload.** Which side the selected row is on decides
        // *how* it runs - the copy here goes to the loader, a copy that is only on the target
        // is started where it already is - and neither is a thing to do to five rows at once.
        // This is the only condition that greys it.
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

    /// **One name is one row, whichever sides it is on.**
    ///
    /// The whole point of the merged model: a thing on both sides is one entry with two
    /// columns, not two entries that happen to look alike.
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

    /// Matched case-insensitively, because a list and a disk spell things differently.
    #[test]
    fn one_thing_spelled_two_ways_is_still_one_thing() {
        let described = Manifest::from_json(r#"[{ "name": "elfldr", "filename": "ELFLDR.ELF" }]"#)
            .expect("reads");
        let listing = Listing::build(&described, &[item("elfldr.elf")], &[]);
        assert_eq!(listing.entries.len(), 1, "{:?}", listing.entries);
        assert_eq!(listing.entries[0].standing(), Standing::OnlyHere);
        assert!(listing.entries[0].described.is_some());
    }

    /// **On neither side is its own state.** A payload nobody has fetched and a file somebody
    /// deleted are different situations, and one box for both would hide that.
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

    /// **Nothing selected refuses, in its own words.**
    ///
    /// An action that looks available and does nothing is the defect this project is about,
    /// in a toolbar.
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

        // Add something that is only on the target, and sending stops applying.
        listing.toggle("there.pkg");
        let refused = listing.offers(Offer::Send).expect_err("mixed selection");
        assert!(
            refused.contains("there.pkg"),
            "the reason should name what is in the way: {refused}"
        );
    }

    /// **A refusal names one thing, not fifty.** A reason listing every offender is one
    /// nobody reads to the end of.
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

    /// Downloading needs a url and a digest that can be checked - the same rule the fetching
    /// code enforces, asked before the button is offered rather than after it is pressed.
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

    /// **Installing needs the package on *this* machine**, because the target fetches it
    /// from here.
    ///
    /// This inverts what it used to assert. A package already on the target cannot be
    /// installed from there: `pkg_install` takes a url, and a path on the target's own disk
    /// gives it nothing it can read - measured against a real package in `/data/pkg`, which
    /// produced the same empty answer as a file that was not there.
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

    /// And it still has to be a package.
    #[test]
    fn installing_still_needs_a_package() {
        let mut listing = two_sided();
        listing.toggle("here.elf");
        let refused = listing
            .offers(Offer::Install)
            .expect_err("an elf is not a package");
        assert!(refused.contains("not a package"), "{refused}");
    }

    /// **A tick for a row that is gone is dropped when the list is rebuilt.**
    ///
    /// The list is rebuilt whenever either side changes. A selection naming something absent
    /// would either do nothing or act on the wrong thing, and both happen quietly.
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

    /// **Three spellings of one payload are one row.**
    ///
    /// Measured on a real setup: the description says `elfldr-ps5.elf`, the disk holds
    /// `elfldr_v0.25.elf`, and the target keeps a directory called `elfldr`. As three rows, the
    /// one carrying the description had no local file - so `run` was refused for a payload that
    /// was downloaded, with a message naming a file that was on the disk.
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

        // And the row the payload table ticks - keyed by the description's filename - is the
        // row that has the local file, which is what `run` needs.
        listing.toggle("elfldr-ps5.elf");
        assert!(
            listing.offers(Offer::Run).is_ok(),
            "{:?}",
            listing.offers(Offer::Run)
        );
    }

    /// **A name that merely starts the same is a different payload.**
    ///
    /// `kstuff` and `kstuff-lite` are two kernel patches and a chain names one of them. Merging
    /// them here would put one payload's local copy under the other's description, which is the
    /// same wrong answer this rule was written for in the boot list.
    #[test]
    fn a_longer_name_is_not_a_version_of_a_shorter_one() {
        let described = Manifest::from_json(r#"[{ "name": "kstuff", "filename": "kstuff.elf" }]"#)
            .expect("reads");
        let listing = Listing::build(&described, &[item("kstuff-lite_v1.10.elf")], &[]);
        assert_eq!(listing.entries.len(), 2, "{:?}", listing.entries);
    }

    /// A folder is known per side, because every action reads one side and writes the other.
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

    /// **A file here and a directory there can still be run.**
    ///
    /// This is how every payload on a prepared target looks: the manager keeps each one in
    /// `/data/pldmgr/payloads/<name>/`, so the far side of a payload whose local copy is a
    /// perfectly ordinary ELF is a directory. Running reads the local file and sends it; it
    /// never looks at what the target keeps under that name. Asking whether *either* side was
    /// a folder refused every one of them, with a reason - "pldmgr is a folder" - that was true
    /// of something the action was not going to touch.
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
        assert!(
            listing.offers(Offer::DeleteThere).is_err(),
            "deleting on the target still reads the target's side, where it is a directory"
        );
    }
}
