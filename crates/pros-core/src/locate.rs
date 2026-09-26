//! Asking a target which of several candidate directories exists.
//!
//! Some directories are properties of the system (`/user/app`, `/data/pkg`) and are
//! constants. Others are made by whichever payload is installed: the common cheat runner
//! reads `/data/cheatrunner/cheats`, `/data/etaHEN/cheats` and `/data/elf-arsenal/cheats`,
//! and which one exists depends on what the target has installed. Only the target can
//! answer that. Before it is asked there is no answer at all, for the same reason as
//! [`crate::payloads::Presence::Unknown`].

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
    /// A finding, not a failure: for cheats it means no cheat runner is installed.
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
/// The first that lists wins, so `candidates` is in order of preference - the tool's own
/// directory before the ones it reads for compatibility.
///
/// # Errors
///
/// Only when the target cannot be reached. A missing candidate is an answer, not an error.
pub fn first_of(link: &pros_link::Link, candidates: &[&str]) -> Result<Where, String> {
    let mut session = Session::open(link).map_err(|why| why.to_string())?;
    let mut tried = Vec::new();
    for candidate in candidates {
        // Any listing means the directory exists; an empty one is still where things go.
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

    /// A find carries the candidates tried before it.
    #[test]
    fn a_find_carries_what_was_tried_first() {
        let found = Where::Found {
            path: "/data/etaHEN/cheats".to_owned(),
            instead_of: vec!["/data/cheatrunner/cheats".to_owned()],
        };
        assert_eq!(found.path(), Some("/data/etaHEN/cheats"));
    }

    /// None of them has no path, rather than falling back to the first candidate.
    #[test]
    fn none_of_them_offers_no_path_to_fall_back_on() {
        let nothing = Where::NoneOfThem(vec!["/data/cheatrunner/cheats".to_owned()]);
        assert_eq!(nothing.path(), None);
    }
}
