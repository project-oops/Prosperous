//! Putting one save's contents into another save's container.
//!
//! # The problem this solves
//!
//! A save made for one edition of a game will not load under another. Astro Bot is
//! `PPSA21564` in America and `PPSA21567` in Europe; Grand Theft Auto V is `PPSA03420`
//! digital in America, `PPSA01721` digital in Europe and `PPSA04263` on a European disc. Four
//! containers, one game, and a save from any of them is inert under the others.
//!
//! # Why the obvious fix does not work, and what does
//!
//! The obvious fix is to edit the title identifier and be done. It fails on the **keystone**:
//! ninety-six bytes beside every save, of which thirty-two are
//! `HMAC-SHA256(key, package_passcode)`. The passcode belongs to the edition, so every edition
//! has a different keystone, and nothing here can compute one - the key is Sony's.
//!
//! What *is* known, from two independent open implementations, is what the keystone does
//! **not** cover: not the save contents, not the parameter file, not the title identifier, not
//! the account. It is a static per-edition value. That is why a save tool can ship a database
//! of them, and it is why the answer is not to forge one but to **keep one you already have.**
//!
//! So: start from a save made by *your own copy* of the game, and replace only the parts that
//! are the game's own data. The container stays yours - keystone, parameter file, everything
//! under `sce_sys` - and the contents come from elsewhere.
//!
//! ```text
//! yours/                     theirs/                    result/
//!   sce_sys/keystone    <-- kept                          sce_sys/keystone    (yours)
//!   sce_sys/param.sfo   <-- kept                          sce_sys/param.sfo   (yours)
//!   memory.dat                    memory.dat  --> taken   memory.dat          (theirs)
//! ```
//!
//! # What this does not do
//!
//! **It does not touch encryption.** A save on a target is an opaque `sdimg_` container, and
//! getting one open or closed is a payload's job. This works on saves that are already open -
//! which is the form they arrive in when somebody shares one, and the form a save manager
//! hands back.
//!
//! **It does not promise the game will accept the result.** The cryptography does not stand in
//! the way; a game checking its own build or region internally still might. That is a fact
//! about each game and nothing here can answer it in advance.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::sfo;

/// Where a save keeps everything that describes the container rather than the game's data.
pub const SYSTEM: &str = "sce_sys";

/// The parameter file inside it.
pub const PARAMS: &str = "sce_sys/param.sfo";

/// The per-edition blob this whole design is arranged around.
pub const KEYSTONE: &str = "sce_sys/keystone";

/// A save that is already open, as a folder of files.
#[derive(Debug, Clone)]
pub struct Open {
    /// Where it is.
    pub root: PathBuf,
    /// What its parameter file says, when it has one.
    pub params: BTreeMap<String, sfo::Value>,
    /// Everything that is not under `sce_sys` - the game's own data.
    pub contents: Vec<PathBuf>,
    /// Whether a keystone is present.
    ///
    /// **Presence, not contents.** What is in it is Sony's business; whether it is there
    /// decides whether this is a container a game will mount.
    pub has_keystone: bool,
}

impl Open {
    /// Reads a save folder.
    ///
    /// # Errors
    ///
    /// When the folder cannot be walked. **A missing parameter file is not an error** - a save
    /// exported without one is still a folder of contents, and saying so is more useful than
    /// refusing to look at it.
    pub fn read(root: &Path) -> Result<Self, String> {
        let mut contents = Vec::new();
        walk(root, root, &mut contents)?;
        contents.sort();

        let params = std::fs::read(root.join(PARAMS))
            .ok()
            .and_then(|bytes| sfo::read(&bytes).ok())
            .unwrap_or_default();

        Ok(Self {
            root: root.to_owned(),
            params,
            contents,
            has_keystone: root.join(KEYSTONE).is_file(),
        })
    }

    /// The title this save belongs to.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.params.get("TITLE_ID")?.text()
    }

    /// The account it belongs to, as hex.
    #[must_use]
    pub fn account(&self) -> Option<String> {
        sfo::account_id(&self.params)
    }
}

/// Everything a folder holds that is not container description.
fn walk(root: &Path, at: &Path, into: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(at).map_err(|why| format!("{}: {why}", at.display()))? {
        let entry = entry.map_err(|why| why.to_string())?;
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap_or(&path).to_owned();
        // The container's own description, which is the half that stays behind.
        if relative.starts_with(SYSTEM) {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, into)?;
        } else {
            into.push(relative);
        }
    }
    Ok(())
}

/// Something worth saying before a graft, that does not stop it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The two saves are for the same title, so nothing needed retargeting.
    SameTitle(String),
    /// They are for different titles, which is the case this exists for.
    Retargeted {
        /// The container's title - the one the result will be.
        keeping: String,
        /// The contents' title - the one they came from.
        from: String,
    },
    /// The donor has a file the container did not.
    ///
    /// **Copied anyway, and said.** A game that writes a file only sometimes would otherwise
    /// look like a mismatch; a genuine mismatch looks the same and is worth a person's eye.
    Extra(String),
    /// The container had a file the donor does not, so the old one is left in place.
    ///
    /// Left rather than removed: it is the container owner's data, and a donor that simply
    /// never wrote that file should not delete it.
    Kept(String),
    /// The container has no keystone, so it is not one a game will mount.
    NoKeystone,
    /// Neither save named a title, so nothing could be compared.
    NoTitles,
}

impl std::fmt::Display for Note {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SameTitle(id) => write!(out, "both are {id} - no retargeting was needed"),
            Self::Retargeted { keeping, from } => {
                write!(out, "contents from {from} put into a {keeping} container")
            }
            Self::Extra(name) => write!(out, "{name} was not in the container and was added"),
            Self::Kept(name) => write!(out, "{name} was not in the donor and was left as it was"),
            Self::NoKeystone => write!(
                out,
                "the container has no {KEYSTONE} - a game will not mount this"
            ),
            Self::NoTitles => write!(out, "neither save names a title, so nothing was compared"),
        }
    }
}

/// What a graft did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    /// Files taken from the donor.
    pub taken: Vec<String>,
    /// Anything worth reading afterwards.
    pub notes: Vec<Note>,
}

/// Puts the donor's contents into a copy of the container, at `into`.
///
/// The container is copied whole first - **including everything under `sce_sys`** - and then
/// the donor's contents are written over it. Nothing is written to either input.
///
/// # Errors
///
/// When either save cannot be read, or the result cannot be written.
pub fn graft(container: &Open, donor: &Open, into: &Path) -> Result<Done, String> {
    let mut notes = Vec::new();
    match (container.title(), donor.title()) {
        (Some(keeping), Some(from)) if keeping == from => {
            notes.push(Note::SameTitle(keeping.to_owned()));
        }
        (Some(keeping), Some(from)) => notes.push(Note::Retargeted {
            keeping: keeping.to_owned(),
            from: from.to_owned(),
        }),
        _ => notes.push(Note::NoTitles),
    }
    if !container.has_keystone {
        notes.push(Note::NoKeystone);
    }

    // The container first, whole. Its `sce_sys` is the point of it.
    copy_tree(&container.root, into)?;

    let mut taken = Vec::new();
    for relative in &donor.contents {
        let from = donor.root.join(relative);
        let to = into.join(relative);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|why| why.to_string())?;
        }
        let name = relative.to_string_lossy().into_owned();
        if !container.contents.contains(relative) {
            notes.push(Note::Extra(name.clone()));
        }
        std::fs::copy(&from, &to).map_err(|why| format!("{name}: {why}"))?;
        taken.push(name);
    }

    for relative in &container.contents {
        if !donor.contents.contains(relative) {
            notes.push(Note::Kept(relative.to_string_lossy().into_owned()));
        }
    }

    Ok(Done { taken, notes })
}

/// Copies a folder and everything under it.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|why| why.to_string())?;
    for entry in std::fs::read_dir(from).map_err(|why| format!("{}: {why}", from.display()))? {
        let entry = entry.map_err(|why| why.to_string())?;
        let path = entry.path();
        let into = to.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &into)?;
        } else {
            std::fs::copy(&path, &into).map_err(|why| format!("{}: {why}", path.display()))?;
        }
    }
    Ok(())
}

/// Rewrites the account in a save's parameter file, so a target will take it as its own.
///
/// # Why this is here and the encryption is not
///
/// On this platform "resigning" a save is not a signature at all: the account is a field, and
/// changing it is a write. What makes a save the target's is the encryption around it, which
/// the target does when the container is closed - so a tool that rewrites this field and hands
/// the folder back to a save manager has done the whole of its half.
///
/// **A save shared publicly usually has this zeroed.** That is a privacy convention and not a
/// wildcard: an identifier of zero matches no account, and every tool that handles these
/// rewrites it rather than relying on it.
///
/// # Errors
///
/// When the parameter file cannot be read or written, or does not carry an account field.
pub fn set_account(save: &Path, account: &[u8; 8]) -> Result<(), String> {
    let path = save.join(PARAMS);
    let mut bytes = std::fs::read(&path).map_err(|why| format!("{}: {why}", path.display()))?;
    sfo::set(&mut bytes, "ACCOUNT_ID", account, false).map_err(|why| why.to_string())?;
    std::fs::write(&path, &bytes).map_err(|why| format!("{}: {why}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{Note, Open, graft};
    use std::path::Path;

    /// Builds a save folder: a container description and some contents.
    fn save(name: &str, title: Option<&str>, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("prosperous-graft-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("sce_sys")).expect("makes it");
        std::fs::write(root.join(super::KEYSTONE), [name.as_bytes()[0]; 96]).expect("writes");
        if let Some(title) = title {
            std::fs::write(root.join(super::PARAMS), params(title)).expect("writes");
        }
        for (path, bytes) in files {
            let at = root.join(path);
            if let Some(parent) = at.parent() {
                std::fs::create_dir_all(parent).expect("makes it");
            }
            std::fs::write(at, bytes).expect("writes");
        }
        root
    }

    /// A parameter file naming one title.
    fn params(title: &str) -> Vec<u8> {
        let key = b"TITLE_ID\0";
        let mut value = title.as_bytes().to_vec();
        value.push(0);
        let room = u32::try_from(value.len()).expect("small");

        let mut out = Vec::new();
        out.extend_from_slice(b"\0PSF");
        out.extend_from_slice(&0x0101_u32.to_le_bytes());
        out.extend_from_slice(&36_u32.to_le_bytes());
        out.extend_from_slice(&(36 + u32::try_from(key.len()).expect("small")).to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0x0204_u16.to_le_bytes());
        out.extend_from_slice(&room.to_le_bytes());
        out.extend_from_slice(&room.to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&value);
        out
    }

    /// **The container's `sce_sys` survives and the contents are replaced.**
    ///
    /// This is the whole method in one assertion: the keystone that came with the container is
    /// the keystone in the result, because it is the only one that will mount.
    #[test]
    fn the_container_keeps_its_own_description_and_takes_the_others_data() {
        let mine = save("mine", Some("PPSA21564"), &[("memory.dat", b"my progress")]);
        let theirs = save(
            "theirs",
            Some("PPSA21567"),
            &[("memory.dat", b"their progress")],
        );
        let into = std::env::temp_dir().join("prosperous-graft-out");
        let _ = std::fs::remove_dir_all(&into);

        let container = Open::read(&mine).expect("reads");
        let donor = Open::read(&theirs).expect("reads");
        let done = graft(&container, &donor, &into).expect("grafts");

        assert_eq!(
            std::fs::read(into.join("memory.dat")).expect("there"),
            b"their progress",
            "the data should be the donor's"
        );
        assert_eq!(
            std::fs::read(into.join(super::KEYSTONE)).expect("there"),
            [b'm'; 96],
            "the keystone must be the container's - the donor's would not mount"
        );
        assert_eq!(done.taken, ["memory.dat"]);
        assert!(done.notes.contains(&Note::Retargeted {
            keeping: "PPSA21564".to_owned(),
            from: "PPSA21567".to_owned(),
        }));
    }

    /// **Neither input is written to.** A graft that damaged the save somebody started from
    /// would take the one container they had.
    #[test]
    fn the_saves_it_was_given_are_left_alone() {
        let mine = save("keep-mine", Some("PPSA21564"), &[("memory.dat", b"mine")]);
        let theirs = save(
            "keep-theirs",
            Some("PPSA21567"),
            &[("memory.dat", b"theirs")],
        );
        let into = std::env::temp_dir().join("prosperous-graft-untouched");
        let _ = std::fs::remove_dir_all(&into);

        let container = Open::read(&mine).expect("reads");
        let donor = Open::read(&theirs).expect("reads");
        graft(&container, &donor, &into).expect("grafts");

        assert_eq!(
            std::fs::read(mine.join("memory.dat")).expect("there"),
            b"mine"
        );
        assert_eq!(
            std::fs::read(theirs.join("memory.dat")).expect("there"),
            b"theirs"
        );
    }

    /// A file the container did not have is added, and said.
    #[test]
    fn a_file_the_container_never_had_is_taken_and_reported() {
        let mine = save("host-thin", Some("PPSA03420"), &[("memory.dat", b"a")]);
        let theirs = save(
            "donor-fat",
            Some("PPSA01721"),
            &[("memory.dat", b"b"), ("extra/slot1.bin", b"c")],
        );
        let into = std::env::temp_dir().join("prosperous-graft-extra");
        let _ = std::fs::remove_dir_all(&into);

        let done = graft(
            &Open::read(&mine).expect("reads"),
            &Open::read(&theirs).expect("reads"),
            &into,
        )
        .expect("grafts");

        assert!(into.join("extra/slot1.bin").is_file());
        assert!(
            done.notes
                .iter()
                .any(|note| matches!(note, Note::Extra(name) if name.contains("slot1"))),
            "{:?}",
            done.notes
        );
    }

    /// A file the donor does not have is **left**, not deleted - a donor that never wrote it
    /// is not a donor saying to remove it.
    #[test]
    fn a_file_the_donor_lacks_is_left_where_it_was() {
        let mine = save(
            "host-fat",
            Some("PPSA03420"),
            &[("memory.dat", b"a"), ("profile.bin", b"mine")],
        );
        let theirs = save("donor-thin", Some("PPSA03420"), &[("memory.dat", b"b")]);
        let into = std::env::temp_dir().join("prosperous-graft-kept");
        let _ = std::fs::remove_dir_all(&into);

        let done = graft(
            &Open::read(&mine).expect("reads"),
            &Open::read(&theirs).expect("reads"),
            &into,
        )
        .expect("grafts");

        assert_eq!(
            std::fs::read(into.join("profile.bin")).expect("still there"),
            b"mine"
        );
        assert!(
            done.notes
                .iter()
                .any(|note| matches!(note, Note::Kept(name) if name.contains("profile"))),
            "{:?}",
            done.notes
        );
        assert!(
            done.notes
                .contains(&Note::SameTitle("PPSA03420".to_owned()))
        );
    }

    /// **A container with no keystone is called out**, because a game will not mount it and
    /// the result would look like a save that simply refuses to load.
    #[test]
    fn a_container_without_a_keystone_is_not_one_a_game_will_take() {
        let root = std::env::temp_dir().join("prosperous-graft-bare");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("makes it");
        std::fs::write(root.join("memory.dat"), b"a").expect("writes");

        let bare = Open::read(&root).expect("reads");
        assert!(!bare.has_keystone);

        let theirs = save("donor-any", Some("PPSA21567"), &[("memory.dat", b"b")]);
        let into = std::env::temp_dir().join("prosperous-graft-bare-out");
        let _ = std::fs::remove_dir_all(&into);

        let done = graft(&bare, &Open::read(&theirs).expect("reads"), &into).expect("grafts");
        assert!(done.notes.contains(&Note::NoKeystone), "{:?}", done.notes);
    }

    /// Everything under `sce_sys` is container description, whatever it is called.
    #[test]
    fn nothing_under_the_system_folder_counts_as_contents() {
        let mine = save(
            "system-heavy",
            Some("PPSA21564"),
            &[("memory.dat", b"a"), ("sce_sys/icon0.png", b"icon")],
        );
        let open = Open::read(&mine).expect("reads");
        assert_eq!(open.contents, [Path::new("memory.dat")]);
    }
}
