//! What can go wrong in the bridge, one variant per kind of fault.

use std::fmt;

/// The crate's result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// A reason the bridge could not do something.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// A block cipher input was not a whole number of 16-byte blocks.
    ///
    /// Every pairing message is block-aligned by construction, so this means the peer sent
    /// something malformed.
    NotBlockAligned {
        /// How many bytes arrived.
        len: usize,
    },
    /// A hex field on the wire was not valid hex.
    Hex(hex::FromHexError),
    /// The certificate or key could not be encoded or decoded.
    Certificate(String),
    /// An RSA signature could not be made or did not verify.
    Signature(String),
    /// A pairing message arrived for a phase that does not expect it, or out of order.
    Pairing(String),
    /// Something underneath returned an I/O error.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBlockAligned { len } => {
                write!(f, "cipher input was {len} bytes, not a multiple of 16")
            }
            Self::Hex(error) => write!(f, "a hex field did not decode: {error}"),
            Self::Certificate(why) => write!(f, "certificate: {why}"),
            Self::Signature(why) => write!(f, "signature: {why}"),
            Self::Pairing(why) => write!(f, "pairing: {why}"),
            Self::Io(error) => write!(f, "io: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Hex(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<hex::FromHexError> for Error {
    fn from(error: hex::FromHexError) -> Self {
        Self::Hex(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
