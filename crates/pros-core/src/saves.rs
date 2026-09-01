//! Where save data is, which is further down than a default can reach.
//!
//! # The shape, measured
//!
//! `/user/home/<user>/savedata_prospero/<title>`. Measured on 2026-08-26: three save folders
//! under one user, each named exactly as a title identifier.
//!
//! The middle part is why this needs code rather than a constant. **Which user is not a
//! question this project can answer on somebody's behalf** - a target can have several, and
//! picking one would be picking somebody's saves for them.
//!
//! # So it descends only when there is nothing to choose between
//!
//! One user, one answer, and the path is shown. More than one, and it stops and says so,
//! because the browser exists for exactly that and a tool that guessed would be guessing
//! about the thing somebody came to protect.
//!
//! That is the same rule as picking a target by name when only one is registered.

use pros_link::files::{Kind, Session};

/// Where user folders live.
///
/// Measured, not guessed.
pub const HOME: &str = "/user/home";

/// What each user's saves sit under.
pub const SAVES: &str = "savedata_prospero";

/// What was found when looking for saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// One user, so one place, and this is it.
    Here(String),
    /// Several users. **Their names, so somebody can choose** - not a pick made for them.
    Several(Vec<String>),
    /// No user folders at all, which is a fact about the target rather than a failure.
    None,
}

/// Picks the user folders out of a listing of the home directory.
///
/// Separated from the fetching so the rule can be tested without a target.
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
        // **Not the first one.** A target with two accounts has two people's saves on it,
        // and a tool that silently picked would be picking whose.
        several => Found::Several(several.to_vec()),
    }
}

/// Asks the target where its saves are.
///
/// # Errors
///
/// Propagates the listing. A home directory that cannot be read is a different problem from
/// one with several users in it, and both are different from one with none.
/// What each user's save metadata sits under.
///
/// Measured: `.sfo` parameter files live here rather than beside the saves themselves, one
/// folder per title, under a `user` level that `savedata_prospero` does not have.
pub const META: &str = "savedata_prospero_meta/user";

/// Which account this target's saves belong to.
///
/// # Why this is read from a save rather than from the target
///
/// Nothing this project can reach announces the account. But every save on the target was
/// written by it, and each one that carries a parameter file states the account in it - so
/// **the target's account is whatever its own saves say it is**.
///
/// Measured: every save on a target named the same account, which is what makes *the*
/// target's account a single thing rather than a choice between several. A target test
/// asserts that, because if it ever stopped being true this answer would depend on which
/// save happened to be read first.
///
/// `None` when nothing could be read - no user, no saves, or none of them carrying a
/// parameter file. **Not an error and not a default**: without it, an incoming save's account
/// has nothing to be compared against, which is [`crate::origin::Needs::Unknown`] rather than
/// permission to copy.
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
                    && let Ok(parameters) = crate::sfo::read(&bytes)
                    && let Some(account) = crate::sfo::account_id(&parameters)
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
/// Propagates the listing.
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

    /// One user is one answer, and it is the path a target was measured using.
    #[test]
    fn one_user_gives_one_place() {
        let listing = [entry("1ea2f4d9", Kind::Directory)];
        assert_eq!(
            decide(&users(&listing)),
            Found::Here("/user/home/1ea2f4d9/savedata_prospero".to_owned())
        );
    }

    /// **Two accounts are two people, and this does not choose between them.**
    ///
    /// Picking the first would be picking whose saves somebody is about to overwrite.
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

    /// A home directory with nothing in it is an answer about the target.
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
