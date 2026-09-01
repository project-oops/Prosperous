//! Sending a payload to the target and running it.
//!
//! # The guard is the point
//!
//! The loader accepts anything beginning `7f 45 4c 46` and then dies silently on the ones
//! it cannot run. It cannot tell a payload from a vendor module, so this does, before a
//! byte leaves the machine. See [`crate::shape`].
//!
//! # The read-back is a convenience and never a mechanism
//!
//! The loader duplicates the connection socket onto the payload standard output and
//! standard error, so a payload sent this way reports over the socket it arrived on.
//!
//! **Nothing may be built on that.** A payload installed as a package, or started from the
//! home screen, has no such socket and its output goes wherever its own sink puts it. This
//! is offered as an optional window on the send call, and a caller that requires output to
//! appear here has written something that works only when the payload is delivered one
//! particular way.

use std::io::Write as _;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::service::LOADER;
use crate::shape;
use crate::wire;

/// How long to wait for the connection itself.
const CONNECT: Duration = Duration::from_secs(6);

/// How long to allow for the transfer.
///
/// Generous: a payload is small but the link is a target on somebody home network, and a
/// send that fails halfway leaves the loader holding a partial file.
const TRANSFER: Duration = Duration::from_secs(30);

/// Sends a payload and listens for whatever it says.
///
/// `listen` may be zero, which sends and returns immediately. That is the honest choice
/// for a payload that reports somewhere else - see the module note.
///
/// # Errors
///
/// [`Error::WrongShape`] before anything is sent, if the bytes are not a payload.
/// [`Error::Unresolved`] or [`Error::Refused`] if the loader cannot be reached, which for
/// this service usually means the jailbreak needs re-running rather than a payload
/// reloading.
pub fn send(link: &crate::Link, payload: &[u8], listen: Duration) -> Result<String> {
    send_at(
        &link.address,
        link.port(&LOADER.name, LOADER.port),
        payload,
        listen,
    )
}

/// Sends a payload to a loader on a port other than the usual one.
///
/// # Why this is public rather than a test hatch
///
/// A target is not always where it says it is. Reached through a tunnel or a forward it
/// answers on whatever port the tunnel chose, and a library that only knows the default
/// has decided that case is not real. It is - and it is also what lets a caller point this
/// at a fake without a second implementation to keep in step.
///
/// # Errors
///
/// As [`send`].
pub fn send_at(address: &str, port: u16, payload: &[u8], listen: Duration) -> Result<String> {
    // Before the connection, not after: refusing early costs the caller nothing, and a
    // connection opened to be abandoned is a connection the loader has to clean up.
    let found = shape::identify(payload);
    if !found.is_payload() {
        return Err(Error::WrongShape { found });
    }

    let mut stream = wire::connect(address, port, CONNECT)?;
    stream.set_write_timeout(Some(TRANSFER))?;
    stream.write_all(payload)?;
    stream.flush()?;

    if listen.is_zero() {
        return Ok(String::new());
    }
    wire::read_for(&mut stream, listen)
}
