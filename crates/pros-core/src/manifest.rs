//! Where payloads come from.
//!
//! # Nothing is shipped, only described
//!
//! This project distributes **no payload binaries**. Three reasons, in order of weight: the
//! payloads are licensed such that redistributing a binary obliges you to offer its source
//! and pointing at the upstream obliges nothing; URLs rot, and a rotted URL should be a text
//! edit rather than a release; and obSCEne's own build already refuses to track a `.elf`, a
//! habit worth inheriting rather than arguing with.
//!
//! # The schema is copied rather than invented
//!
//! The payload manager on the target keeps a repository description with exactly the right
//! fields already. Copying it costs nothing and buys something real: **a target that is
//! already configured is already described**, so its own repository can be read as a source
//! instead of being typed in again.
//!
//! # The shape of that file has not been measured, and this does not pretend otherwise
//!
//! The field names are known. Whether the document is a list, or an object keyed by name,
//! or a wrapper around either, is not. So this recognises the shapes that are plausible and
//! **names what it found** when it recognises none - rather than assuming one, and reporting
//! an empty repository for a file that was full.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::checksum::{Checksum, Unreadable};

/// One payload, described.
///
/// Every field beyond a name is optional because this is somebody else's document and a
/// missing description is not a reason to refuse the entry. The one field whose absence
/// *does* matter is the checksum, and that is refused where it is used rather than here -
/// see [`Payload::checksum`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    /// What the payload is called.
    pub name: String,
    /// The file it arrives as.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Where to fetch it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Where it comes from, for a person.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Where it comes from, for a machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_direct: Option<String>,
    /// Which build this describes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// When the description was last touched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update: Option<String>,
    /// The digest the file should have, exactly as the document states it.
    ///
    /// Kept as text rather than parsed on the way in, so a manifest carrying a digest this
    /// cannot check still **loads** and still **reports** - and fails at the point somebody
    /// tries to trust it, where the message can say what to do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    /// How the publisher groups it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// What it is for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Which file to take out of an archive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract_file: Option<String>,
    /// How to pick the right asset from a release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_pattern: Option<String>,
    /// The port it listens on once it is running, if it listens.
    ///
    /// # This field is ours, and the rest are not
    ///
    /// Everything above was copied from the file a payload manager keeps on the target.
    /// That file is **one tool's cache**, not a standard: this project has seen exactly one
    /// instance of it and has no evidence of a consensus behind it. Copying it bought
    /// interoperability with the thing that exists, which was worth more than a format of
    /// our own.
    ///
    /// This one is an addition, and it buys something nothing else can: **presence.** Whether
    /// a payload is running is answered by connecting to a port, so without one the answer
    /// is *nothing here can tell* - which is honest and is not useful. Five services have
    /// ports this project measured; every other entry is unknowable until something says.
    ///
    /// A description that mentions a port in prose is not this. Reading a number out of
    /// somebody's sentence is guessing at meaning, and a wrong guess here reports one
    /// payload's state as another's.
    ///
    /// **It survives a merge with a target's repository**, because that repository does not
    /// carry the field and an absence is not a correction. See [`Manifest::merged_with`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// What becomes possible once this answers.
    ///
    /// **Only meaningful beside [`Self::port`]**, since without one there is nothing to be
    /// answering. It is the third column of the check: a reader told *8082 is open* has a
    /// worse tool than one told *saves can be decrypted*.
    ///
    /// Absent, a declared service still appears, described as itself. That is worse prose and
    /// the same fact, which is the right way round for an optional field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlocks: Option<String>,
    /// Whether there is no workflow at all without this.
    ///
    /// **This is the field that can block a check**, so it defaults to absent rather than to
    /// `false`: declaring something required is a claim about somebody else's workflow, and
    /// the only person who can make it is the one editing the list.
    ///
    /// A required payload that is missing blocks exactly as a compiled-in one does. Before
    /// this existed, a declared payload was probed and its result was kept somewhere the
    /// verdict never looked - so the tool could know a required thing was down and still
    /// report *ready*.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl Payload {
    /// The digest this payload must have, if it can be used.
    ///
    /// # Errors
    ///
    /// [`Unreadable::Absent`] when the entry states none, and the other variants when it
    /// states one this cannot check. **Both are errors**, because a payload that is about to
    /// be run with kernel-adjacent privileges and cannot be checked is exactly the case a
    /// silent pass would hide.
    pub fn checksum(&self) -> Result<Checksum, Unreadable> {
        match self.checksum.as_deref() {
            None => Err(Unreadable::Absent),
            Some(text) => Checksum::parse(text),
        }
    }

    /// Takes the facts that change from another description of the same payload.
    ///
    /// Fields the other side does not state are left alone: **an absence is not a
    /// correction**, and somebody's hand-written description should survive a repository
    /// that carries none.
    fn take_facts_from(&mut self, other: &Self) {
        // Ordered as they are in the file, so a reader can check none is missed.
        if other.filename.is_some() {
            self.filename.clone_from(&other.filename);
        }
        if other.url.is_some() {
            self.url.clone_from(&other.url);
        }
        if other.source.is_some() {
            self.source.clone_from(&other.source);
        }
        if other.source_direct.is_some() {
            self.source_direct.clone_from(&other.source_direct);
        }
        if other.version.is_some() {
            self.version.clone_from(&other.version);
        }
        if other.last_update.is_some() {
            self.last_update.clone_from(&other.last_update);
        }
        if other.checksum.is_some() {
            self.checksum.clone_from(&other.checksum);
        }
        if other.category.is_some() {
            self.category.clone_from(&other.category);
        }
        if other.description.is_some() {
            self.description.clone_from(&other.description);
        }
        if other.extract_file.is_some() {
            self.extract_file.clone_from(&other.extract_file);
        }
        if other.asset_pattern.is_some() {
            self.asset_pattern.clone_from(&other.asset_pattern);
        }
        if other.port.is_some() {
            self.port = other.port;
        }
        // The same rule as `port`, and for the same reason: a target's own repository does not
        // carry these, so its silence about them is not a correction.
        if other.unlocks.is_some() {
            self.unlocks.clone_from(&other.unlocks);
        }
        if other.required.is_some() {
            self.required = other.required;
        }
    }

    /// This entry as a service to be probed, when it says enough to be one.
    ///
    /// **A port is the whole requirement.** Without one there is nothing to connect to, and
    /// presence is unanswerable - which is honest and is not a service.
    #[must_use]
    pub fn as_service(&self) -> Option<pros_link::service::Service> {
        let port = self.port?;
        Some(pros_link::service::Service::declared(
            self.name.clone(),
            port,
            self.unlocks
                .clone()
                .unwrap_or_else(|| format!("use {}", self.name)),
            self.required.unwrap_or(false),
            // **Never from here.** Whether a service is a way back into a target is this
            // program's judgement about its own recovery, not a fact a payload repository has
            // an opinion about - so it is stated in `catalogue`, in a file this project owns.
            false,
            false,
        ))
    }

    /// Whether this entry can be verified at all, without saying anything about a file.
    ///
    /// For reporting on a manifest as a whole - which entries are usable and which need
    /// somebody to find a digest - rather than for deciding whether to send anything.
    #[must_use]
    pub fn is_verifiable(&self) -> bool {
        self.checksum().is_ok()
    }
}

/// Where the payload manager keeps its repository.
///
/// **Measured against a target on 2026-08-26**, along with what is in it: a plain JSON
/// array of 25 entries carrying `name`, `filename`, `url`, `source`, `source_direct`,
/// `version`, `last_update`, `checksum`, `category` and `description` - the schema this
/// project copied rather than invented, confirmed to have been worth copying.
///
/// Its digests are **64 bare hexadecimal characters**, which is SHA-256, which is the one
/// algorithm this project verifies. That was an open question until a target answered it.
///
/// It is a constant rather than a parameter for the same reason the boot list's path is:
/// somebody looked. (D013)
pub const TARGET_REPOSITORY: &str = "/data/pldmgr/repository_cache.json";

/// The payloads this project expects a target to be running.
///
/// # What it claims, and what it deliberately does not
///
/// Names, urls and digests, read off a target's own payload-manager repository.
///
/// **This used to state neither url nor digest**, on the grounds that nothing here had
/// measured them. That was right until a target handed the measurements over; keeping the
/// stub afterwards would have been a different dishonesty - pretending not to know something
/// that had been checked end to end.
///
/// Equivalent to `Tracked::Payloads.shipped()`, kept because it is the older name and reads
/// better at call sites that only ever mean payloads.
#[must_use]
pub fn recommended() -> Manifest {
    Tracked::Payloads.shipped()
}

/// A kind of thing this project can track, fetch, verify and send.
///
/// # One mechanism, five lists
///
/// Describe, fetch, check the digest, keep, send: none of it cares what kind of thing it is
/// moving, so every kind gets all of it for nothing. What differs is what an honest list can
/// contain, and that is decided per kind rather than by whether anybody got round to it:
///
/// - **Payloads** are published as files by the people who write them, with digests. The
///   shipped list came off a target's own repository.
/// - **Packages** and **titles** are the same artifact - zip bundles launched through the
///   homebrew server, installed under `/data/homebrew`. The split between them is this
///   project's convenience, not an upstream distinction, and nothing depends on it being
///   right. `.pkg` files are a different thing, they do exist, and none are listed here
///   because no public source of them with digests was found - which is a gap in what has
///   been measured, not a claim that the format is unused.
/// - **Titles carries no commercial games and never will.** A list of urls for commercial
///   titles is a list of pirated games. Open-source engines, published by their own authors,
///   are a different thing and are listed.
/// - **Cheats** are published as files too, pinned to a commit so their digests cannot go
///   stale under them.
/// - **Saves ship an empty list on purpose.** On this platform a save is signed for the
///   target that wrote it, so a downloaded save is a file the target rejects - a list of
///   them would be a list of things that do not work, each entry looking exactly like one
///   that does. The section still has two sides to copy between, which is the whole job.
///
/// Every kind has a file so that the format is documented and an empty one can say why it is
/// empty rather than looking broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tracked {
    /// Things a loader runs.
    Payloads,
    /// Homebrew applications - emulators, players, tools.
    Packages,
    /// Homebrew games. Never commercial ones.
    Titles,
    /// Cheat tables, for whatever runs them.
    Cheats,
    /// Saves, which are yours and are not downloadable. Shipped empty.
    Saves,
}

impl Tracked {
    /// The file this kind's list lives in.
    const fn file(self) -> &'static str {
        match self {
            Self::Payloads => "payloads.json",
            Self::Packages => "packages.json",
            Self::Titles => "titles.json",
            Self::Cheats => "cheats.json",
            Self::Saves => "saves.json",
        }
    }

    /// Every kind, for anything that has to cover all of them.
    pub const ALL: [Self; 5] = [
        Self::Payloads,
        Self::Packages,
        Self::Titles,
        Self::Cheats,
        Self::Saves,
    ];

    /// The list this project ships for this kind.
    ///
    /// **Compiled in so a fresh install is useful before it has seen a target**, and written
    /// out to disk on first use so a correction needs a text editor rather than a rebuild.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: a shipped list that does not parse is caught by
    /// `every_shipped_list_reads`, not discovered by somebody at runtime.
    #[must_use]
    pub fn shipped(self) -> Manifest {
        let text = match self {
            Self::Payloads => include_str!("../data/recommended.json"),
            Self::Packages => include_str!("../data/packages.json"),
            Self::Titles => include_str!("../data/titles.json"),
            Self::Cheats => include_str!("../data/cheats.json"),
            Self::Saves => include_str!("../data/saves.json"),
        };
        Manifest::from_json(text)
            .unwrap_or_else(|why| unreachable!("a shipped list should always read: {why}"))
    }

    /// Where this kind's list is kept.
    #[must_use]
    pub fn path(self) -> Option<std::path::PathBuf> {
        let mut path = crate::target::directory()?;
        path.push(self.file());
        Some(path)
    }

    /// The list on disk, or the one this project ships.
    ///
    /// **A machine that has never run this still gets an answer.** The alternative - an empty
    /// section until somebody finds a list - tells a newcomer to already know what they came
    /// here to find out.
    ///
    /// A file on disk always wins, including an empty one somebody emptied on purpose.
    ///
    /// # Errors
    ///
    /// Only when a file exists and cannot be read as a manifest - which is worth reporting,
    /// because somebody wrote it.
    pub fn read(self) -> Result<Manifest, NotAManifest> {
        match self.path().filter(|path| path.exists()) {
            Some(path) => Manifest::from_file(&path),
            None => Ok(self.shipped()),
        }
    }
}

/// Where this project keeps its own manifest when nobody names one.
///
/// Beside the registry, for the same reason the registry is not under the per-user
/// application data directory: a file somebody cannot find is worse than no file.
#[must_use]
pub fn default_path() -> Option<std::path::PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("payloads.json");
    Some(path)
}

/// Where a fetched payload is kept before it is sent.
///
/// **Separate from the manifest on purpose.** The manifest is a description and is worth
/// editing by hand; this holds binaries, which are not, and which this project never ships.
///
/// # Why this is the data directory and not the cache one
///
/// It was the cache directory, on the reasoning that anything here carries a url and a digest
/// and can therefore be fetched again - which is a good rule and was the wrong answer, because
/// **the payloads screen never agreed to it**. Every other section takes its local folder from
/// `data_root()/<section>`, and the payloads section is a section: the listing behind its
/// `run`, `send` and `delete here` buttons was reading `data_root()/payloads` while downloads,
/// the size column, and every *is it here already* question were reading the cache one.
///
/// So a payload fetched through this program landed in a directory the same screen did not
/// list, and the button that would have sent it said it was not on this machine. One name, two
/// directories, and the half of the screen that measured disagreeing with the half that acted -
/// which is this project's own defect, in the folder its files live in.
#[must_use]
pub fn staging() -> Option<std::path::PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("payloads");
    Some(path)
}

/// A set of payload descriptions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    payloads: Vec<Payload>,
}

impl Manifest {
    /// Builds a manifest from entries.
    #[must_use]
    pub fn new(payloads: Vec<Payload>) -> Self {
        Self { payloads }
    }

    /// Everything described.
    #[must_use]
    pub fn payloads(&self) -> &[Payload] {
        &self.payloads
    }

    /// One entry by name.
    /// **Case-insensitively**, because a repository writing `nanoDNS` and a list writing
    /// `nanodns` are describing one payload. Matching exactly produced both, side by side,
    /// which is the duplication this project was asked to stop having.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Payload> {
        self.payloads
            .iter()
            .find(|payload| payload.name.eq_ignore_ascii_case(name))
    }

    /// Replaces one description, or adds it when the list has never heard of it.
    ///
    /// **Matched by name, case-insensitively**, the same as everywhere else these are
    /// compared. A list holding `elfldr` and an update naming `ELFLDR` are one payload, and
    /// keeping both would leave two entries fighting over one filename.
    pub fn absorb(&mut self, one: Payload) {
        match self
            .payloads
            .iter_mut()
            .find(|kept| kept.name.eq_ignore_ascii_case(&one.name))
        {
            Some(kept) => *kept = one,
            None => self.payloads.push(one),
        }
    }

    /// Entries whose checksum cannot be used, with the reason.
    ///
    /// **The point of having this at all.** A manifest reports on itself, so a person can
    /// see which entries are trustworthy before a workflow discovers it one payload at a
    /// time, half way through a job.
    #[must_use]
    pub fn unverifiable(&self) -> Vec<(&str, Unreadable)> {
        self.payloads
            .iter()
            .filter_map(|payload| match payload.checksum() {
                Ok(_) => None,
                Err(why) => Some((payload.name.as_str(), why)),
            })
            .collect()
    }

    /// Reads a manifest from JSON, whatever plausible shape it is in.
    ///
    /// # Errors
    ///
    /// [`Unreadable`] describing what the document turned out to be, when it is not a shape
    /// this recognises.
    ///
    /// [`Unreadable`]: NotAManifest
    pub fn from_json(text: &str) -> Result<Self, NotAManifest> {
        let document: serde_json::Value =
            serde_json::from_str(text).map_err(|error| NotAManifest::NotJson {
                said: error.to_string(),
            })?;
        Self::from_value(document)
    }

    /// Reads a manifest from a file.
    ///
    /// # Errors
    ///
    /// As [`Manifest::from_json`], and [`NotAManifest::Unreadable`] when the file cannot be
    /// read at all.
    pub fn from_file(path: &Path) -> Result<Self, NotAManifest> {
        let text = std::fs::read_to_string(path).map_err(|error| NotAManifest::Unreadable {
            path: path.display().to_string(),
            said: error.to_string(),
        })?;
        Self::from_json(&text)
    }

    /// Writes the manifest as a list, which is the shape this project's own file uses.
    ///
    /// # Errors
    ///
    /// Propagates a serialisation failure, which for this shape means somebody has put
    /// something unrepresentable in a field.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.payloads)
    }

    /// Takes everything another manifest knows that this one does not.
    ///
    /// # Why merging rather than replacing
    ///
    /// A target's repository is curated and updated; a local file may have been edited by
    /// hand. Replacing loses the edits, and keeping both means two lists that disagree and a
    /// person choosing between them every time - which is how nine entries becoming
    /// twenty-five reads as an explosion rather than as **finding out about sixteen more**.
    ///
    /// So: one list. Entries are matched by name.
    ///
    /// - **The other side wins for facts that change** - url, digest, version, when it was
    ///   updated, category. Those are what a repository is for, and a stale digest is worse
    ///   than no digest because it fails a download that was fine.
    /// - **This side keeps anything the other does not say.** A description somebody wrote
    ///   is not overwritten by an absence.
    /// - Entries only one side has are kept, both ways. Nothing is dropped for being
    ///   unfamiliar.
    #[must_use]
    pub fn merged_with(&self, other: &Self) -> Self {
        let mut payloads = self.payloads.clone();

        for incoming in &other.payloads {
            match payloads
                .iter_mut()
                .find(|existing| existing.name.eq_ignore_ascii_case(&incoming.name))
            {
                Some(existing) => {
                    // The repository's spelling wins, because it is the one every other
                    // tool reading that file shows and the one its own urls are built from.
                    existing.name.clone_from(&incoming.name);
                    existing.take_facts_from(incoming);
                }
                None => payloads.push(incoming.clone()),
            }
        }
        payloads.sort_by(|left, right| left.name.cmp(&right.name));
        Self { payloads }
    }

    /// What changed between two manifests, for saying so out loud.
    ///
    /// **A merge that silently altered a file is a merge nobody can review.** Returns how
    /// many were added and how many were filled in.
    #[must_use]
    pub fn difference_from(&self, before: &Self) -> (usize, usize) {
        let added = self
            .payloads
            .iter()
            .filter(|payload| before.find(&payload.name).is_none())
            .count();
        let changed = self
            .payloads
            .iter()
            .filter(|payload| {
                before
                    .find(&payload.name)
                    .is_some_and(|was| was != *payload)
            })
            .count();
        (added, changed)
    }

    /// Writes the manifest where this project keeps it.
    ///
    /// # Errors
    ///
    /// Propagates the write, and reports a machine with no home directory - at which point
    /// there is nowhere for any of this to live.
    pub fn save(&self) -> Result<std::path::PathBuf, String> {
        let path = default_path().ok_or("no home directory, so there is nowhere to keep it")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|why| why.to_string())?;
        }
        let text = self.to_json().map_err(|why| why.to_string())?;
        std::fs::write(&path, text).map_err(|why| why.to_string())?;
        Ok(path)
    }

    /// Recognises the document.
    fn from_value(document: serde_json::Value) -> Result<Self, NotAManifest> {
        // A plain list: this project's own shape.
        if let serde_json::Value::Array(entries) = document {
            return read_entries(entries);
        }

        let serde_json::Value::Object(fields) = document else {
            return Err(NotAManifest::Unexpected {
                found: "a value that is neither a list nor an object".to_owned(),
            });
        };

        // A wrapper around a list, under any of the names such a file plausibly uses.
        for key in ["payloads", "entries", "repository", "items"] {
            if let Some(serde_json::Value::Array(entries)) = fields.get(key) {
                return read_entries(entries.clone());
            }
        }

        // An object keyed by name, where each value is an entry. Recognised by its values
        // rather than asserted: a single non-entry value means this is a different document
        // that happens to be an object, and guessing would report it as empty.
        let named: BTreeMap<&String, &serde_json::Value> = fields.iter().collect();
        if !named.is_empty()
            && named.values().all(|value| {
                value
                    .as_object()
                    .is_some_and(|entry| entry.contains_key("url"))
            })
        {
            let mut payloads = Vec::with_capacity(named.len());
            for (name, value) in named {
                // **The key is the name, and it is supplied before the entry is read.**
                //
                // The alternative - making the name optional and filling it in afterwards -
                // would also let a *list* entry through with no name at all, and a payload
                // that cannot be named cannot be asked for. Strict where it matters, and
                // this shape simply carries the name somewhere else.
                let mut entry = value.clone();
                if let Some(fields) = entry.as_object_mut() {
                    fields
                        .entry("name")
                        .or_insert_with(|| serde_json::Value::String(name.clone()));
                }
                let payload: Payload =
                    serde_json::from_value(entry).map_err(|error| NotAManifest::BadEntry {
                        which: name.clone(),
                        said: error.to_string(),
                    })?;
                payloads.push(payload);
            }
            return Ok(Self { payloads });
        }

        Err(NotAManifest::Unexpected {
            found: format!(
                "an object with the fields {:?}, none of which is a list of payloads",
                fields.keys().take(8).collect::<Vec<_>>()
            ),
        })
    }
}

/// Reads a list of entries.
fn read_entries(entries: Vec<serde_json::Value>) -> Result<Manifest, NotAManifest> {
    let mut payloads = Vec::with_capacity(entries.len());
    for (index, entry) in entries.into_iter().enumerate() {
        let payload: Payload =
            serde_json::from_value(entry).map_err(|error| NotAManifest::BadEntry {
                which: format!("entry {index}"),
                said: error.to_string(),
            })?;
        payloads.push(payload);
    }
    Ok(Manifest { payloads })
}

/// Why a document could not be read as a manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAManifest {
    /// The file could not be read.
    Unreadable {
        /// Which file.
        path: String,
        /// What the operating system said.
        said: String,
    },
    /// The text is not JSON.
    NotJson {
        /// What the parser said, including where.
        said: String,
    },
    /// One entry could not be read, and the rest are therefore in doubt.
    BadEntry {
        /// Which entry, by name or position.
        which: String,
        /// What the parser said.
        said: String,
    },
    /// The document is JSON, and is not a manifest.
    ///
    /// **Names what it found.** The failure this avoids is reporting an empty repository for
    /// a file that was full but shaped differently, which looks like a target with no
    /// payloads configured rather than like a tool that did not understand the file.
    Unexpected {
        /// What it turned out to be.
        found: String,
    },
}

impl fmt::Display for NotAManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, said } => write!(f, "could not read {path}: {said}"),
            Self::NotJson { said } => write!(f, "not JSON: {said}"),
            Self::BadEntry { which, said } => write!(f, "{which} could not be read: {said}"),
            Self::Unexpected { found } => write!(
                f,
                "this is not a payload repository - it is {found}. \
                 Reporting it as empty would have been worse than saying so"
            ),
        }
    }
}

impl std::error::Error for NotAManifest {}

#[cfg(test)]
mod tests {
    use super::Tracked;

    /// Where the format is written down.
    const SCHEMA: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/manifest.schema.json"
    );

    use super::{Manifest, NotAManifest, Payload};

    const ONE: &str = r#"[
        {
            "name": "elfldr",
            "filename": "elfldr.elf",
            "url": "https://example.invalid/elfldr.elf",
            "checksum": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "category": "loader",
            "description": "sends it a payload, it runs it"
        }
    ]"#;

    /// The shape this project writes.
    #[test]
    fn a_list_of_entries_reads() {
        let manifest = Manifest::from_json(ONE).expect("a list is a manifest");
        assert_eq!(manifest.payloads().len(), 1);
        let found = manifest.find("elfldr").expect("by name");
        assert_eq!(found.category.as_deref(), Some("loader"));
        assert!(found.is_verifiable());
    }

    /// A file keyed by name, which is a plausible shape for the target's own repository.
    #[test]
    fn an_object_keyed_by_name_reads_and_takes_its_names_from_the_keys() {
        let text = r#"{
            "klogsrv": { "url": "https://example.invalid/klogsrv.elf", "version": "1.2" },
            "shsrv":   { "url": "https://example.invalid/shsrv.elf" }
        }"#;
        let manifest = Manifest::from_json(text).expect("an object of entries is a manifest");
        assert_eq!(manifest.payloads().len(), 2);
        assert!(manifest.find("klogsrv").is_some());
        assert_eq!(
            manifest.find("klogsrv").and_then(|p| p.version.as_deref()),
            Some("1.2")
        );
    }

    /// A payload that cannot be named cannot be asked for, so a list entry without one is
    /// refused rather than stored with an empty name.
    #[test]
    fn a_list_entry_with_no_name_is_refused() {
        assert!(matches!(
            Manifest::from_json(r#"[{ "url": "https://example.invalid/x.elf" }]"#),
            Err(NotAManifest::BadEntry { .. })
        ));
    }

    /// A wrapper around a list.
    #[test]
    fn a_wrapped_list_reads() {
        let text = format!(r#"{{ "version": 2, "payloads": {ONE} }}"#);
        let manifest = Manifest::from_json(&text).expect("a wrapped list is a manifest");
        assert_eq!(manifest.payloads().len(), 1);
    }

    /// Something else is named, not reported as empty.
    ///
    /// The failure this pins is the quiet one: a full file in an unrecognised shape read as
    /// a target with nothing configured.
    #[test]
    fn a_document_that_is_not_a_manifest_says_so_rather_than_reading_as_empty() {
        let error = Manifest::from_json(r#"{"status":"ok","count":25}"#)
            .expect_err("that is not a repository");
        match error {
            NotAManifest::Unexpected { found } => {
                assert!(
                    found.contains("status"),
                    "it does not say what it saw: {found}"
                );
            }
            other => panic!("expected a named refusal, got {other:?}"),
        }
    }

    /// Unknown fields are kept in the sense that they do not stop the read - the file
    /// belongs to another tool and is allowed to carry more than this knows about.
    #[test]
    fn an_entry_with_extra_fields_still_reads() {
        let text = r#"[{ "name": "x", "url": "u", "something_new": 42 }]"#;
        assert_eq!(
            Manifest::from_json(text)
                .expect("extra fields are not an error")
                .payloads()
                .len(),
            1
        );
    }

    /// A manifest reports on its own trustworthiness before a workflow discovers it one
    /// payload at a time, half way through a job.
    #[test]
    fn a_manifest_says_which_entries_cannot_be_verified() {
        let text = r#"[
            { "name": "good", "checksum": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" },
            { "name": "old",  "checksum": "d41d8cd98f00b204e9800998ecf8427e" },
            { "name": "bare" }
        ]"#;
        let manifest = Manifest::from_json(text).expect("reads");
        let doubtful = manifest.unverifiable();
        assert_eq!(doubtful.len(), 2);
        let names: Vec<&str> = doubtful.iter().map(|(name, _)| *name).collect();
        assert!(
            names.contains(&"old") && names.contains(&"bare"),
            "{names:?}"
        );
    }

    /// The list this project ships reads, and covers every service a check asks about.
    ///
    /// A recommended set that omitted one of them would let a target pass a check while
    /// the list said nothing was missing.
    #[test]
    fn the_built_in_list_covers_every_service() {
        let manifest = super::recommended();
        for service in pros_link::service::SERVICES {
            assert!(
                manifest.find(service.name.as_ref()).is_some(),
                "{} is checked for and is not in the recommended list",
                service.name
            );
        }
    }

    /// **Every entry can be fetched and every entry can be checked.**
    ///
    /// This inverts what it used to assert. The list shipped here once carried no url and no
    /// checksum, on the grounds that this project had measured neither and a list stating
    /// them would be asserting facts nobody established. That was right at the time and is
    /// no longer true: the entries were read off a target's own repository, and one of them
    /// was fetched and verified end to end.
    ///
    /// So the guarantee is now the useful one. A url with no digest is the dangerous
    /// combination - it invites a download that cannot be checked - and this makes shipping
    /// one impossible rather than merely discouraged.
    #[test]
    fn every_entry_shipped_can_be_fetched_and_verified() {
        for payload in super::recommended().payloads() {
            assert!(
                payload.url.is_some(),
                "{} cannot be fetched, so listing it only describes something out of reach",
                payload.name
            );
            assert!(
                payload.checksum().is_ok(),
                "{} states a url but no digest this can check - the one combination that                  invites an unverifiable download",
                payload.name
            );
        }
    }

    /// Every entry says what it is for and what kind of thing it is.
    ///
    /// A list of bare names is a worse answer than the reader already had, and an entry with
    /// no category falls into the group nobody is looking for.
    #[test]
    fn the_built_in_list_says_what_each_one_is() {
        for payload in super::recommended().payloads() {
            assert!(
                payload
                    .description
                    .as_ref()
                    .is_some_and(|what| !what.trim().is_empty()),
                "{} is listed without saying what it does",
                payload.name
            );
            assert!(
                payload
                    .category
                    .as_ref()
                    .is_some_and(|what| !what.trim().is_empty()),
                "{} has no category, so it groups under 'not categorised'",
                payload.name
            );
        }
    }

    /// **The list is big enough to be the answer rather than a sample of one.**
    ///
    /// It shipped with nine entries and a note saying to read a target for the rest, which
    /// made a fresh install useless until somebody had target. Pinning a floor stops that
    /// quietly coming back if the file is ever regenerated from something thinner.
    #[test]
    fn the_built_in_list_is_the_whole_list() {
        let manifest = super::recommended();
        assert!(
            manifest.payloads().len() >= 25,
            "the shipped list has shrunk to {} entries",
            manifest.payloads().len()
        );
        assert!(
            manifest.unverifiable().is_empty(),
            "{} entries cannot be verified",
            manifest.unverifiable().len()
        );
    }

    /// **One list, not two.** The target knows more; this takes what it knows.
    #[test]
    fn merging_fills_in_what_the_target_knows() {
        let mine = Manifest::from_json(r#"[{ "name": "elfldr", "description": "my own note" }]"#)
            .expect("reads");
        let theirs = Manifest::from_json(
            r#"[
                { "name": "elfldr", "url": "https://example.invalid/elfldr.elf",
                  "checksum": "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" },
                { "name": "zftpd", "url": "https://example.invalid/zftpd.elf" }
            ]"#,
        )
        .expect("reads");

        let merged = mine.merged_with(&theirs);
        assert_eq!(merged.payloads().len(), 2, "the new one should be kept");

        let elfldr = merged.find("elfldr").expect("still there");
        assert!(elfldr.url.is_some(), "the url should have come across");
        assert!(elfldr.is_verifiable(), "and the digest with it");
        assert_eq!(
            elfldr.description.as_deref(),
            Some("my own note"),
            "an absence is not a correction - a hand-written description survived"
        );
    }

    /// **Two spellings of one name are one payload.**
    ///
    /// A real repository writes `nanoDNS` and this project's own list wrote `nanodns`.
    /// Matching exactly kept both, which is precisely the duplication a merge is for
    /// avoiding - and the repository's spelling is the one that wins, because it is what
    /// every other tool reading that file shows.
    #[test]
    fn a_difference_of_case_is_not_a_different_payload() {
        let mine = Manifest::from_json(r#"[{ "name": "nanodns" }]"#).expect("reads");
        let theirs =
            Manifest::from_json(r#"[{ "name": "nanoDNS", "version": "0.4" }]"#).expect("reads");
        let merged = mine.merged_with(&theirs);

        assert_eq!(merged.payloads().len(), 1, "{:?}", merged.payloads());
        assert_eq!(merged.payloads()[0].name, "nanoDNS");
        assert_eq!(merged.payloads()[0].version.as_deref(), Some("0.4"));
    }

    /// **A field this project added survives a merge with a repository that lacks it.**
    ///
    /// The target's file has no `port`. If a merge treated its absence as a correction,
    /// every port anybody wrote down would be erased by the next read - and the erasing
    /// would look exactly like the repository being authoritative.
    #[test]
    fn a_field_the_target_does_not_carry_is_not_erased_by_it() {
        let mine = Manifest::from_json(r#"[{ "name": "websrv", "port": 8080 }]"#).expect("reads");
        let theirs = Manifest::from_json(
            r#"[{ "name": "websrv", "version": "v0.34", "description": "a web server" }]"#,
        )
        .expect("reads");

        let merged = mine.merged_with(&theirs);
        let entry = merged.find("websrv").expect("still there");
        assert_eq!(
            entry.port,
            Some(8080),
            "the port was erased by a file without one"
        );
        assert_eq!(
            entry.version.as_deref(),
            Some("v0.34"),
            "and the version came across"
        );
    }

    /// A merge says what it did, because one that silently rewrote a file is one nobody can
    /// review.
    #[test]
    fn a_merge_says_how_much_it_changed() {
        let mine = Manifest::from_json(r#"[{ "name": "elfldr" }]"#).expect("reads");
        let theirs = Manifest::from_json(
            r#"[{ "name": "elfldr", "version": "v0.25" }, { "name": "new-one" }]"#,
        )
        .expect("reads");
        let merged = mine.merged_with(&theirs);
        assert_eq!(merged.difference_from(&mine), (1, 1));
    }

    /// Nothing is dropped for being unfamiliar, in either direction.
    #[test]
    fn merging_drops_nothing_from_either_side() {
        let mine = Manifest::from_json(r#"[{ "name": "only-mine" }]"#).expect("reads");
        let theirs = Manifest::from_json(r#"[{ "name": "only-theirs" }]"#).expect("reads");
        let merged = mine.merged_with(&theirs);
        assert!(merged.find("only-mine").is_some());
        assert!(merged.find("only-theirs").is_some());
    }

    /// Each kind keeps its own list, and they are different files.
    #[test]
    fn each_kind_has_its_own_list() {
        use super::Tracked;
        let files: Vec<_> = [Tracked::Payloads, Tracked::Packages, Tracked::Cheats]
            .into_iter()
            .filter_map(Tracked::path)
            .collect();
        // On a machine with a home directory there are three, and they are distinct.
        if files.len() == 3 {
            assert_ne!(files[0], files[1]);
            assert_ne!(files[1], files[2]);
        }
    }

    /// **A kind nobody has described is empty, not broken.**
    ///
    /// This project ships a payload list because a target handed one over. It ships none
    /// for the rest, and an empty section somebody can fill in is what that honestly is.
    #[test]
    fn a_kind_with_no_list_yet_reads_as_empty() {
        use super::Tracked;
        // Whatever is on this machine, reading must not fail for a file that is not there.
        for kind in [Tracked::Packages, Tracked::Cheats] {
            let read = kind.read();
            assert!(read.is_ok(), "{kind:?} failed rather than being empty");
        }
    }

    /// What is written can be read.
    #[test]
    fn a_manifest_round_trips() {
        let manifest = Manifest::new(vec![Payload {
            name: "elfldr".to_owned(),
            url: Some("https://example.invalid/elfldr.elf".to_owned()),
            ..Payload::default()
        }]);
        let text = manifest.to_json().expect("writes");
        assert_eq!(Manifest::from_json(&text).expect("reads back"), manifest);
    }

    /// **Every field this reads is described in the schema, and nothing extra is.**
    ///
    /// A schema is a document, and documents drift away from the code that they describe
    /// without anything going red. This makes that go red: add a field to [`Payload`] and
    /// forget the schema, or leave a field in the schema after removing it here, and the
    /// build says so.
    ///
    /// The check is by serialisation rather than by a list written out here, because a list
    /// written out here would be a third thing to keep in step.
    #[test]
    fn the_schema_describes_exactly_the_fields_that_are_read() {
        let everything = Payload {
            name: "x".to_owned(),
            filename: Some(String::new()),
            url: Some(String::new()),
            source: Some(String::new()),
            source_direct: Some(String::new()),
            version: Some(String::new()),
            last_update: Some(String::new()),
            checksum: Some(String::new()),
            category: Some(String::new()),
            description: Some(String::new()),
            extract_file: Some(String::new()),
            asset_pattern: Some(String::new()),
            port: Some(1),
            unlocks: Some(String::new()),
            required: Some(true),
        };
        let json = serde_json::to_value(&everything).expect("serialises");
        let mut fields: Vec<String> = json
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect();
        fields.sort();

        let text = std::fs::read_to_string(SCHEMA).expect("the schema is where the docs say");
        let schema: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        let mut described: Vec<String> = schema["$defs"]["payload"]["properties"]
            .as_object()
            .expect("the payload definition has properties")
            .keys()
            .cloned()
            .collect();
        described.sort();

        assert_eq!(
            fields, described,
            "the schema and the type have drifted apart"
        );
    }

    /// **The ports in the shipped list agree with the table this project probes.**
    ///
    /// Two statements of the same fact now exist - the compiled table and an editable file -
    /// and two statements of one fact disagree eventually. This is the one that notices,
    /// without needing a target to notice it.
    #[test]
    fn the_shipped_ports_agree_with_the_services_this_project_probes() {
        for payload in super::recommended().payloads() {
            let Some(service) = pros_link::service::SERVICES
                .iter()
                .find(|service| service.name.eq_ignore_ascii_case(&payload.name))
            else {
                continue;
            };
            assert_eq!(
                payload.port,
                Some(service.port),
                "the list and the probe table disagree about {}",
                payload.name
            );
        }
    }

    /// And the shipped list is an instance of the schema in the most basic sense: every entry
    /// carries the one field the schema requires.
    #[test]
    fn the_shipped_list_carries_what_the_schema_requires() {
        let text = std::fs::read_to_string(SCHEMA).expect("the schema is where the docs say");
        let schema: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        let required = schema["$defs"]["payload"]["required"]
            .as_array()
            .expect("something is required");
        assert_eq!(required.len(), 1, "only the name should ever be required");
        assert_eq!(required[0], "name");

        for payload in super::recommended().payloads() {
            assert!(!payload.name.trim().is_empty(), "an entry with no name");
        }
    }

    /// **Every list this project ships parses.**
    ///
    /// [`Tracked::shipped`] panics on a list that does not read, which is right - a broken
    /// shipped list is a build fault, not a runtime condition. This is what makes that panic
    /// unreachable rather than merely documented as unreachable.
    #[test]
    fn every_shipped_list_reads() {
        for kind in Tracked::ALL {
            let manifest = kind.shipped();
            for payload in manifest.payloads() {
                assert!(
                    !payload.name.trim().is_empty(),
                    "{kind:?} has an entry with no name"
                );
            }
        }
    }

    /// **Nothing ships with a url it cannot check.**
    ///
    /// A url with no digest is the one combination that invites an unverifiable download.
    /// Applied across every kind, so a list added later cannot quietly arrive without them.
    #[test]
    fn every_shipped_entry_can_be_fetched_and_verified() {
        for kind in Tracked::ALL {
            for payload in kind.shipped().payloads() {
                assert!(
                    payload.url.is_some(),
                    "{kind:?}/{} cannot be fetched, so listing it only describes something \
                     out of reach",
                    payload.name
                );
                assert!(
                    payload.checksum().is_ok(),
                    "{kind:?}/{} states a url but no digest this can check",
                    payload.name
                );
            }
        }
    }

    /// **Saves ship empty, and that is the finding rather than an oversight.**
    ///
    /// A save on this platform is signed for the target that wrote it, so a downloaded one is
    /// a file the target rejects. A list of them would be a list of things that do not work,
    /// each entry indistinguishable from one that does.
    ///
    /// Pinned by a test because "we never got round to it" and "this cannot honestly exist"
    /// look identical in an empty file, and only one of them should survive somebody tidying
    /// up later.
    #[test]
    fn saves_ship_empty_on_purpose() {
        assert!(
            Tracked::Saves.shipped().payloads().is_empty(),
            "a downloadable save is not a usable save - see the file's own note"
        );
    }

    /// **Cheat urls are pinned to a commit, never to a branch.**
    ///
    /// A branch url serves whatever is there today. The moment anybody pushes, the digest
    /// recorded here stops matching - and a digest mismatch reads as a corrupted download,
    /// which sends somebody hunting a network fault that does not exist.
    #[test]
    fn cheat_urls_cannot_change_under_their_digests() {
        for payload in Tracked::Cheats.shipped().payloads() {
            let url = payload.url.as_deref().expect("checked above");
            assert!(
                !url.contains("/main/") && !url.contains("/master/") && !url.contains("/HEAD/"),
                "{} is pinned to a branch, so its digest goes stale on the next push: {url}",
                payload.name
            );
            assert!(
                url.split('/')
                    .any(|part| part.len() == 40 && part.chars().all(|c| c.is_ascii_hexdigit())),
                "{} is not pinned to a commit: {url}",
                payload.name
            );
        }
    }

    /// **Every title is published by whoever wrote it.**
    ///
    /// Titles is the one list where the wrong entry is not merely unhelpful. A commercial game
    /// url makes this a piracy index, so the list is open-source engines and nothing else.
    ///
    /// This cannot test intent, so it tests the property that follows from it: every entry is
    /// published from its own project's release, which is not where commercial games come
    /// from.
    #[test]
    fn every_title_comes_from_its_own_publisher() {
        for payload in Tracked::Titles.shipped().payloads() {
            let source = payload.source.as_deref().unwrap_or_default();
            assert!(
                source.starts_with("https://github.com/"),
                "{} does not name an open publisher: {source:?}",
                payload.name
            );
        }
    }

    /// **Every field used by every shipped list is one the schema describes.**
    ///
    /// The earlier test compares the schema to the type. This compares it to the data, which
    /// is a different way to drift: a list can be hand-edited with a field the schema never
    /// mentioned, and it would be kept and ignored rather than rejected.
    #[test]
    fn the_shipped_lists_use_only_fields_the_schema_describes() {
        let text = std::fs::read_to_string(SCHEMA).expect("the schema is where the docs say");
        let schema: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        let described = schema["$defs"]["payload"]["properties"]
            .as_object()
            .expect("the payload definition has properties");

        for kind in Tracked::ALL {
            let json = serde_json::to_value(kind.shipped().payloads()).expect("serialises");
            for entry in json.as_array().expect("a list") {
                for field in entry.as_object().expect("an object").keys() {
                    assert!(
                        described.contains_key(field),
                        "{kind:?} uses {field}, which the schema does not describe"
                    );
                }
            }
        }
    }
}
