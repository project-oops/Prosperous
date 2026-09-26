//! Everything a `pros-core` operation can fail with.
//!
//! A variant that a caller branches on carries a typed reason; the rest carry the words of
//! the layer that failed. Every message reads the same as the reason's own `Display`.

use std::fmt;
use std::path::Path;

use crate::checksum::{Mismatch, Unreadable};
use crate::fetch::NotFetched;
use crate::manifest::NotAManifest;
use crate::sfo::NotChanged;
use crate::sources::NotAsked;
use crate::staging::NotStaged;
use crate::target::Ambiguous;

/// A `Result` whose error is [`Error`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Anything that stops a `pros-core` operation completing.
#[derive(Debug)]
pub enum Error {
    /// The transport to the target failed.
    Link(pros_link::Error),
    /// A file or directory on this machine could not be read or written.
    Io(std::io::Error),
    /// A checksum that cannot be used.
    Checksum(Unreadable),
    /// Bytes that are not the bytes that were described.
    Mismatch(Mismatch),
    /// A payload that was not fetched.
    NotFetched(NotFetched),
    /// A document that could not be read as a manifest.
    NotAManifest(NotAManifest),
    /// A parameter that could not be changed.
    NotChanged(NotChanged),
    /// A release question that produced no answer.
    NotAsked(NotAsked),
    /// A file that was not staged.
    NotStaged(NotStaged),
    /// A target that could not be picked.
    Ambiguous(Ambiguous),
    /// Anything else, in the words of whatever refused.
    Failed(String),
}

impl Error {
    /// A failure described in words, for cases no caller tells apart.
    pub fn failed(why: impl Into<String>) -> Self {
        Self::Failed(why.into())
    }

    /// A failure at a path on this machine, reading `<path>: <why>`.
    pub fn at(path: &Path, why: impl fmt::Display) -> Self {
        Self::Failed(format!("{}: {why}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Link(why) => why.fmt(f),
            Self::Io(why) => why.fmt(f),
            Self::Checksum(why) => why.fmt(f),
            Self::Mismatch(why) => why.fmt(f),
            Self::NotFetched(why) => why.fmt(f),
            Self::NotAManifest(why) => why.fmt(f),
            Self::NotChanged(why) => why.fmt(f),
            Self::NotAsked(why) => why.fmt(f),
            Self::NotStaged(why) => why.fmt(f),
            Self::Ambiguous(why) => why.fmt(f),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Link(why) => Some(why),
            Self::Io(why) => Some(why),
            _ => None,
        }
    }
}

/// Each reason converts into the crate error, so `?` lifts it.
macro_rules! lift {
    ($($reason:ty => $variant:ident),* $(,)?) => {
        $(impl From<$reason> for Error {
            fn from(why: $reason) -> Self {
                Self::$variant(why)
            }
        })*
    };
}

lift! {
    pros_link::Error => Link,
    std::io::Error => Io,
    Unreadable => Checksum,
    Mismatch => Mismatch,
    NotFetched => NotFetched,
    NotAManifest => NotAManifest,
    NotChanged => NotChanged,
    NotAsked => NotAsked,
    NotStaged => NotStaged,
    Ambiguous => Ambiguous,
}
