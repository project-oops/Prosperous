//! Running a command on the target without loading a payload.
//!
//! The service is raw TCP with no telnet option negotiation. A telnet client's opening `IAC`
//! bytes would land in the shell as typed input, so this sends plain bytes.
//!
//! The server neither closes after a command nor sends an end marker, so a response is
//! complete when nothing more arrives.

use std::io::Write as _;
use std::time::Duration;

use crate::error::Result;
use crate::wire;

/// Service name a port override uses to move the shell off its usual port.
const SERVICE: &str = "shsrv";

/// Port the shell listens on.
const PORT: u16 = 2323;

/// How long to wait for the connection.
const CONNECT: Duration = Duration::from_secs(6);

/// How long to let the banner and prompt arrive before typing.
const BANNER: Duration = Duration::from_millis(600);

/// Runs one command and returns what it printed.
///
/// `settle` is how long silence lasts before the answer counts as complete. Too short
/// truncates output; too long only costs a moment.
///
/// # Errors
///
/// [`crate::Error::Refused`] when the shell is not loaded. The shell is optional; without it
/// commands go through a payload instead.
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

    wire::drain_banner(&mut stream, BANNER);

    stream.write_all(command.as_bytes())?;
    // A bare `\n`, not `\r\n`: the shell does not strip `\r`, so `launch <id>\r` finds no such
    // title. CRLF is right only on the FTP control channel.
    stream.write_all(b"\n")?;
    stream.flush()?;

    wire::read_until_quiet(&mut stream, settle)
}
