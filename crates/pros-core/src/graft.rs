//! Putting one save's contents into another save's container.
//!
//! A save made for one edition of a game (for example `PPSA21564` versus `PPSA21567`) does not
//! load under another, because of the keystone under `sce_sys`: a static per-edition value
//! keyed by the vendor, not computable here, and covering neither the contents, the parameter
//! file, the title identifier nor the account. So a graft keeps the container's `sce_sys`
//! whole and replaces only the game's own data with the donor's.
//!
//! This works on saves that are already decrypted, as folders; opening and closing the
//! `sdimg_` container is a payload's job. A game may still check its build or region itself.

use std::path::{Path, PathBuf};

use crate::sfo;

/// Where a save keeps everything that describes the container rather than the game's data.
pub const SYSTEM: &str = "sce_sys";

/// The parameter file inside it.
pub const PARAMS: &str = "sce_sys/param.sfo";

/// The per-edition keystone, which a graft keeps from the container.
pub const KEYSTONE: &str = "sce_sys/keystone";

/// A save that is already open, as a folder of files.
#[derive(Debug, Clone)]
pub struct Open {
    /// Where it is.
    pub root: PathBuf,
    /// What its parameter file says, when it has one. Empty when it has none.
    pub params: selfish_title::sfo::Sfo,
    /// Everything that is not under `sce_sys`: the game's own data.
    pub contents: Vec<PathBuf>,
    /// Whether a keystone is present; without one a game will not mount the container.
    pub has_keystone: bool,
}

impl Open {
    /// Reads a save folder.
    ///
    /// # Errors
    ///
    /// When the folder cannot be walked. A missing parameter file is not an error; the
    /// parameters are then empty.
    pub fn read(root: &Path) -> Result<Self, String> {
        let mut contents = Vec::new();
        walk(root, root, &mut contents)?;
        contents.sort();

        let params = std::fs::read(root.join(PARAMS))
            .ok()
            .and_then(|bytes| selfish_title::sfo::Sfo::parse(&bytes).ok())
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
        self.params.text("TITLE_ID")
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
    /// They are for different titles.
    Retargeted {
        /// The container's title, which the result has.
        keeping: String,
        /// The contents' title.
        from: String,
    },
    /// The donor has a file the container did not. Copied, and reported because it may be a
    /// mismatch.
    Extra(String),
    /// The container had a file the donor does not. Left in place: a donor that never wrote a
    /// file does not delete the container's.
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
/// The container is copied whole first, including `sce_sys`, and then the donor's contents
/// are written over it. Nothing is written to either input.
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
/// Re-signing a decrypted save is this field write; the target applies the encryption when
/// the container is closed. A publicly shared save usually has the account zeroed, which
/// matches no account.
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

    /// The container's `sce_sys` survives and its contents are replaced by the donor's.
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

    /// Neither input is written to.
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

    /// A file the container did not have is added and reported.
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

    /// A file the donor does not have is left in place, not deleted.
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

    /// A container with no keystone is reported.
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
