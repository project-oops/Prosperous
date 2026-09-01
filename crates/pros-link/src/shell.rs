//! Running a command on the target without loading a payload.
//!
//! # Raw TCP, despite what it is called
//!
//! The service describes itself as telnet-like, and it is not telnet. There is no option
//! negotiation anywhere in it: bytes in, bytes out.
//!
//! That matters because a real telnet client works only by accident. It opens by sending
//! `IAC` option negotiation, which this server reads as somebody typing, and the junk
//! lands in the shell. So this speaks the protocol that is actually there, which is none.
//!
//! # Quiet, not closed
//!
//! The server does not close after a command and sends no end marker, so the only signal
//! that a response has finished is that nothing more arrives. Reading until quiet is the
//! honest implementation of an interface with no framing.

use std::io::Write as _;
use std::time::Duration;

use crate::error::Result;
use crate::wire;

/// Port the shell listens on.
/// Which service an override names when it moves this off its usual port.
const SERVICE: &str = "shsrv";

const PORT: u16 = 2323;

/// How long to wait for the connection.
const CONNECT: Duration = Duration::from_secs(6);

/// How long to let the banner and prompt arrive before typing.
const BANNER: Duration = Duration::from_millis(600);

/// Runs one command and returns what it printed.
///
/// `settle` is how long silence has to last before the answer is considered complete.
/// Generous is the right bias: guessing short truncates output, guessing long costs a
/// moment.
///
/// # Errors
///
/// [`crate::Error::Refused`] when the shell is not loaded. It is optional - its absence means
/// commands have to go through a payload instead, not that nothing can be done.
///
/// [`Error`]: crate::Error
pub fn run(link: &crate::Link, command: &str, settle: Duration) -> Result<String> {
    run_at(&link.address, link.port(SERVICE, PORT), command, settle)
}

/// Runs a command against a shell on a port other than the usual one.
///
/// Public for the same reason as [`crate::loader::send_at`].
///
/// # Errors
///
/// As [`run`].
pub fn run_at(address: &str, port: u16, command: &str, settle: Duration) -> Result<String> {
    let mut stream = wire::connect(address, port, CONNECT)?;

    // Let the greeting land before typing over it.
    wire::drain_banner(&mut stream, BANNER);

    stream.write_all(command.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    wire::read_until_quiet(&mut stream, settle)
}
