//! What services exist, what they unlock, and which of them is a way back in.
//!
//! `manifest` holds facts about a payload in a payload manager's format; the role a service
//! plays for this program lives here instead, in a file this project owns, joined to the
//! manifest by payload name.
//!
//! The compiled-in services are the default and the file is optional; it overrides and
//! extends. Later wins: the compiled-in services, then what a payload list declares for
//! itself, then this file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pros_link::service::{SERVICES, Service};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What a file may say about one service.
///
/// Every field is optional, so an entry states only what it corrects.
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
    /// A startup list is audited against this. A file service alone does not count: it can
    /// put an ELF on the disk and cannot run it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovers: Option<bool>,
    /// Why this is in a startup list, in the owner's own words.
    ///
    /// Knowledge about one setup that nothing on the target records, so it is kept here
    /// where it survives rebuilds and payload updates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Whether this is what runs a startup list once it is up.
    ///
    /// An autoloader list that does not name it never starts it, and the list then does
    /// nothing with no error reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_lists: Option<bool>,
}

/// Every service this program knows about, however it came to know.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    /// In the order they should be reported, loader first.
    services: Vec<Service>,
    /// Why each is in a chain, keyed by payload name so a note can describe a payload that is
    /// not a known service.
    notes: BTreeMap<String, String>,
}

impl Catalogue {
    /// The compiled-in services, used when nothing is configured.
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

    /// Every service that is a way to put a payload on the target. A startup list needs any
    /// one of them.
    #[must_use]
    pub fn ways_back(&self) -> Vec<&Service> {
        self.services
            .iter()
            .filter(|service| service.recovers)
            .collect()
    }

    /// Takes what a payload list declared about itself.
    ///
    /// A payload naming a port describes a service. Anything already known is corrected
    /// rather than duplicated, so one name never appears twice.
    pub fn take_declared(&mut self, manifest: &crate::manifest::Manifest) {
        for payload in manifest.payloads() {
            if let Some(desc) = payload.unlocks.as_ref().or(payload.description.as_ref())
                && !desc.trim().is_empty()
            {
                self.notes
                    .entry(payload.name.clone())
                    .or_insert_with(|| desc.clone());
            }
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

    /// Why a payload is in a chain, if a note was written. Matched case-insensitively by name.
    #[must_use]
    pub fn note(&self, name: &str) -> Option<&str> {
        self.notes
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, note)| note.as_str())
    }

    /// Applies one entry, correcting a service already known or adding a new one.
    ///
    /// A field the entry does not state is left alone: an absence is not a correction, as in
    /// the payload list's merge.
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
        // Without a port there is nothing to connect to, so the entry is not a service.
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
    /// When the document will not parse. A broken override is reported rather than silently
    /// replaced by the defaults.
    pub fn take_json(&mut self, text: &str) -> Result<()> {
        let entries: Vec<Entry> =
            serde_json::from_str(text).map_err(|why| Error::failed(why.to_string()))?;
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
/// When the file exists and will not parse. A missing file is not an error; it means the
/// compiled-in services.
pub fn load() -> Result<Catalogue> {
    let mut catalogue = Catalogue::builtin();
    let Some(path) = path() else {
        return Ok(catalogue);
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => catalogue.take_json(&text).map_err(|why| {
            Error::failed(format!(
                "{} could not be read: {why}. Delete it to fall back to the built-in services",
                path.display()
            ))
        })?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(Error::at(&path, error)),
    }
    Ok(catalogue)
}

/// A catalogue built from the defaults, a payload list, and the file, in that order.
///
/// # Errors
///
/// As [`load`].
pub fn load_with(manifest: &crate::manifest::Manifest) -> Result<Catalogue> {
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

    /// The loader, the shell and the manager are ways back; the file service is not.
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

    /// A file corrects a known service rather than adding a second one of the same name.
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

    /// A payload not compiled in can be declared a way back.
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

    /// A file that will not parse is reported, not ignored.
    #[test]
    fn a_broken_file_is_an_error_rather_than_a_shrug() {
        let mut catalogue = Catalogue::builtin();
        assert!(catalogue.take_json("{ not json at all").is_err());
    }
}

#[cfg(test)]
mod notes {
    use super::Catalogue;

    /// A note can be written for a payload that is not a known service.
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
        // Holding the note does not make it a service.
        assert!(catalogue.get("kstuff-lite").is_none());
    }

    /// A note about a built-in service is kept beside it.
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

    /// An empty note is not a note.
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
/// The file is read, one entry changed or added, and the rest written back as it came, so
/// entries this call does not know about survive. An empty note removes the note.
///
/// # Errors
///
/// When there is nowhere to keep it, or the existing file cannot be read or replaced.
pub fn write_note(name: &str, note: &str) -> Result<PathBuf> {
    let path =
        path().ok_or_else(|| Error::failed("no home directory, so there is nowhere to keep it"))?;
    let mut entries: Vec<Entry> = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|why| Error::at(&path, why))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(Error::at(&path, error)),
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
    // An entry left with nothing but a name is dropped.
    entries.retain(|entry| {
        entry.note.is_some()
            || entry.port.is_some()
            || entry.unlocks.is_some()
            || entry.required.is_some()
            || entry.recovers.is_some()
            || entry.runs_lists.is_some()
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| Error::at(parent, why))?;
    }
    let text =
        serde_json::to_string_pretty(&entries).map_err(|why| Error::failed(why.to_string()))?;
    std::fs::write(&path, text + "\n").map_err(|why| Error::at(&path, why))?;
    Ok(path)
}

#[cfg(test)]
mod writing {
    use super::{Catalogue, Entry};

    /// A note is added without disturbing the rest of the entry it lands on.
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

    /// An entry with nothing but a name states nothing.
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
