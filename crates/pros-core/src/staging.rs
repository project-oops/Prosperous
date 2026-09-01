//! Payloads kept here, ready to be sent.
//!
//! # Why this exists before any way of fetching does
//!
//! This project ships no payload binaries and cannot yet download one, because reaching a
//! public mirror needs a security layer that has not been argued for. That leaves an obvious
//! gap and a much less obvious fact: **a person who already has the file needs nothing from
//! that decision at all.** They downloaded it themselves, from the project that publishes
//! it, which is where the manifest was going to point anyway.
//!
//! So staging is the whole of the workflow that can exist today - put the file here, have it
//! checked against what the manifest says it should be, send it. Fetching, when it arrives,
//! becomes a way of filling this directory rather than a new path through the program.
//!
//! # Nothing arrives here unverified
//!
//! A payload is about to be run with kernel-adjacent privileges. **The digest is checked on
//! the way in**, not on the way out, so that everything in this directory is already known
//! to be what it claims - and a file somebody dropped in by hand is not.

use std::path::{Path, PathBuf};

use crate::checksum::Mismatch;
use crate::manifest::{Payload, staging};

/// Where a payload would be if it were staged.
///
/// `None` when the entry names no file, which is a description somebody has not finished
/// rather than a payload that is missing.
#[must_use]
pub fn path_for(payload: &Payload) -> Option<PathBuf> {
    let mut path = staging()?;
    path.push(payload.filename.as_ref()?);
    Some(path)
}

/// Whether this payload is here already.
#[must_use]
pub fn is_staged(payload: &Payload) -> bool {
    path_for(payload).is_some_and(|path| path.exists())
}

/// Copies of this payload that are here under some **other** version's filename.
///
/// # Why this is not the same question as [`is_staged`]
///
/// A staged file is found by the filename the description gives, and those filenames carry
/// versions - `elfldr_v0.24.elf`, `elfldr_v0.25.elf`. So when a list moves on, the copy fetched
/// last month stops being found by anything: [`is_staged`] says no, the size column says `-`,
/// and a whole payload that is sitting on the disk reads as one nobody has.
///
/// **The difference decides what the button should say.** Getting a file for the first time and
/// replacing an older one with a newer one are not the same act, and a control that calls both
/// of them *download* is describing only the first.
#[must_use]
pub fn older_here(payload: &Payload) -> Vec<PathBuf> {
    let Some(dir) = staging() else {
        return Vec::new();
    };
    let wanted = payload.filename.as_deref().unwrap_or(&payload.name);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            // Not the described file itself - that one is `is_staged`'s answer, not this one.
            if name.eq_ignore_ascii_case(wanted) {
                return false;
            }
            // The same matching every other part of this project uses for *is this that
            // payload*, so a second rule here cannot disagree with the startup list's.
            crate::chain::Chain::parse(name)
                .position(&payload.name)
                .is_some()
        })
        .collect()
}

/// Copies a file in, having checked it is the one described.
///
/// # Errors
///
/// [`NotStaged::Unverifiable`] when the manifest states no digest this can check - the file
/// is **not** copied, because a payload nobody can verify is one that should not be sitting
/// in a directory whose whole promise is that everything in it was checked.
///
/// [`NotStaged::Mismatched`] when it is the wrong file, carrying both digests.
/// Copies a checked file into a directory the caller names.
///
/// # Why a caller gets to say where
///
/// [`accept`] puts things in one directory whose whole promise is that everything in it was
/// verified. That is right for payloads, which are sent by name from one place.
///
/// It is wrong for a window that shows a folder and offers to fill it. **A download that
/// lands somewhere other than the folder the pane names is a download that appears not to
/// have happened** - the file is on disk, verified, and invisible where somebody is looking
/// for it. The verification is identical; only the destination differs.
///
/// # Errors
///
/// The same as [`accept`], and nothing is written unless the digest matched.
pub fn accept_into(payload: &Payload, from: &Path, dir: &Path) -> Result<PathBuf, NotStaged> {
    let expected = payload.checksum().map_err(|why| NotStaged::Unverifiable {
        why: why.to_string(),
    })?;
    let bytes = std::fs::read(from).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    expected.verify(&bytes).map_err(NotStaged::Mismatched)?;

    let name = payload
        .filename
        .clone()
        .unwrap_or_else(|| payload.name.clone());
    std::fs::create_dir_all(dir).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    let into = dir.join(name);
    std::fs::write(&into, &bytes).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    Ok(into)
}

/// Copies a file in, having checked it is the one described.
///
/// # Errors
///
/// [`NotStaged::Unverifiable`] when the manifest states no digest this can check - the file
/// is **not** copied, because a payload nobody can verify is one that should not be sitting
/// in a directory whose whole promise is that everything in it was checked.
///
/// [`NotStaged::Mismatched`] when it is the wrong file, carrying both digests.
pub fn accept(payload: &Payload, from: &Path) -> Result<PathBuf, NotStaged> {
    let expected = payload.checksum().map_err(|why| NotStaged::Unverifiable {
        why: why.to_string(),
    })?;
    let bytes = std::fs::read(from).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    expected.verify(&bytes).map_err(NotStaged::Mismatched)?;

    let into = path_for(payload).ok_or(NotStaged::Nowhere)?;
    if let Some(parent) = into.parent() {
        std::fs::create_dir_all(parent).map_err(|why| NotStaged::Unreadable {
            why: why.to_string(),
        })?;
    }
    std::fs::write(&into, &bytes).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    Ok(into)
}

/// Why a file was not staged.
#[derive(Debug)]
pub enum NotStaged {
    /// The manifest states no digest this can check.
    Unverifiable {
        /// What the manifest said, in the checksum module's words.
        why: String,
    },
    /// It is not the file the manifest describes.
    Mismatched(Mismatch),
    /// The file could not be read, or the directory could not be written.
    Unreadable {
        /// What the system said.
        why: String,
    },
    /// There is nowhere to put it - no home directory, or the entry names no file.
    Nowhere,
}

impl std::fmt::Display for NotStaged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unverifiable { why } => write!(
                f,
                "not staged, because nothing could be established about it: {why}"
            ),
            Self::Mismatched(mismatch) => write!(f, "not staged: {mismatch}"),
            Self::Unreadable { why } => write!(f, "not staged: {why}"),
            Self::Nowhere => write!(
                f,
                "nowhere to put it - no home directory, or the manifest entry names no file"
            ),
        }
    }
}

impl std::error::Error for NotStaged {}

#[cfg(test)]
mod tests {
    use super::{NotStaged, accept, accept_into};
    use crate::manifest::Payload;

    /// **A file lands in the directory the caller named, under the name the entry gives it.**
    ///
    /// Worth pinning because the failure is silent: the download succeeds, the digest matches,
    /// the file is written - somewhere else. Nothing errors, and the pane that offered the
    /// download goes on offering it, because the folder it lists is still empty.
    #[test]
    fn a_download_lands_where_the_caller_said() {
        let dir = std::env::temp_dir().join("prosperous-accept-into");
        let _ = std::fs::remove_dir_all(&dir);
        let from = std::env::temp_dir().join("prosperous-accept-into-source");
        std::fs::write(&from, b"hello").expect("writes");

        let payload = Payload {
            name: "greeting".to_owned(),
            filename: Some("greeting.bin".to_owned()),
            // The digest of "hello".
            checksum: Some(
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_owned(),
            ),
            ..Payload::default()
        };

        let into = accept_into(&payload, &from, &dir).expect("it is the file described");
        assert_eq!(into, dir.join("greeting.bin"), "it went somewhere else");
        assert_eq!(std::fs::read(&into).expect("readable"), b"hello");

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&from);
    }

    /// And the wrong file is refused there too, leaving the directory empty.
    ///
    /// The check is the same check; only the destination differs. A version of this that
    /// verified less because the caller chose the folder would be worse than no check at all,
    /// because the folder is where somebody goes looking for things they trust.
    #[test]
    fn the_wrong_file_is_not_written_to_the_named_directory_either() {
        let dir = std::env::temp_dir().join("prosperous-accept-into-wrong");
        let _ = std::fs::remove_dir_all(&dir);
        let from = std::env::temp_dir().join("prosperous-accept-into-wrong-source");
        std::fs::write(&from, b"not hello").expect("writes");

        let payload = Payload {
            name: "greeting".to_owned(),
            filename: Some("greeting.bin".to_owned()),
            checksum: Some(
                "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".to_owned(),
            ),
            ..Payload::default()
        };

        let refused = accept_into(&payload, &from, &dir);
        assert!(matches!(refused, Err(NotStaged::Mismatched(_))));
        assert!(
            !dir.join("greeting.bin").exists(),
            "a file that failed its digest was written anyway"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&from);
    }

    /// The wrong file is refused, and both digests are said so a person can tell which of
    /// the two is wrong - the download or the description.
    #[test]
    fn the_wrong_file_is_not_staged() {
        let payload = Payload {
            name: "elfldr".to_owned(),
            filename: Some("elfldr.elf".to_owned()),
            checksum: Some(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
            ),
            ..Payload::default()
        };
        let scratch = std::env::temp_dir().join(format!("pros-stage-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let wrong = scratch.join("wrong.elf");
        std::fs::write(&wrong, b"not the payload that was described").expect("written");

        match accept(&payload, &wrong) {
            Err(NotStaged::Mismatched(mismatch)) => {
                assert!(!mismatch.expected.is_empty());
                assert_ne!(mismatch.expected, mismatch.found);
            }
            other => panic!("the wrong file was accepted: {other:?}"),
        }
    }

    /// **An entry nobody can verify does not get a file staged for it.**
    ///
    /// The promise of this directory is that everything in it was checked. A payload with a
    /// digest in an algorithm this cannot read would break that promise quietly, which is
    /// worse than refusing loudly.
    #[test]
    fn a_payload_that_cannot_be_verified_is_not_staged_at_all() {
        let payload = Payload {
            name: "old".to_owned(),
            filename: Some("old.elf".to_owned()),
            checksum: Some("d41d8cd98f00b204e9800998ecf8427e".to_owned()),
            ..Payload::default()
        };
        let scratch = std::env::temp_dir().join(format!("pros-stage-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let any = scratch.join("any.elf");
        std::fs::write(&any, b"anything").expect("written");

        assert!(
            matches!(accept(&payload, &any), Err(NotStaged::Unverifiable { .. })),
            "a payload with an uncheckable digest was staged"
        );
    }

    /// An entry with no file name has nowhere to go, and that is a description somebody has
    /// not finished rather than a failure of the file.
    #[test]
    fn an_entry_with_no_filename_has_nowhere_to_go() {
        let payload = Payload {
            name: "nameless".to_owned(),
            checksum: Some(
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_owned(),
            ),
            ..Payload::default()
        };
        let scratch = std::env::temp_dir().join(format!("pros-stage-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).expect("a scratch directory");
        let file = scratch.join("some.elf");
        // The bytes that match the digest above, so the refusal is about the name and not
        // about the contents.
        std::fs::write(&file, b"abc").expect("written");

        assert!(matches!(accept(&payload, &file), Err(NotStaged::Nowhere)));
    }
}
