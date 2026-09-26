//! Where save data is on a target.
//!
//! Saves are at `/user/home/<user>/savedata_prospero/<title>` (measured: save folders under
//! one user, each named as a title identifier). A target can have several users, and picking
//! one would be picking somebody's saves for them, so the path is given only when there is
//! exactly one user; otherwise the users are offered for a person to choose.

use pros_link::files::{Kind, Session};

/// Where user folders live (measured).
pub const HOME: &str = "/user/home";

/// What each user's saves sit under.
pub const SAVES: &str = "savedata_prospero";

/// What was found when looking for saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// One user, so one place, and this is it.
    Here(String),
    /// Several users, named so somebody can choose.
    Several(Vec<String>),
    /// No user folders at all, which is a fact about the target rather than a failure.
    None,
}

/// Picks the user folders out of a listing of the home directory.
#[must_use]
pub fn users(entries: &[pros_link::files::Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| entry.is_usable())
        .filter(|entry| entry.kind == Kind::Directory)
        .filter(|entry| entry.name != "." && entry.name != "..")
        .map(|entry| entry.name.clone())
        .collect()
}

/// Decides where saves are from the user folders that were found.
#[must_use]
pub fn decide(users: &[String]) -> Found {
    match users {
        [] => Found::None,
        [only] => Found::Here(format!("{HOME}/{only}/{SAVES}")),
        // Never the first one: two accounts are two people's saves.
        several => Found::Several(several.to_vec()),
    }
}

/// What each user's save metadata sits under.
///
/// Measured: the `.sfo` parameter files live here, one folder per title, under a `user` level
/// that `savedata_prospero` does not have.
pub const META: &str = "savedata_prospero_meta/user";

/// Which account this target's saves belong to.
///
/// Nothing reachable announces the account, but each save's parameter file states the
/// account that wrote it, and every save on a target names the same one (measured, and
/// asserted by a target test). `None` when nothing could be read; an incoming save then has
/// nothing to compare against, which is [`crate::origin::Needs::Unknown`], not permission.
#[must_use]
pub fn account_on(link: &pros_link::Link) -> Option<String> {
    let mut session = Session::open(link).ok()?;
    let found = account_using(&mut session);
    session.close();
    found
}

/// The same, on a session already open.
fn account_using(session: &mut Session) -> Option<String> {
    let users = users(&session.list(HOME).ok()?);
    for user in users {
        let meta = format!("{HOME}/{user}/{META}");
        let Ok(titles) = session.list(&meta) else {
            continue;
        };
        for title in titles {
            let Ok(files) = session.list(&format!("{meta}/{}", title.name)) else {
                continue;
            };
            for file in files {
                if !file.name.to_ascii_lowercase().ends_with(".sfo") {
                    continue;
                }
                if let Ok(bytes) = session.retrieve(&format!("{meta}/{}/{}", title.name, file.name))
                    && let Some(account) = crate::sfo::account_in(&bytes)
                {
                    return Some(account);
                }
            }
        }
    }
    None
}

/// Asks the target where its saves are.
///
/// # Errors
///
/// Propagates the listing. A home directory that cannot be read is a different finding from
/// one with several users, or none.
pub fn find(link: &pros_link::Link) -> Result<Found, String> {
    let mut session = Session::open(link).map_err(|why| why.to_string())?;
    let entries = session.list(HOME).map_err(|why| why.to_string());
    session.close();
    Ok(decide(&users(&entries?)))
}

#[cfg(test)]
mod tests {
    use pros_link::files::{Entry, Kind};

    use super::{Found, decide, users};

    fn entry(name: &str, kind: Kind) -> Entry {
        Entry {
            name: name.to_owned(),
            kind,
            size: None,
            raw: name.to_owned(),
        }
    }

    /// One user gives one path, in the measured layout.
    #[test]
    fn one_user_gives_one_place() {
        let listing = [entry("1ea2f4d9", Kind::Directory)];
        assert_eq!(
            decide(&users(&listing)),
            Found::Here("/user/home/1ea2f4d9/savedata_prospero".to_owned())
        );
    }

    /// Several users are offered, never chosen between.
    #[test]
    fn several_users_are_offered_rather_than_chosen_between() {
        let listing = [
            entry("1ea2f4d9", Kind::Directory),
            entry("2fb3a5e0", Kind::Directory),
        ];
        match decide(&users(&listing)) {
            Found::Several(names) => assert_eq!(names.len(), 2),
            other => panic!("it chose for somebody: {other:?}"),
        }
    }

    /// An empty home directory is a finding, not an error.
    #[test]
    fn no_users_is_a_finding_rather_than_an_error() {
        assert_eq!(decide(&users(&[])), Found::None);
    }

    /// Files and unreadable lines are not users.
    #[test]
    fn only_directories_are_users() {
        let listing = [
            entry("1ea2f4d9", Kind::Directory),
            entry("username.dat", Kind::File),
            Entry {
                name: "total 8".to_owned(),
                kind: Kind::Unrecognised,
                size: None,
                raw: "total 8".to_owned(),
            },
        ];
        assert_eq!(users(&listing), ["1ea2f4d9"]);
    }
}
