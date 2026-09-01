//! What can go wrong talking to a target, told apart.
//!
//! # Why this is not `std::io::Error`
//!
//! The reference implementation returned one, and for a command-line tool that prints and
//! exits it was the right call. It is the wrong call for a library two projects call,
//! because it flattens three situations a caller needs to distinguish:
//!
//! - **A refused port is normal.** It means the payload is not loaded, which is the
//!   ordinary state of a target that has just rebooted. A caller probing five services
//!   expects several of these and must not treat them as failures.
//! - **An unresolved address is the operator's mistake**, and no amount of retrying fixes
//!   it.
//! - **The wrong file shape is the caller's own bug**, caught before anything is sent.
//!
//! An `io::Error` says "something went wrong on a socket" for all three. A diagnostic loop
//! that branches on the difference - and orbistoun's does - would have to match on error
//! strings to get it back, which is how a library teaches its callers to be fragile.

use std::fmt;
use std::time::Duration;

use crate::shape::Shape;

/// Anything that stops a target operation completing.
#[derive(Debug)]
pub enum Error {
    /// The address did not resolve to anything.
    ///
    /// Separate from a refusal because retrying will not help and the fix is different: a
    /// typo in a registration, or a name the network cannot answer for.
    Unresolved {
        /// What was asked for, as the caller wrote it.
        address: String,
    },
    /// Nothing accepted on that port.
    ///
    /// **Not an error in the usual sense.** A target with a payload unloaded refuses, and
    /// so does one that is switched off, and telling those apart is what the duration is
    /// for - see [`crate::service::Reachability`].
    Refused {
        /// The port that refused.
        port: u16,
        /// How long the refusal took to arrive.
        took: Duration,
    },
    /// The file offered is not something the loader can run.
    ///
    /// Caught **before** anything is sent, because the loader cannot catch it: a vendor
    /// module and a plain payload share their first four bytes, so its own check passes
    /// either and then it dies silently on the one it cannot run.
    WrongShape {
        /// What the bytes turned out to be.
        found: Shape,
    },
    /// The target understood the request and said no.
    ///
    /// Separate from everything above because **nothing is broken**. A missing file, a
    /// read-only mount, a path that does not exist: the connection is fine, the service is
    /// fine, and the answer is no. A caller browsing a filesystem meets several of these
    /// per session and must not treat them as the link failing.
    Rejected {
        /// What was being attempted, in the words of the operation rather than the wire.
        doing: String,
        /// What the target said, verbatim, because its wording is the diagnosis.
        reply: String,
    },
    /// The target answered in a shape this crate could not read.
    ///
    /// The remedy is the opposite of [`Error::Rejected`]: that one is usually the
    /// operator's path, this one is usually **this crate being wrong** about a server it
    /// was written against second-hand. So it carries what was actually said, which is the
    /// only useful thing to put in a bug report.
    Unintelligible {
        /// What was being attempted when the answer stopped making sense.
        doing: String,
        /// What arrived instead.
        said: String,
    },
    /// The socket failed in a way with no more specific meaning here.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved { address } => {
                write!(f, "{address} did not resolve to an address")
            }
            Self::Refused { port, took } => {
                write!(f, "nothing accepted on port {port} after {took:?}")
            }
            // The remedy, not just the diagnosis: a person holding the wrong file wants to
            // know which tool wants it, and that is knowable from the shape alone.
            Self::WrongShape { found } => {
                write!(f, "{} - {}", found.describe(), found.remedy())
            }
            Self::Rejected { doing, reply } => {
                write!(f, "the target refused {doing}: {reply}")
            }
            Self::Unintelligible { doing, said } => {
                write!(
                    f,
                    "could not make sense of the target while {doing}: {said}"
                )
            }
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// What a target operation answers.
pub type Result<T> = std::result::Result<T, Error>;
