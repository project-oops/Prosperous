//! Where payloads come from.
//!
//! This project distributes no payload binaries, only descriptions: redistributing a binary
//! obliges offering its source, and a moved URL is a text edit rather than a release.
//!
//! The schema is the target payload manager's own repository format, so a configured
//! target's repository reads as a source. A list, an object keyed by name, and a wrapper
//! around a list are all recognised; any other document is refused with a description of what
//! it is, never read as empty.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::checksum::{Checksum, Unreadable};

/// One payload, described.
///
/// Every field beyond a name is optional because the document belongs to another tool. A
/// missing checksum is refused where it is used; see [`Payload::checksum`].
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
    /// Where a local development build lives on this machine, relative to the repository root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local: Option<String>,
    /// Which build this describes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// When the description was last touched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update: Option<String>,
    /// The digest the file should have, exactly as the document states it.
    ///
    /// Kept as text, so a manifest with a digest this cannot check still loads and reports,
    /// and fails only where the digest is trusted.
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
    /// This project's addition to the payload manager's format: presence is measured by
    /// connecting to a port, so without one it is unknown. A port mentioned in a description
    /// is never parsed. It survives a merge with a target's repository, which does not carry
    /// it; see [`Manifest::merged_with`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// What becomes possible once this answers.
    ///
    /// Only meaningful beside [`Self::port`]. It is the third column of the check; absent, a
    /// declared service is described by its name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlocks: Option<String>,
    /// Whether there is no workflow at all without this.
    ///
    /// A missing required payload blocks a check exactly as a compiled-in service does. Absent
    /// rather than `false` by default: only the person editing the list can declare it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl Payload {
    /// The digest this payload must have, if it can be used.
    ///
    /// # Errors
    ///
    /// [`Unreadable::Absent`] when the entry states none, and the other variants when it
    /// states one this cannot check. Both are errors: a payload runs with elevated privileges
    /// on the target and must not pass unchecked.
    pub fn checksum(&self) -> Result<Checksum, Unreadable> {
        match self.checksum.as_deref() {
            None => Err(Unreadable::Absent),
            Some(text) => Checksum::parse(text),
        }
    }

    /// Takes the facts that change from another description of the same payload.
    ///
    /// Fields the other side does not state are left alone: an absence is not a correction.
    fn take_facts_from(&mut self, other: &Self) {
        // In field order, so a missing one is easy to spot.
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
        if other.source_local.is_some() {
            self.source_local.clone_from(&other.source_local);
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
        if other.unlocks.is_some() {
            self.unlocks.clone_from(&other.unlocks);
        }
        if other.required.is_some() {
            self.required = other.required;
        }
    }

    /// This entry as a service to be probed, when it says enough to be one.
    ///
    /// A port is the whole requirement: without one there is nothing to connect to.
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
            // Recovery roles are this program's judgement, stated in `catalogue`, never taken
            // from a payload repository.
            false,
            false,
        ))
    }

    /// Whether this entry can be verified at all, without saying anything about a file.
    ///
    /// For reporting on a manifest as a whole, not for deciding whether to send anything.
    #[must_use]
    pub fn is_verifiable(&self) -> bool {
        self.checksum().is_ok()
    }
}

/// Where the payload manager keeps its repository.
///
/// Measured on a target: a plain JSON array of entries carrying `name`, `filename`, `url`,
/// `source`, `source_direct`, `version`, `last_update`, `checksum`, `category` and
/// `description`. Its digests are 64 bare hexadecimal characters, SHA-256.
///
/// A constant rather than a parameter because it was measured. (D013)
pub const TARGET_REPOSITORY: &str = "/data/pldmgr/repository_cache.json";

/// The payloads this project expects a target to be running.
///
/// Names, urls and digests, read off a target's own payload-manager repository. Equivalent
/// to `Tracked::Payloads.shipped()`, for call sites that only mean payloads.
#[must_use]
pub fn recommended() -> Manifest {
    Tracked::Payloads.shipped()
}

/// A kind of thing this project can track, fetch, verify and send.
///
/// Every kind shares one mechanism (describe, fetch, check the digest, keep, send); what a
/// list may contain differs per kind:
///
/// - Payloads are published as files by their authors, with digests.
/// - Packages and titles are both zip bundles installed under `/data/homebrew`; the split is
///   this project's convenience.
/// - Titles lists open-source engines published by their own authors, never commercial games.
/// - Cheats are pinned to a commit so their digests cannot go stale.
/// - Saves ship empty: a save is signed for the target that wrote it, so a downloaded one is
///   rejected.
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
    /// Compiled in so a fresh install is useful before it has seen a target.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: `every_shipped_list_reads` parses every list.
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
    /// A file on disk is merged with the shipped list and written back when that changes it.
    ///
    /// # Errors
    ///
    /// Only when a file exists and cannot be read as a manifest.
    pub fn read(self) -> Result<Manifest, NotAManifest> {
        match self.path().filter(|path| path.exists()) {
            Some(path) => {
                let on_disk = Manifest::from_file(&path)?;
                let merged = self.shipped().merged_with(&on_disk);
                if merged != on_disk
                    && let Ok(text) = merged.to_json()
                {
                    let _ = std::fs::write(&path, text);
                }
                Ok(merged)
            }
            None => Ok(self.shipped()),
        }
    }
}

/// Where this project keeps its own manifest when nobody names one.
///
/// Beside the registry, where a person can find it.
#[must_use]
pub fn default_path() -> Option<std::path::PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("payloads.json");
    Some(path)
}

/// Where a fetched payload is kept before it is sent.
///
/// Separate from the manifest, which is a hand-editable description; this holds binaries.
/// It is the data directory's `payloads` folder, the same `<section>` folder every other
/// section uses, so fetching, listing and sending all see one directory.
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

    /// One entry by name, case-insensitively: `nanoDNS` and `nanodns` are one payload.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&Payload> {
        self.payloads
            .iter()
            .find(|payload| payload.name.eq_ignore_ascii_case(name))
    }

    /// Replaces one description, or adds it when the list has never heard of it.
    ///
    /// Matched by name, case-insensitively, as everywhere else these are compared.
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
    /// So a person sees which entries are trustworthy before a workflow needs them.
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
    /// [`NotAManifest`] describing what the document turned out to be, when it is not a
    /// shape this recognises.
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
    /// Propagates a serialisation failure.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.payloads)
    }

    /// Takes everything another manifest knows that this one does not.
    ///
    /// A target's repository is curated; a local file may have hand edits. Entries are
    /// matched by name, case-insensitively:
    ///
    /// - The other side wins for every field it states (url, digest, version, category...);
    ///   a stale digest fails a good download.
    /// - This side keeps anything the other does not state.
    /// - Entries only one side has are kept, both ways.
    #[must_use]
    pub fn merged_with(&self, other: &Self) -> Self {
        let mut payloads = self.payloads.clone();

        for incoming in &other.payloads {
            match payloads
                .iter_mut()
                .find(|existing| existing.name.eq_ignore_ascii_case(&incoming.name))
            {
                Some(existing) => {
                    // The repository's spelling wins: every other tool reading it shows that.
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
    /// Returns how many entries were added and how many changed, so a merge can be reviewed.
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
    /// Propagates the write, and reports a machine with no home directory.
    pub fn save(&self) -> crate::Result<std::path::PathBuf> {
        let path = default_path().ok_or_else(|| {
            crate::Error::failed("no home directory, so there is nowhere to keep it")
        })?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = self
            .to_json()
            .map_err(|why| crate::Error::failed(why.to_string()))?;
        std::fs::write(&path, text)?;
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

        // An object keyed by name, recognised only when every value is an entry.
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
                // The key supplies the name before the entry is read, so `name` stays
                // required and a list entry without one is still refused.
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
    /// Names what it found, rather than reading an unrecognised shape as an empty repository.
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

    /// A list of entries, the shape this project writes, reads.
    #[test]
    fn a_list_of_entries_reads() {
        let manifest = Manifest::from_json(ONE).expect("a list is a manifest");
        assert_eq!(manifest.payloads().len(), 1);
        let found = manifest.find("elfldr").expect("by name");
        assert_eq!(found.category.as_deref(), Some("loader"));
        assert!(found.is_verifiable());
    }

    /// An object keyed by name reads, taking names from the keys.
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

    /// A list entry with no name is refused.
    #[test]
    fn a_list_entry_with_no_name_is_refused() {
        assert!(matches!(
            Manifest::from_json(r#"[{ "url": "https://example.invalid/x.elf" }]"#),
            Err(NotAManifest::BadEntry { .. })
        ));
    }

    /// A wrapper around a list reads.
    #[test]
    fn a_wrapped_list_reads() {
        let text = format!(r#"{{ "version": 2, "payloads": {ONE} }}"#);
        let manifest = Manifest::from_json(&text).expect("a wrapped list is a manifest");
        assert_eq!(manifest.payloads().len(), 1);
    }

    /// An unrecognised document is named, never read as empty.
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

    /// Unknown fields do not stop the read.
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

    /// A manifest names its entries that cannot be verified.
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

    /// The shipped list covers every service a check asks about.
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

    /// Every shipped payload has a url and a checkable digest.
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

    /// Every shipped payload has a description and a category.
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

    /// The shipped payload list is the target repository's whole list, all verifiable.
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

    /// A merge fills in what the target knows and keeps local descriptions.
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

    /// Names differing only in case merge into one entry with the repository's spelling.
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

    /// A `port` survives a merge with a repository that does not carry one.
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

    /// A merge reports how many entries it added and changed.
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

    /// Reading a kind whose file is absent succeeds.
    #[test]
    fn a_kind_with_no_list_yet_reads_as_empty() {
        use super::Tracked;
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

    /// The schema describes exactly the fields [`Payload`] serialises.
    #[test]
    fn the_schema_describes_exactly_the_fields_that_are_read() {
        let everything = Payload {
            name: "x".to_owned(),
            filename: Some(String::new()),
            url: Some(String::new()),
            source: Some(String::new()),
            source_direct: Some(String::new()),
            source_local: Some(String::new()),
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

    /// The ports in the shipped list agree with the compiled service table.
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

    /// Every shipped entry carries `name`, the schema's only required field.
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

    /// Every shipped list parses, so the panic in [`Tracked::shipped`] is unreachable.
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

    /// Every shipped entry of every kind has a url and a checkable digest.
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

    /// The saves list ships empty: a save is signed for the target that wrote it.
    #[test]
    fn saves_ship_empty_on_purpose() {
        assert!(
            Tracked::Saves.shipped().payloads().is_empty(),
            "a downloadable save is not a usable save - see the file's own note"
        );
    }

    /// Cheat urls are pinned to a commit, so their digests cannot go stale.
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

    /// Every title comes from its own open-source project's release, never a commercial game.
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

    /// Every field used by every shipped list is one the schema describes.
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
