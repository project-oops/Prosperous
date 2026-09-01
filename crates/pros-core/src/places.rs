//! Where things live on a target, per storage device.
//!
//! # Why this is a table and not constants scattered through the code
//!
//! Every path here belongs to somebody else's program. `pldmgr` decides where payloads are;
//! the `y2jb` autoloader decides where *its* payloads are, and it is not the same place;
//! `ShadowMountPlus` decides where games are, and it looks in ten roots this project had never
//! heard of. Each of those was learnt separately and written down wherever it was first
//! needed, which is how the same question came to have two answers in two files.
//!
//! # Why devices are a list rather than an assumption
//!
//! **There are eight USB mounts and two external ones**, and this project knew about two of
//! the first and none of the second. A screen pinned to internal storage cannot answer *is it
//! actually on the stick*, which is the question a person asks when a list they deployed does
//! not load - and answering it by inference from a scan of four hardcoded directories is how
//! three separate wrong diagnoses got made in one afternoon.
//!
//! # Everything here is cited
//!
//! No path in this file is a guess. Each carries where it came from, and a place nobody has
//! measured is **absent rather than plausible** - the packages directory on a stick is not
//! here for exactly that reason.

/// A storage device on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Device {
    /// The console's own drive.
    Internal,
    /// A USB stick, `0` to `7`.
    ///
    /// **Eight, not two.** `pldmgr.h` declares `SCAN_DIRS_COUNT 9` - its own directory plus
    /// `/mnt/usb0/pldmgr` through `/mnt/usb7/pldmgr` - and `ShadowMountPlus` scans the same
    /// eight roots.
    Usb(u8),
    /// An external drive, `0` or `1`.
    ///
    /// From `ShadowMountPlus`, which scans `/mnt/ext0` and `/mnt/ext1` beside the sticks.
    /// Nothing else this project talks to mentions them, which is why they carry fewer places.
    Ext(u8),
}

impl Device {
    /// How many USB mounts a target can have.
    pub const STICKS: u8 = 8;

    /// How many external drives.
    pub const DRIVES: u8 = 2;

    /// Every device a target could have, in the order to offer them.
    ///
    /// **All of them, whether or not they are plugged in.** Asking the target which exist is a
    /// listing per device and this is a menu; a device with nothing on it lists as empty, which
    /// is the same answer for less work and no waiting.
    #[must_use]
    pub fn all() -> Vec<Self> {
        let mut found = vec![Self::Internal];
        found.extend((0..Self::STICKS).map(Self::Usb));
        found.extend((0..Self::DRIVES).map(Self::Ext));
        found
    }

    /// What to call it.
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::Internal => "internal".to_owned(),
            Self::Usb(which) => format!("usb{which}"),
            Self::Ext(which) => format!("ext{which}"),
        }
    }

    /// Where the device is mounted, when it is not the console's own drive.
    #[must_use]
    pub fn root(self) -> Option<String> {
        match self {
            Self::Internal => None,
            Self::Usb(which) => Some(format!("/mnt/usb{which}")),
            Self::Ext(which) => Some(format!("/mnt/ext{which}")),
        }
    }

    /// Whether a startup list on this device is a way back in when the internal one is broken.
    #[must_use]
    pub const fn is_removable(self) -> bool {
        !matches!(self, Self::Internal)
    }
}

/// What a screen is looking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Looking {
    /// ELF payloads.
    Payloads,
    /// Installed and mountable games.
    Titles,
    /// Packages waiting to be registered.
    Packages,
    /// Cheat files.
    Cheats,
    /// Anywhere at all.
    Anything,
}

/// One directory worth offering, and why it is worth offering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spot {
    /// The full path on the target.
    pub path: String,
    /// What to call it in a menu.
    pub label: &'static str,
    /// Whose directory it is, so a person can tell why it exists.
    pub note: &'static str,
}

/// Everywhere worth looking, for this kind of thing on this device.
///
/// Empty when nothing has been measured for that pair - which is an answer, and a different one
/// from a directory that exists and is empty.
#[must_use]
pub fn where_to_look(looking: Looking, device: Device) -> Vec<Spot> {
    match looking {
        Looking::Payloads => payload_places(device),
        Looking::Titles => title_places(device),
        Looking::Packages => package_places(device),
        Looking::Cheats => cheat_places(device),
        // Anywhere at all: the device's own root, and nothing claimed about what is in it.
        Looking::Anything => vec![Spot {
            path: device.root().unwrap_or_else(|| "/".to_owned()),
            label: "the whole device",
            note: "no claim about what is in it",
        }],
    }
}

/// One entry, so the tables below read as tables.
fn spot(path: String, label: &'static str, note: &'static str) -> Spot {
    Spot { path, label, note }
}

/// Where payloads live, which is two different answers on one device.
fn payload_places(device: Device) -> Vec<Spot> {
    match device.root() {
        // ---- payloads -------------------------------------------------------------------
        None => vec![
            spot(
                crate::payloads::INTERNAL.to_owned(),
                "the manager's",
                "pldmgr.h: PAYLOADS_STORAGE_DIR",
            ),
            spot(
                "/data/ps5_autoloader".to_owned(),
                "the autoloader's",
                "y2jb: ps5_autoloader on the internal drive, payloads directly inside it",
            ),
            spot(
                crate::payloads::ELSEWHERE.to_owned(),
                "sent here",
                "where this program's send button used to default to",
            ),
        ],
        Some(root) => vec![
            spot(
                format!("{root}/pldmgr"),
                "the manager's",
                "pldmgr.h: SCAN_DIRS covers /mnt/usb0..usb7/pldmgr",
            ),
            spot(
                format!("{root}/ps5_autoloader"),
                "the autoloader's",
                "y2jb: a stick's root is the first place it looks, payloads directly inside",
            ),
            spot(
                root,
                "the whole device",
                "listed by the manager, resolvable by neither",
            ),
        ],
    }
}

/// Where games live, from the payload that mounts them.
fn title_places(device: Device) -> Vec<Spot> {
    match device.root() {
        None => vec![
            spot(
                "/user/app".to_owned(),
                "installed",
                "where the system keeps applications",
            ),
            spot(
                "/data/homebrew".to_owned(),
                "homebrew",
                "ShadowMountPlus scans this by default",
            ),
            spot(
                "/data/etaHEN/games".to_owned(),
                "etaHEN's games",
                "ShadowMountPlus scans this by default",
            ),
            spot(
                "/mnt/shadowmnt".to_owned(),
                "mounted now",
                "ShadowMountPlus mounts images under here",
            ),
        ],
        Some(root) => vec![
            spot(
                format!("{root}/homebrew"),
                "homebrew",
                "ShadowMountPlus scans this on every stick and external drive",
            ),
            spot(
                format!("{root}/etaHEN/games"),
                "etaHEN's games",
                "ShadowMountPlus scans this on every stick and external drive",
            ),
            spot(
                root,
                "the whole device",
                "ShadowMountPlus scans the root as well",
            ),
        ],
    }
}

/// Where packages wait to be registered.
///
/// **Nothing for a removable device, on purpose.** Both internal directories were measured on a
/// real target; where an installer looks on a stick was not, and a plausible path in a menu is
/// one somebody will believe.
fn package_places(device: Device) -> Vec<Spot> {
    if device.is_removable() {
        return Vec::new();
    }
    vec![
        spot(
            "/data/homebrew/pkg".to_owned(),
            "uploads",
            "measured on a target running an upload tool",
        ),
        spot(
            "/data/pkg".to_owned(),
            "install staging",
            "measured on a target running an upload tool",
        ),
    ]
}

/// Where cheat files live, one directory per tool that reads them.
fn cheat_places(device: Device) -> Vec<Spot> {
    if device.is_removable() {
        return Vec::new();
    }
    vec![
        spot(
            "/data/cheatrunner/cheats".to_owned(),
            "cheatrunner's own",
            "where the cheat runner looks",
        ),
        spot(
            "/data/etaHEN/cheats".to_owned(),
            "etaHEN's",
            "where etaHEN keeps them",
        ),
        spot(
            "/data/elf-arsenal/cheats".to_owned(),
            "elf-arsenal's",
            "where elf-arsenal keeps them",
        ),
    ]
}

/// Which device a path is on, read from the path itself.
///
/// **So a chooser can follow somebody who navigated rather than picked.** A person who walks
/// into `/mnt/usb1/homebrew` from somewhere else has changed device, and a menu still saying
/// *internal* would be describing a different screen from the one they are looking at.
#[must_use]
pub fn device_of(path: &str) -> Device {
    let after = |prefix: &str| -> Option<u8> {
        let rest = path.strip_prefix(prefix)?;
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    if let Some(which) = after("/mnt/usb")
        && which < Device::STICKS
    {
        return Device::Usb(which);
    }
    if let Some(which) = after("/mnt/ext")
        && which < Device::DRIVES
    {
        return Device::Ext(which);
    }
    Device::Internal
}

#[cfg(test)]
mod tests {
    use super::{Device, Looking, device_of, where_to_look};

    /// **Eight sticks and two drives**, because that is what the payloads that use them say.
    #[test]
    fn every_device_a_target_can_have_is_offered() {
        let all = Device::all();
        assert_eq!(all.len(), 1 + 8 + 2);
        assert_eq!(all[0], Device::Internal);
        assert!(all.contains(&Device::Usb(7)), "pldmgr scans up to usb7");
        assert!(
            all.contains(&Device::Ext(1)),
            "ShadowMountPlus scans ext0 and ext1"
        );
    }

    /// A device's places are built from its own root, not from a table of eleven copies.
    #[test]
    fn a_sticks_places_are_under_that_stick() {
        for spot in where_to_look(Looking::Payloads, Device::Usb(3)) {
            assert!(spot.path.starts_with("/mnt/usb3"), "{}", spot.path);
        }
        for spot in where_to_look(Looking::Titles, Device::Ext(1)) {
            assert!(spot.path.starts_with("/mnt/ext1"), "{}", spot.path);
        }
    }

    /// **The two payload directories on a stick are not the same directory.**
    ///
    /// `pldmgr` resolves `<stick>/pldmgr`; the autoloader reads `<stick>/ps5_autoloader` and
    /// loads what is directly inside it. A screen offering only one of them cannot answer why
    /// a list that names a file which is plainly present still fails to load it.
    #[test]
    fn a_stick_offers_both_payload_directories() {
        let paths: Vec<String> = where_to_look(Looking::Payloads, Device::Usb(0))
            .into_iter()
            .map(|spot| spot.path)
            .collect();
        assert!(paths.contains(&"/mnt/usb0/pldmgr".to_owned()), "{paths:?}");
        assert!(
            paths.contains(&"/mnt/usb0/ps5_autoloader".to_owned()),
            "{paths:?}"
        );
    }

    /// **Nothing is offered that nobody measured.** A plausible path in a menu is believed.
    #[test]
    fn a_stick_offers_no_package_directory() {
        assert!(where_to_look(Looking::Packages, Device::Usb(0)).is_empty());
        assert!(
            !where_to_look(Looking::Packages, Device::Internal).is_empty(),
            "and the two internal ones were measured"
        );
    }

    /// Every place says whose it is, so a person can tell why it is on the list.
    #[test]
    fn every_place_says_where_it_came_from() {
        for device in Device::all() {
            for looking in [
                Looking::Payloads,
                Looking::Titles,
                Looking::Packages,
                Looking::Cheats,
                Looking::Anything,
            ] {
                for spot in where_to_look(looking, device) {
                    assert!(!spot.label.is_empty(), "{spot:?}");
                    assert!(spot.note.len() > 10, "{spot:?}");
                    assert!(spot.path.starts_with('/'), "{spot:?}");
                }
            }
        }
    }

    /// **A path names its own device**, so navigating changes the chooser.
    #[test]
    fn a_path_says_which_device_it_is_on() {
        assert_eq!(device_of("/mnt/usb0/pldmgr"), Device::Usb(0));
        assert_eq!(device_of("/mnt/usb7"), Device::Usb(7));
        assert_eq!(device_of("/mnt/ext1/homebrew"), Device::Ext(1));
        assert_eq!(device_of("/data/pldmgr/payloads"), Device::Internal);
        assert_eq!(device_of("/user/app"), Device::Internal);
        // Not a device this project believes in, so not a device.
        assert_eq!(device_of("/mnt/usb9"), Device::Internal);
    }
}
