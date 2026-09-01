//! Reading the system log.
//!
//! # Why a window rather than a read to the end
//!
//! This service streams. There is no end to wait for and no length to read - it hands over
//! whatever the kernel emits for as long as the connection is open, so the only sensible
//! interface is one where the caller says how long to listen.
//!
//! **Nothing arriving is a result.** A quiet log is a fact about the target, and a caller
//! that reads an empty string has learned something rather than failed at something.
//!
//! # Two shapes, because there are two questions
//!
//! [`crate::log::read`] takes a window and answers *what did it say just then* - one request, one answer,
//! which is what a command line wants.
//!
//! [`crate::log::follow`] hands back the connection and lets a caller take lines as they arrive. A window
//! showing a log wants that one: reading five seconds at a time means five seconds of not
//! looking, and **a message that arrived in the gap is one nobody sees** - which for a log is
//! the whole failure, since its only job is to say what happened when something went wrong.

use std::time::Duration;

use crate::error::Result;
use crate::wire;

/// Port the log service listens on.
/// Which service an override names when it moves this off its usual port.
const SERVICE: &str = "klogsrv";

const PORT: u16 = 3232;

/// How long to wait for the connection.
const CONNECT: Duration = Duration::from_secs(4);

/// Listens to the system log for `window` and returns what arrived.
///
/// # Errors
///
/// [`crate::Error::Refused`] when the log service is not loaded, which is the ordinary state of a
/// target that has just come back - it is optional, and its absence costs visibility
/// rather than capability.
///
/// [`Error`]: crate::Error
pub fn read(link: &crate::Link, window: Duration) -> Result<String> {
    read_at(&link.address, link.port(SERVICE, PORT), window)
}

/// Listens to a log service on a port other than the usual one.
///
/// Public for the same reason as [`crate::loader::send_at`]: a target behind a tunnel
/// answers where the tunnel put it, and a caller testing against a stand-in needs the same
/// door rather than a parallel one.
///
/// # Errors
///
/// As [`read`].
pub fn read_at(address: &str, port: u16, window: Duration) -> Result<String> {
    let mut stream = wire::connect(address, port, CONNECT)?;
    wire::read_for(&mut stream, window)
}

/// One line of the log, as it arrives.
pub type Line = std::io::Result<String>;

/// A log being followed, and the handle that ends it.
///
/// # Why stopping is a separate object
///
/// Reading blocks. A follower sitting on a quiet log is inside a read, not between lines, so
/// **a flag it checked between lines would never be looked at** - and a quiet log is exactly
/// when somebody gives up watching.
///
/// So the connection is shut down under it. The read returns, the iterator ends, and whoever
/// was driving it is free. That needs a second handle on the same connection, which is what
/// this is.
#[derive(Debug)]
pub struct Stopper(std::net::TcpStream);

impl Stopper {
    /// Ends the follow.
    ///
    /// Safe to call more than once and after it has already ended: shutting a closed socket
    /// is an error nobody needs to hear about.
    pub fn stop(&self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

/// Opens the log for following.
///
/// # Why an iterator rather than a callback
///
/// The caller decides when to stop by dropping it or by using the [`Stopper`]. A callback
/// would have to be told, which means a flag, a check, and a window between the two where
/// lines are still being handed to something that has gone.
///
/// The connection has **no read timeout**: a log that says nothing for a minute is a quiet
/// target, not a broken one, and a timeout would turn the ordinary case into an error.
///
/// # Errors
///
/// [`crate::Error::Refused`] when the service is not loaded - the ordinary state of a target
/// that has just come back.
pub fn follow(link: &crate::Link) -> Result<(Stopper, impl Iterator<Item = Line> + use<>)> {
    follow_at(&link.address, link.port(SERVICE, PORT))
}

/// The same, on another port.
///
/// # Errors
///
/// As [`follow`].
pub fn follow_at(
    address: &str,
    port: u16,
) -> Result<(Stopper, impl Iterator<Item = Line> + use<>)> {
    use std::io::BufRead as _;

    let stream = wire::connect(address, port, CONNECT)?;
    stream.set_read_timeout(None)?;
    let stopper = Stopper(stream.try_clone()?);
    Ok((stopper, std::io::BufReader::new(stream).lines()))
}
