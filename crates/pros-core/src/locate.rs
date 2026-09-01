//! Asking a target which of several places a thing actually lives in.
//!
//! # Why a path can be a question rather than a constant
//!
//! Some directories on this platform are properties of the machine: `/user/app`,
//! `/user/appmeta`, `/user/home`, `/data/pkg`. They are there because the system put them
//! there, and a constant is the right way to name one.
//!
//! Others are made by whatever payload happens to be running. Cheats are the clearest case.
//! Reading a working tool for the path gives `/data/etaHEN/cheats`; reading the cheat runner
//! that most people use gives **three**, and says it reads all of them:
//!
//! - `/data/cheatrunner/cheats`
//! - `/data/etaHEN/cheats`
//! - `/data/elf-arsenal/cheats`
//!
//! None of the three is *the* path. Which one is right depends on what somebody installed,
//! and no amount of reading source code settles it - only the target can. A constant here
//! would be correct on some targets and would send everybody else looking for a directory
//! their machine was never going to have.
//!
//! # What this returns, and why it is three answers
//!
//! **Found, none of them, or nothing asked.** The third exists for the same reason
//! [`crate::payloads::Presence::Unknown`] does: before a target has been asked, *not found*
//! is not a fact, and presenting it as one is inventing a measurement.
//!
//! A target with none of them is a real and common state - it means no cheat runner is
//! installed - and it is worth saying in those words rather than showing an empty listing of
//! a path that does not exist.

use pros_link::files::Session;

/// Where something turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// It is here, and these are the others that were tried and were not there.
    Found {
        /// The first candidate that listed.
        path: String,
        /// Everything tried before it.
        instead_of: Vec<String>,
    },
    /// The target was asked about every candidate and had none of them.
    ///
    /// **A finding, not a failure.** For cheats it means no cheat runner is installed, which
    /// is what a person needs told - not an empty directory listing.
    NoneOfThem(Vec<String>),
}

impl Where {
    /// The path to use, when there is one.
    #[must_use]
    pub fn path(&self) -> Option<&str> {
        match self {
            Self::Found { path, .. } => Some(path),
            Self::NoneOfThem(_) => None,
        }
    }
}

/// Asks a target which of these directories it has.
///
/// The first that lists wins, so the order of `candidates` is the order of preference - put
/// the tool's own directory before the ones it reads for compatibility.
///
/// # Errors
///
/// Only when the target cannot be reached at all. **A candidate that is not there is not an
/// error**; it is the answer to the question being asked, and stopping at the first one would
/// mean never reaching the second.
pub fn first_of(link: &pros_link::Link, candidates: &[&str]) -> Result<Where, String> {
    let mut session = Session::open(link).map_err(|why| why.to_string())?;
    let mut tried = Vec::new();
    for candidate in candidates {
        // A listing that comes back at all is the directory existing. Its contents are not
        // the question here - an installed cheat runner with no cheats in it yet still has
        // the directory, and that is where cheats should go.
        if session.list(candidate).is_ok() {
            session.close();
            return Ok(Where::Found {
                path: (*candidate).to_owned(),
                instead_of: tried,
            });
        }
        tried.push((*candidate).to_owned());
    }
    session.close();
    Ok(Where::NoneOfThem(tried))
}

#[cfg(test)]
mod tests {
    use super::Where;

    /// The winner names what it beat, so a person can see which tools were looked for.
    #[test]
    fn a_find_carries_what_was_tried_first() {
        let found = Where::Found {
            path: "/data/etaHEN/cheats".to_owned(),
            instead_of: vec!["/data/cheatrunner/cheats".to_owned()],
        };
        assert_eq!(found.path(), Some("/data/etaHEN/cheats"));
    }

    /// **None of them has no path**, rather than falling back to the first candidate.
    ///
    /// A fallback would put a directory that does not exist in the path box, and the empty
    /// listing under it would look exactly like an installed cheat runner with no cheats.
    #[test]
    fn none_of_them_offers_no_path_to_fall_back_on() {
        let nothing = Where::NoneOfThem(vec!["/data/cheatrunner/cheats".to_owned()]);
        assert_eq!(nothing.path(), None);
    }
}
