//! Sending a payload to the target and running it.
//!
//! The loader accepts anything beginning `7f 45 4c 46` and dies silently on what it cannot
//! run, so the bytes are checked before sending. See [`crate::shape`].
//!
//! The loader duplicates the connection socket onto the payload's standard output and error,
//! so a payload sent this way reports over that socket. A payload installed as a package or
//! started from the home screen has no such socket, so the read-back is optional and nothing
//! depends on it.

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
/// Generous: a send that fails halfway leaves the loader holding a partial file.
const TRANSFER: Duration = Duration::from_secs(30);

/// Sends a payload and listens for whatever it says.
///
/// A zero `listen` sends and returns immediately, for a payload that reports elsewhere.
///
/// # Errors
///
/// [`Error::WrongShape`] before anything is sent, if the bytes are not a payload.
/// [`Error::Unresolved`] or [`Error::Refused`] if the loader cannot be reached, which usually
/// means the entry point needs re-running.
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
/// Public because a target reached through a tunnel or forward answers on the port the
/// tunnel chose, and because it lets a caller point this at a fake.
///
/// # Errors
///
/// As [`send`].
pub fn send_at(address: &str, port: u16, payload: &[u8], listen: Duration) -> Result<String> {
    // Checked before connecting, so the loader never cleans up an abandoned connection.
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
