//! What can go wrong talking to a target, told apart.
//!
//! A plain `std::io::Error` would flatten cases a caller branches on: a refused port is the
//! normal state of a service that is not loaded, an unresolved address is the operator's
//! mistake, and a wrong file shape is the caller's bug caught before sending. Each is its
//! own variant so no caller matches on error strings.

use std::fmt;
use std::time::Duration;

use crate::shape::Shape;

/// Anything that stops a target operation completing.
#[derive(Debug)]
pub enum Error {
    /// The address did not resolve to anything.
    ///
    /// Retrying does not help: the fix is the registration or the network's name service.
    Unresolved {
        /// What was asked for, as the caller wrote it.
        address: String,
    },
    /// Nothing accepted on that port.
    ///
    /// A target with the payload unloaded refuses, and so does one that is switched off;
    /// the duration tells them apart. See [`crate::service::Reachability`].
    Refused {
        /// The port that refused.
        port: u16,
        /// How long the refusal took to arrive.
        took: Duration,
    },
    /// The file offered is not something the loader can run.
    ///
    /// Caught before anything is sent: a vendor module and a plain payload share their first
    /// four bytes, so the loader accepts either and dies silently on the one it cannot run.
    WrongShape {
        /// What the bytes turned out to be.
        found: Shape,
    },
    /// The target understood the request and said no.
    ///
    /// A missing file, a read-only mount or a bad path: the link and the service are fine,
    /// and the answer is no.
    Rejected {
        /// What was being attempted, in the words of the operation rather than the wire.
        doing: String,
        /// What the target said, verbatim, because its wording is the diagnosis.
        reply: String,
    },
    /// The target answered in a shape this crate could not read.
    ///
    /// Unlike [`Error::Rejected`], this usually means this crate is wrong about the server,
    /// so it carries what was said for the bug report.
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
            // The shape alone says which tool wants the file, so the message names it.
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
