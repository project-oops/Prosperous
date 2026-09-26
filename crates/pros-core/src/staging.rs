//! Payloads kept here, ready to be sent.
//!
//! Staging is the path from a file a person already has, or one [`crate::fetch`] downloaded,
//! to a send: the file is checked against the manifest's digest on the way in, so everything
//! in the staging directory is already known to be what it claims. A file dropped in by hand
//! has not been checked.

use std::path::{Path, PathBuf};

use crate::checksum::Mismatch;
use crate::manifest::{Payload, staging};

/// Where a payload would be if it were staged.
///
/// `None` when the entry names no file, which is an unfinished description rather than a
/// missing payload.
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

/// Copies of this payload that are here under some other version's filename.
///
/// Filenames carry versions (`elfldr_v0.24.elf`, `elfldr_v0.25.elf`), so when a manifest moves
/// on, [`is_staged`] no longer finds the older copy. This finds it, so a caller can offer to
/// replace an older copy rather than to download for the first time.
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
            // The described file itself is `is_staged`'s answer.
            if name.eq_ignore_ascii_case(wanted) {
                return false;
            }
            // The startup list's own matching, so the two cannot disagree about which payload
            // a file is.
            crate::chain::Chain::parse(name)
                .position(&payload.name)
                .is_some()
        })
        .collect()
}

/// Copies a checked file into a directory the caller names.
///
/// For a window that shows a folder and offers to fill it: the file must land in the folder
/// being shown. The verification is the same as [`accept`]'s.
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
/// [`NotStaged::Unverifiable`] when the manifest states no digest this can check; the file is
/// not copied, because everything in the staging directory is checked.
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

/// Copies a local build into staging or a named directory.
///
/// A matching manifest checksum verifies it. A differing or absent checksum is accepted with
/// a warning, as a development build just compiled on this machine.
///
/// # Errors
///
/// Returns [`NotStaged`] if the source cannot be read, the destination directory cannot be
/// created, or the file cannot be written.
pub fn accept_local_into(
    payload: &Payload,
    from: &Path,
    dir: Option<&Path>,
) -> Result<PathBuf, NotStaged> {
    let bytes = std::fs::read(from).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    if let Ok(expected) = payload.checksum() {
        if let Err(mismatch) = expected.verify(&bytes) {
            tracing::warn!(
                payload = %payload.name,
                %mismatch,
                "local build checksum differs from manifest; accepting as local development build"
            );
        } else {
            tracing::info!(payload = %payload.name, "local build verified against manifest digest");
        }
    } else {
        tracing::info!(
            payload = %payload.name,
            "staging local build without manifest checksum"
        );
    }
    let name = payload
        .filename
        .clone()
        .unwrap_or_else(|| payload.name.clone());
    let into = if let Some(dir) = dir {
        std::fs::create_dir_all(dir).map_err(|why| NotStaged::Unreadable {
            why: why.to_string(),
        })?;
        dir.join(name)
    } else {
        let p = path_for(payload).ok_or(NotStaged::Nowhere)?;
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|why| NotStaged::Unreadable {
                why: why.to_string(),
            })?;
        }
        p
    };
    std::fs::write(&into, &bytes).map_err(|why| NotStaged::Unreadable {
        why: why.to_string(),
    })?;
    Ok(into)
}

/// The same, into the default staging directory.
///
/// # Errors
///
/// Returns [`NotStaged`] for the same reasons as [`accept_local_into`].
pub fn accept_local(payload: &Payload, from: &Path) -> Result<PathBuf, NotStaged> {
    accept_local_into(payload, from, None)
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

    /// A file lands in the directory the caller named, under the entry's filename.
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

    /// The wrong file is refused for a named directory too, leaving it empty.
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

    /// The wrong file is refused with both digests.
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

    /// An entry whose digest cannot be checked gets nothing staged.
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

    /// An entry with no filename has nowhere to go.
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
        // Bytes matching the digest above, so the refusal is about the name.
        std::fs::write(&file, b"abc").expect("written");

        assert!(matches!(accept(&payload, &file), Err(NotStaged::Nowhere)));
    }
}
