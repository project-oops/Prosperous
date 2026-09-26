//! Reading the system log.
//!
//! The service streams whatever the kernel emits for as long as the connection is open, with
//! no end and no length, so the caller says how long to listen. An empty read is a quiet
//! target, not a failure.
//!
//! [`crate::log::read`] listens for a fixed window, for a command line.
//! [`crate::log::follow`] hands back the connection line by line, for a view that must not
//! miss messages between windows.

use std::time::Duration;

use crate::error::Result;
use crate::wire;

/// Service name a port override uses to move the log off its usual port.
const SERVICE: &str = "klogsrv";

/// Port the log service listens on.
const PORT: u16 = 3232;

/// How long to wait for the connection.
const CONNECT: Duration = Duration::from_secs(4);

/// Listens to the system log for `window` and returns what arrived.
///
/// # Errors
///
/// [`crate::Error::Refused`] when the log service is not loaded, the ordinary state of a
/// target that has just restarted. The service is optional.
pub fn read(link: &crate::Link, window: Duration) -> Result<String> {
    read_at(&link.address, link.port(SERVICE, PORT), window)
}

/// Listens to a log service on a port other than the usual one.
///
/// Public for the same reason as [`crate::loader::send_at`].
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
/// A follower on a quiet log is blocked inside a read, where a flag would never be checked.
/// This second handle shuts the connection down under it, so the read returns and the
/// iterator ends.
#[derive(Debug)]
pub struct Stopper(std::net::TcpStream);

impl Stopper {
    /// Ends the follow.
    ///
    /// Safe to call more than once and after the follow has ended; the error from shutting
    /// a closed socket is ignored.
    pub fn stop(&self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

/// Opens the log for following.
///
/// The caller stops by dropping the iterator or through the [`Stopper`]. The connection has
/// no read timeout, because a log silent for a minute is a quiet target, not a broken one.
///
/// # Errors
///
/// [`crate::Error::Refused`] when the service is not loaded, the ordinary state of a target
/// that has just restarted.
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
