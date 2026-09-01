//! Where a copied save came from, and whether it can go back as-is.
//!
//! # The question a save transfer has to answer first
//!
//! Save data on this platform is encrypted and signed for the account that wrote it. Copying
//! the files off a target is a plain fetch and always works. Copying them *on* is only a
//! plain send when they are going back to the account they came from - otherwise they need
//! decrypting and re-signing for the target, which is a different job needing a payload this
//! project does not carry.
//!
//! **The two cases look identical.** Both are a directory of files going across a network,
//! both complete without error, and the difference only shows up later when a target refuses
//! a save somebody was relying on. So the transfer has to decide before it starts, and it
//! cannot decide by looking at the files.
//!
//! # Asking the save, before asking anything this project wrote down
//!
//! A save carries its own answer. `ACCOUNT_ID` sits in the `.sfo` parameter file a target
//! keeps beside it, and comparing that against the account already on the destination settles
//! the question outright - for a save that arrived from anywhere, by any route, not only for
//! one this tool copied. That is the first source, and it is better than any record this
//! project could keep.
//!
//! **It is not always there.** Measured on a target with three saves: one carried `.sfo`
//! files, two carried only icons. A design that used it alone would work for one save in
//! three and fail quietly for the rest, which is the shape of failure this whole project
//! exists to avoid.
//!
//! So there is a second source. When a save is fetched, the account it belongs to is in the
//! path it came out of; that is written down beside the copy at the moment it is known for
//! certain, and read back if the parameter file cannot answer.
//!
//! # And a third answer, which is no answer
//!
//! Neither source, and the result is [`crate::origin::Needs::Unknown`] - deliberately not the same as
//! *fine*. A save with no provenance is precisely the one most likely to have come from
//! another target, so defaulting it to a plain copy would put the failure exactly where it
//! does most damage.
//!
//! The same holds for a known account with nothing to compare it against: an identifier on
//! its own says nothing about whether a copy will work.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file written beside a copied save.
///
/// Named to sort with dotfiles and to be obviously this tool's, so somebody looking at the
/// folder can tell what it is and delete it without wondering.
pub const RECORD: &str = ".prosperous-origin.json";

/// Where a copy came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The target as it was registered here.
    pub target: String,
    /// Its address at the time.
    pub address: String,
    /// **The account the save belongs to.** The part that decides everything below.
    pub user: String,
    /// The path it was taken from, whole.
    pub from: String,
    /// When, as seconds since the epoch. Informational.
    pub when: u64,
}

/// What a save needs before it can go back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Needs {
    /// Nothing: it is going back to the account it came from.
    Nothing,
    /// Decrypting and re-signing, because the account differs.
    ///
    /// Carries both accounts, because *which* differs is the first thing somebody asks and
    /// the answer decides whether they meant to do this at all.
    Resigning {
        /// The account that wrote it.
        wrote: String,
        /// The account it is going to.
        going_to: String,
    },
    /// **Nothing here can tell.**
    ///
    /// No record beside the copy, or the destination is not a save path this recognises. Not
    /// the same as [`Needs::Nothing`]: an unrecorded save is the one most likely to have come
    /// from another target.
    Unknown(String),
}

impl Needs {
    /// Whether a plain copy is enough.
    #[must_use]
    pub const fn is_plain(&self) -> bool {
        matches!(self, Self::Nothing)
    }
}

/// The account a save path belongs to.
///
/// `/user/home/<user>/savedata_prospero/<title>` gives `<user>`. Anything that is not that
/// shape gives nothing, which is what makes an unrecognised destination *unknown* rather than
/// silently matching.
#[must_use]
pub fn user_in(path: &str) -> Option<String> {
    let mut parts = path.trim_start_matches('/').split('/');
    (parts.next()? == "user" && parts.next()? == "home")
        .then(|| parts.next().map(str::to_owned))
        .flatten()
        .filter(|user| !user.is_empty())
}

/// Writes the record beside a fetched copy.
///
/// # Errors
///
/// When the folder cannot be written to. **Worth reporting rather than ignoring**: a backup
/// with no record is one that will read as *unknown* forever, and somebody should find that
/// out now rather than at the restore.
pub fn stamp(into: &Path, origin: &Origin) -> Result<PathBuf, String> {
    let path = into.join(RECORD);
    let text = serde_json::to_string_pretty(origin).map_err(|why| why.to_string())?;
    std::fs::create_dir_all(into).map_err(|why| why.to_string())?;
    std::fs::write(&path, text).map_err(|why| why.to_string())?;
    Ok(path)
}

/// Reads the record beside a copy, if there is one.
#[must_use]
pub fn of(folder: &Path) -> Option<Origin> {
    let text = std::fs::read_to_string(folder.join(RECORD)).ok()?;
    serde_json::from_str(&text).ok()
}

/// The account a copied save says it belongs to, from its own parameter file.
///
/// **Better than anything this project could record**, because it is true for a save that
/// arrived from anywhere rather than only for one this tool copied. Any `.sfo` in the folder
/// or below it will do - they carry the same account, being the same target's saves.
///
/// `None` when there is no parameter file, which is common: of three saves measured on a
/// target, one carried `.sfo` files and two carried only icons. That is why this is the
/// first source and not the only one.
#[must_use]
pub fn account_of(folder: &Path) -> Option<String> {
    fn look(dir: &Path, depth: usize) -> Option<String> {
        if depth > 3 {
            return None;
        }
        let mut below = Vec::new();
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                below.push(path);
            } else if path
                .extension()
                .is_some_and(|end| end.eq_ignore_ascii_case("sfo"))
                && let Ok(bytes) = std::fs::read(&path)
                && let Ok(parameters) = crate::sfo::read(&bytes)
                && let Some(account) = crate::sfo::account_id(&parameters)
            {
                return Some(account);
            }
        }
        below.into_iter().find_map(|dir| look(&dir, depth + 1))
    }
    look(folder, 0)
}

/// Decides what a copy needs to go to this destination.
///
/// # Three sources, in order of what they are worth
///
/// 1. **The save's own `ACCOUNT_ID`**, read out of its parameter file, compared against the
///    account already on the destination target. True regardless of where the save came
///    from or which tool moved it.
/// 2. **The record written when this tool made the copy**, which covers saves whose parameter
///    file is missing - two in three, measured.
/// 3. **Nothing**, which is [`Needs::Unknown`] and is not the same as fine.
///
/// `account_here` is the account the destination target belongs to, when it is known - read
/// from a save already on it, by the same route as (1). Without it the first source cannot be
/// used, because an account identifier means nothing on its own; there is only something to
/// compare it *to*.
#[must_use]
pub fn needed(folder: &Path, to: &str, account_here: Option<&str>) -> Needs {
    // The save's own word first.
    if let Some(theirs) = account_of(folder) {
        return match account_here {
            Some(ours) if ours.eq_ignore_ascii_case(&theirs) => Needs::Nothing,
            Some(ours) => Needs::Resigning {
                wrote: theirs,
                going_to: ours.to_owned(),
            },
            None => Needs::Unknown(format!(
                "this save says it belongs to account {theirs}, and nothing has read which \
                 account the target belongs to"
            )),
        };
    }

    // Then what was written down when the copy was made.
    let Some(going_to) = user_in(to) else {
        return Needs::Unknown(format!(
            "{to} is not a save path this recognises, and the save carries no parameter file, \
             so whose it is cannot be checked"
        ));
    };
    let Some(origin) = of(folder) else {
        return Needs::Unknown(format!(
            "no parameter file in this copy and no {RECORD} beside it, so which account wrote \
             it is not known"
        ));
    };
    if origin.user == going_to {
        Needs::Nothing
    } else {
        Needs::Resigning {
            wrote: origin.user,
            going_to,
        }
    }
}

/// The moment now, in seconds, for stamping a record.
#[must_use]
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[cfg(test)]
mod tests {
    use super::{Needs, Origin, needed, stamp, user_in};

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("prosperous-origin-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("makes it");
        path
    }

    fn origin(user: &str) -> Origin {
        Origin {
            target: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            user: user.to_owned(),
            from: format!("/user/home/{user}/savedata_prospero/PPSA01650"),
            when: 0,
        }
    }

    /// The account is the third segment, and only in a path of the right shape.
    #[test]
    fn the_account_comes_out_of_the_path() {
        assert_eq!(
            user_in("/user/home/1ea2f4d9/savedata_prospero/PPSA01650").as_deref(),
            Some("1ea2f4d9")
        );
        assert_eq!(user_in("/user/home/1ea2f4d9").as_deref(), Some("1ea2f4d9"));
        assert_eq!(user_in("/data/pkg"), None);
        assert_eq!(user_in("/user/app/PPSA01650"), None);
        assert_eq!(user_in("/user/home"), None);
    }

    /// **Going back where it came from needs nothing.** The ordinary case, and the only one
    /// where a plain copy is honest.
    #[test]
    fn a_save_going_back_to_its_own_account_is_a_plain_copy() {
        let folder = scratch("same");
        stamp(&folder, &origin("1ea2f4d9")).expect("stamps");

        let needs = needed(
            &folder,
            "/user/home/1ea2f4d9/savedata_prospero/PPSA01650",
            None,
        );
        assert_eq!(needs, Needs::Nothing);
        assert!(needs.is_plain());
    }

    /// **A different account needs re-signing, and both accounts are named.**
    ///
    /// The failure this prevents is silent: the copy would succeed, and the target would
    /// reject the save later, with nothing to connect the two events.
    #[test]
    fn a_save_going_to_another_account_needs_resigning() {
        let folder = scratch("different");
        stamp(&folder, &origin("1ea2f4d9")).expect("stamps");

        let needs = needed(
            &folder,
            "/user/home/beefcafe/savedata_prospero/PPSA01650",
            None,
        );
        assert_eq!(
            needs,
            Needs::Resigning {
                wrote: "1ea2f4d9".to_owned(),
                going_to: "beefcafe".to_owned(),
            }
        );
        assert!(!needs.is_plain());
    }

    /// **A copy with no record is unknown, not fine.**
    ///
    /// It is also the likeliest to need work, having come from somewhere else. Defaulting it
    /// to a plain copy would put the failure exactly where it does most harm.
    #[test]
    fn a_copy_from_nowhere_known_is_not_assumed_to_be_fine() {
        let folder = scratch("nameless");
        let needs = needed(
            &folder,
            "/user/home/1ea2f4d9/savedata_prospero/PPSA01650",
            None,
        );
        assert!(matches!(needs, Needs::Unknown(_)), "{needs:?}");
        assert!(!needs.is_plain());
    }

    /// A destination that is not a save path cannot be checked, and says so rather than
    /// matching by accident.
    #[test]
    fn an_unrecognised_destination_is_unknown_rather_than_matching() {
        let folder = scratch("elsewhere");
        stamp(&folder, &origin("1ea2f4d9")).expect("stamps");

        let needs = needed(&folder, "/data/pkg", None);
        assert!(matches!(needs, Needs::Unknown(_)), "{needs:?}");
    }

    /// **The save's own parameter file beats anything recorded here.**
    ///
    /// It is true for a save that arrived from anywhere - downloaded, handed over, copied by
    /// another tool - where the record beside a copy only ever describes copies this made.
    #[test]
    fn the_saves_own_account_is_what_decides_when_it_has_one() {
        let folder = scratch("parameters");
        std::fs::write(folder.join("save.sfo"), sfo_for("769f77716958d37e")).expect("writes");
        // A record that disagrees, to prove which one is consulted.
        stamp(&folder, &origin("someone-else")).expect("stamps");

        assert_eq!(
            needed(
                &folder,
                "/user/home/whatever/savedata_prospero/PPSA01650",
                Some("769f77716958d37e")
            ),
            Needs::Nothing,
            "the accounts match, so it is a plain copy"
        );
        assert_eq!(
            needed(
                &folder,
                "/user/home/whatever/savedata_prospero/PPSA01650",
                Some("00112233445566aa")
            ),
            Needs::Resigning {
                wrote: "769f77716958d37e".to_owned(),
                going_to: "00112233445566aa".to_owned(),
            }
        );
    }

    /// **An account with nothing to compare it to is unknown.**
    ///
    /// An identifier on its own says nothing about whether a copy will work; the whole
    /// question is whether it matches the target's. Treating a known account as sufficient
    /// would be the boolean mistake in a new place.
    #[test]
    fn an_account_with_nothing_to_compare_it_to_is_not_an_answer() {
        let folder = scratch("nothing-to-compare");
        std::fs::write(folder.join("save.sfo"), sfo_for("769f77716958d37e")).expect("writes");

        let needs = needed(
            &folder,
            "/user/home/1ea2f4d9/savedata_prospero/PPSA01650",
            None,
        );
        assert!(matches!(needs, Needs::Unknown(_)), "{needs:?}");
    }

    /// The parameter file is found below the top of the copy, since that is where a target
    /// keeps it - under a title folder, not beside it.
    #[test]
    fn a_parameter_file_further_down_is_still_found() {
        let folder = scratch("nested");
        let below = folder.join("user").join("PPSA01650");
        std::fs::create_dir_all(&below).expect("makes it");
        std::fs::write(below.join("sce_bu.sfo"), sfo_for("769f77716958d37e")).expect("writes");

        assert_eq!(
            super::account_of(&folder).as_deref(),
            Some("769f77716958d37e")
        );
    }

    /// A minimal parameter file carrying one account.
    fn sfo_for(account: &str) -> Vec<u8> {
        let bytes: Vec<u8> = (0..account.len() / 2)
            .map(|at| u8::from_str_radix(&account[at * 2..at * 2 + 2], 16).expect("hex"))
            .collect();
        let key = b"ACCOUNT_ID\0";
        let mut out = Vec::new();
        out.extend_from_slice(b"\0PSF");
        out.extend_from_slice(&0x0101_u32.to_le_bytes());
        out.extend_from_slice(&36_u32.to_le_bytes());
        out.extend_from_slice(&(36 + u32::try_from(key.len()).expect("small")).to_le_bytes());
        out.extend_from_slice(&1_u32.to_le_bytes());
        out.extend_from_slice(&0_u16.to_le_bytes());
        out.extend_from_slice(&0x0004_u16.to_le_bytes());
        out.extend_from_slice(&u32::try_from(bytes.len()).expect("small").to_le_bytes());
        out.extend_from_slice(&u32::try_from(bytes.len()).expect("small").to_le_bytes());
        out.extend_from_slice(&0_u32.to_le_bytes());
        out.extend_from_slice(key);
        out.extend_from_slice(&bytes);
        out
    }
}
