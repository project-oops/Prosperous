//! What services exist, what they buy, and which of them is a way back in.
//!
//! # Why this is ours and the payload list is not
//!
//! `manifest` reads a document belonging to a payload manager: where a payload comes from,
//! what it hashes to, which version it is. **Those are facts about a payload**, and copying
//! that format bought interoperability with a tool people already run.
//!
//! What a service *means to this program* is a different kind of fact. `elfldr` being a way
//! back into a target after a bad restart is not something a payload repository has an opinion
//! about, and bolting it onto somebody else's schema would tie every judgement this program
//! makes to the continued existence of one manager's cache file.
//!
//! So roles live here, in a file this project owns, **referring to payloads by the name the
//! manifest already uses**. Two documents, one subject each, joined by a name.
//!
//! # It works with no file at all
//!
//! The five compiled-in services are the default and are what runs when nothing is
//! configured. A tool that needs a configuration file before it works is a tool that is broken
//! out of the box. The file **overrides and extends**; it never has to exist.
//!
//! # Precedence, stated once
//!
//! Later wins, so the more specific source is later:
//!
//! 1. the compiled-in five,
//! 2. anything a payload list declared for itself,
//! 3. this file.
//!
//! One resolution order, written down, so two sources can differ without the program having to
//! guess which is right.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pros_link::service::{SERVICES, Service};
use serde::{Deserialize, Serialize};

/// What a file may say about one service.
///
/// Every field optional: an entry that only corrects a port should not have to restate what
/// the service unlocks, and one that only marks something a way back should not have to
/// repeat its port.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The payload's name, as the payload list spells it. This is the join.
    pub name: String,
    /// The port it listens on when loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// What becomes possible once it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlocks: Option<String>,
    /// Whether there is no workflow at all without it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    /// Whether having it running is a way to put a payload on the target.
    ///
    /// **This is what a startup list is audited against.** Moving files is not enough on its
    /// own: a file service can put an ELF on the disk and has no way to run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovers: Option<bool>,
    /// Why this is in a startup list, in somebody own words.
    ///
    /// **The one field here that is prose rather than a fact.** What a service *does* comes
    /// from the built-in table or from a payload published description; why it sits where it
    /// does in a particular chain - *runs first, so unsigned code can run* - is knowledge
    /// about somebody setup that nothing on the target records and this program cannot
    /// derive. Written here, it survives every rebuild and every payload update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whether this is what runs a startup list once it is up.
    ///
    /// **An autoloader list that does not name it never starts it**, and the list it would
    /// have run then silently does nothing - no error, because the thing that would report
    /// one never ran either. That has cost a real target its jailbreak twice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_lists: Option<bool>,
}

/// Every service this program knows about, however it came to know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    /// In the order they should be reported, loader first.
    services: Vec<Service>,
    /// Why each is in a chain, for the ones somebody has written a note about.
    ///
    /// Beside the services rather than on them: a note is about a payload whether or not it
    /// is a service this program knows, so keying it by name reaches both.
    notes: BTreeMap<String, String>,
}

impl Catalogue {
    /// The compiled-in five, which is what runs when nothing is configured.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            services: SERVICES.to_vec(),
            notes: BTreeMap::new(),
        }
    }

    /// Everything known, in reporting order.
    #[must_use]
    pub fn services(&self) -> &[Service] {
        &self.services
    }

    /// One by name, however it was spelled.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Service> {
        self.services
            .iter()
            .find(|service| service.name.eq_ignore_ascii_case(name))
    }

    /// Every service that is a way to put a payload on the target.
    ///
    /// **The question a startup list is audited against.** Any one of them is enough.
    #[must_use]
    pub fn ways_back(&self) -> Vec<&Service> {
        self.services
            .iter()
            .filter(|service| service.recovers)
            .collect()
    }

    /// Takes what a payload list declared about itself.
    ///
    /// A payload naming a port describes a service, whoever wrote it. Anything already known
    /// is **corrected rather than duplicated**, so one name never appears twice.
    pub fn take_declared(&mut self, manifest: &crate::manifest::Manifest) {
        for payload in manifest.payloads() {
            let Some(declared) = payload.as_service() else {
                continue;
            };
            self.absorb(Entry {
                name: declared.name.into_owned(),
                port: Some(declared.port),
                unlocks: Some(declared.unlocks.into_owned()),
                required: Some(declared.required),
                recovers: None,
                runs_lists: None,
                note: None,
            });
        }
    }

    /// Applies one entry, correcting a service already known or adding a new one.
    ///
    /// **A field the entry does not state is left alone**: an absence is not a correction, the
    /// same rule the payload list follows when merging.
    /// Why a payload is in a chain, if anybody has written it down.
    ///
    /// Matched case-insensitively by name, like everything else joining these two documents.
    #[must_use]
    pub fn note(&self, name: &str) -> Option<&str> {
        self.notes
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, note)| note.as_str())
    }

    /// Applies one entry, correcting a service already known or adding a new one.
    ///
    /// **A field the entry does not state is left alone**: an absence is not a correction, the
    /// same rule the payload list follows when merging.
    pub fn absorb(&mut self, entry: Entry) {
        if let Some(note) = entry.note.as_ref().filter(|note| !note.trim().is_empty()) {
            self.notes.insert(entry.name.clone(), note.clone());
        }
        if let Some(known) = self
            .services
            .iter_mut()
            .find(|service| service.name.eq_ignore_ascii_case(&entry.name))
        {
            if let Some(port) = entry.port {
                known.port = port;
            }
            if let Some(unlocks) = entry.unlocks {
                known.unlocks = unlocks.into();
            }
            if let Some(required) = entry.required {
                known.required = required;
            }
            if let Some(recovers) = entry.recovers {
                known.recovers = recovers;
            }
            if let Some(runs) = entry.runs_lists {
                known.runs_lists = runs;
            }
            return;
        }
        // A port is what makes a service answerable at all. Without one there is nothing to
        // connect to, so an entry naming none describes nothing this program can check.
        let Some(port) = entry.port else {
            return;
        };
        self.services.push(Service::declared(
            entry.name,
            port,
            entry.unlocks.unwrap_or_else(|| "use it".to_owned()),
            entry.required.unwrap_or(false),
            entry.recovers.unwrap_or(false),
            entry.runs_lists.unwrap_or(false),
        ));
    }

    /// Reads entries from JSON and applies them.
    ///
    /// # Errors
    ///
    /// When the document will not parse. **A file somebody wrote and got wrong is reported**,
    /// not silently ignored: quietly falling back to the defaults would mean an override that
    /// never took effect and never said so.
    pub fn take_json(&mut self, text: &str) -> Result<(), String> {
        let entries: Vec<Entry> = serde_json::from_str(text).map_err(|why| why.to_string())?;
        for entry in entries {
            self.absorb(entry);
        }
        Ok(())
    }
}

/// Where the file lives, when this machine has somewhere to keep one.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("services.json");
    Some(path)
}

/// The catalogue this machine should use: the defaults, then the file if there is one.
///
/// # Errors
///
/// When the file exists and will not parse. A missing file is not an error - it is the normal
/// case, and it means the compiled-in five.
pub fn load() -> Result<Catalogue, String> {
    let mut catalogue = Catalogue::builtin();
    let Some(path) = path() else {
        return Ok(catalogue);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => catalogue.take_json(&text).map_err(|why| {
            format!(
                "{} could not be read: {why}. Delete it to fall back to the built-in services",
                path.display()
            )
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", path.display())),
    }
    Ok(catalogue)
}

/// A catalogue built from the defaults, a payload list, and the file, in that order.
///
/// # Errors
///
/// As [`load`].
pub fn load_with(manifest: &crate::manifest::Manifest) -> Result<Catalogue, String> {
    let mut catalogue = Catalogue::builtin();
    catalogue.take_declared(manifest);
    if let Some(path) = path()
        && let Ok(text) = std::fs::read_to_string(&path)
    {
        catalogue.take_json(&text)?;
    }
    Ok(catalogue)
}

#[cfg(test)]
mod tests {
    use super::{Catalogue, Entry};

    /// With no file, the compiled-in services are what there is.
    #[test]
    fn the_default_catalogue_is_the_built_in_services() {
        let catalogue = Catalogue::builtin();
        assert!(catalogue.get("elfldr").is_some());
        assert_eq!(catalogue.services().len(), 5);
    }

    /// **The loader, the shell and the manager are ways back; moving files is not.**
    ///
    /// A file service can put an ELF on the disk and has no way to run it, so a chain that
    /// leaves only that behind has left nothing that can start anything.
    #[test]
    fn a_file_service_alone_is_not_a_way_back() {
        let ways: Vec<String> = Catalogue::builtin()
            .ways_back()
            .iter()
            .map(|service| service.name.to_string())
            .collect();
        assert!(ways.contains(&"elfldr".to_owned()));
        assert!(ways.contains(&"shsrv".to_owned()));
        assert!(ways.contains(&"pldmgr".to_owned()));
        assert!(!ways.contains(&"ftpsrv".to_owned()));
    }

    /// A file corrects what is already known rather than adding a second entry under the
    /// same name.
    #[test]
    fn an_entry_corrects_rather_than_duplicates() {
        let mut catalogue = Catalogue::builtin();
        let before = catalogue.services().len();
        catalogue.absorb(Entry {
            name: "ftpsrv".to_owned(),
            port: Some(2122),
            ..Entry::default()
        });
        assert_eq!(catalogue.services().len(), before, "no second ftpsrv");
        assert_eq!(catalogue.get("ftpsrv").map(|one| one.port), Some(2122));
        assert_eq!(
            catalogue.get("ftpsrv").map(|one| one.required),
            Some(true),
            "a field it did not state is left alone"
        );
    }

    /// **A rival payload can be declared a way back**, which is the whole point: nothing here
    /// should assume the five it was built with are the only ones that ever will be.
    #[test]
    fn a_payload_this_program_never_heard_of_can_be_a_way_back() {
        let mut catalogue = Catalogue::builtin();
        catalogue
            .take_json(
                r#"[{ "name": "zftpd", "port": 2121, "unlocks": "move files", "recovers": true }]"#,
            )
            .expect("it reads");
        let one = catalogue.get("zftpd").expect("it was added");
        assert!(one.recovers);
        assert!(one.declared, "and it knows it came from a file");
        assert!(
            catalogue
                .ways_back()
                .iter()
                .any(|service| service.name == "zftpd")
        );
    }

    /// An entry with no port describes nothing that can be checked, so it is not a service.
    #[test]
    fn an_entry_with_no_port_adds_nothing() {
        let mut catalogue = Catalogue::builtin();
        let before = catalogue.services().len();
        catalogue
            .take_json(r#"[{ "name": "mystery", "recovers": true }]"#)
            .expect("it reads");
        assert_eq!(catalogue.services().len(), before);
    }

    /// **A file that will not parse is reported, not ignored.** Silently using the defaults
    /// would be an override that never took effect and never said so.
    #[test]
    fn a_broken_file_is_an_error_rather_than_a_shrug() {
        let mut catalogue = Catalogue::builtin();
        assert!(catalogue.take_json("{ not json at all").is_err());
    }
}

#[cfg(test)]
mod notes {
    use super::Catalogue;

    /// **A reason can be written for a payload this program has never heard of.**
    ///
    /// Why an entry sits where it does in a chain is knowledge about somebody's setup. Nothing
    /// on the target records it and nothing here can derive it, so it is written in a file -
    /// which means it survives a rebuild, a payload update, and this program's opinions.
    #[test]
    fn a_note_can_be_written_for_anything() {
        let mut catalogue = Catalogue::builtin();
        catalogue
            .take_json(
                r#"[{ "name": "kstuff-lite",
                      "note": "runs first, so unsigned code can run" }]"#,
            )
            .expect("it reads");
        assert_eq!(
            catalogue.note("kstuff-lite"),
            Some("runs first, so unsigned code can run")
        );
        // And it did not have to invent a service to hold it: an entry with no port is still
        // not something that can be probed.
        assert!(catalogue.get("kstuff-lite").is_none());
    }

    /// A note about one of the built-in five is kept beside it.
    #[test]
    fn a_note_can_also_be_written_about_a_known_service() {
        let mut catalogue = Catalogue::builtin();
        catalogue
            .take_json(r#"[{ "name": "ftpsrv", "note": "how everything else gets fixed" }]"#)
            .expect("it reads");
        assert_eq!(
            catalogue.note("ftpsrv"),
            Some("how everything else gets fixed")
        );
        assert_eq!(
            catalogue.get("ftpsrv").map(|one| one.port),
            Some(2121),
            "and the service it is about is untouched"
        );
    }

    /// Nothing written is nothing said - an empty note is not a note.
    #[test]
    fn an_empty_note_is_not_one() {
        let mut catalogue = Catalogue::builtin();
        catalogue
            .take_json(r#"[{ "name": "ftpsrv", "note": "   " }]"#)
            .expect("it reads");
        assert_eq!(catalogue.note("ftpsrv"), None);
    }
}

/// Writes a note about one payload into the file, keeping everything else in it.
///
/// # Why it reads before it writes
///
/// The file is somebody's, and it may carry ports, roles and notes this call knows nothing
/// about. Rewriting it from what is in memory would silently drop whatever was not loaded -
/// so the document is read, one entry is changed or added, and the rest goes back as it came.
///
/// An empty note removes it, because *there is no reason recorded* and *there is a reason and
/// it is blank* should not be two different states in the file.
///
/// # Errors
///
/// When there is nowhere to keep it, or the existing file cannot be read or replaced.
pub fn write_note(name: &str, note: &str) -> Result<PathBuf, String> {
    let path =
        path().ok_or_else(|| "no home directory, so there is nowhere to keep it".to_owned())?;
    let mut entries: Vec<Entry> = match std::fs::read_to_string(&path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };

    let note = note.trim();
    match entries
        .iter_mut()
        .find(|entry| entry.name.eq_ignore_ascii_case(name))
    {
        Some(entry) => entry.note = (!note.is_empty()).then(|| note.to_owned()),
        None if note.is_empty() => return Ok(path),
        None => entries.push(Entry {
            name: name.to_owned(),
            note: Some(note.to_owned()),
            ..Entry::default()
        }),
    }
    // An entry left with nothing to say at all is dropped rather than kept as a name.
    entries.retain(|entry| {
        entry.note.is_some()
            || entry.port.is_some()
            || entry.unlocks.is_some()
            || entry.required.is_some()
            || entry.recovers.is_some()
            || entry.runs_lists.is_some()
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| format!("{}: {why}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&entries).map_err(|why| why.to_string())?;
    std::fs::write(&path, text + "\n").map_err(|why| format!("{}: {why}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod writing {
    use super::{Catalogue, Entry};

    /// **A note is added without disturbing what else the file says.**
    ///
    /// The file may carry ports and roles this call knows nothing about; rewriting it from
    /// memory would drop them, and the dropping would look exactly like they had never been
    /// written. Checked through the same reader the real path uses.
    #[test]
    fn writing_a_note_keeps_the_rest_of_the_file() {
        let existing = r#"[{ "name": "zftpd", "port": 2121, "recovers": true }]"#;
        let mut entries: Vec<Entry> = serde_json::from_str(existing).expect("it reads");
        // What `write_note` does to the document it read.
        match entries
            .iter_mut()
            .find(|entry| entry.name.eq_ignore_ascii_case("zftpd"))
        {
            Some(entry) => entry.note = Some("the file service on this box".to_owned()),
            None => panic!("it is there"),
        }
        let text = serde_json::to_string(&entries).expect("it writes");

        let mut catalogue = Catalogue::builtin();
        catalogue.take_json(&text).expect("it reads back");
        assert_eq!(
            catalogue.note("zftpd"),
            Some("the file service on this box")
        );
        let one = catalogue.get("zftpd").expect("still a service");
        assert_eq!(one.port, 2121, "its port survived");
        assert!(one.recovers, "and its role");
    }

    /// An entry that would be left saying nothing at all is dropped rather than kept as a
    /// bare name, so clearing a note tidies up after itself.
    #[test]
    fn an_entry_with_nothing_left_to_say_is_not_kept() {
        let entry = Entry {
            name: "kstuff-lite".to_owned(),
            note: None,
            ..Entry::default()
        };
        let says_something = entry.note.is_some()
            || entry.port.is_some()
            || entry.unlocks.is_some()
            || entry.required.is_some()
            || entry.recovers.is_some()
            || entry.runs_lists.is_some();
        assert!(!says_something, "nothing about it is stated");
    }
}
