//! Connecting, and reading from things that do not say when they have finished.
//!
//! # Three shapes of answer, none of them request-response
//!
//! Every service here is awkward in its own way, and the awkwardness is the interface
//! rather than a defect to be papered over:
//!
//! - the log **streams and never ends**, so a reader stops on a window it chose
//! - the shell has **no framing at all**, so a reader stops when nothing more arrives
//! - the loader **may or may not answer**, so a reader must be correct when it does not
//!
//! A single `read_response` would have to pretend one of those is the others. These
//! helpers keep the difference visible at the call site, where the caller can see which
//! rule it is relying on.

use std::io::Read as _;
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// How long a read blocks before the loop checks its own clock again.
///
/// Short enough that a deadline is honoured promptly, long enough that a quiet connection
/// is not a spin.
const POLL: Duration = Duration::from_millis(250);

/// Opens a connection, telling an unresolved name apart from a refusal.
///
/// The distinction is the caller: a name that does not resolve is a typo somebody must fix,
/// and a port that refuses is usually a payload that is not loaded. Retrying helps with
/// neither, but only one of them is worth mentioning.
///
/// # Errors
///
/// [`Error::Unresolved`] when the address names nothing, [`Error::Refused`] when nothing
/// accepts.
pub(crate) fn connect(address: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    let started = Instant::now();
    let addr = (address, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut addrs| addrs.next())
        .ok_or_else(|| {
            // A name that does not resolve and a port that is shut look identical from the
            // outside - the tool simply does not connect - and the remedies are unrelated.
            tracing::debug!(%address, port, "address did not resolve");
            Error::Unresolved {
                address: address.to_owned(),
            }
        })?;
    TcpStream::connect_timeout(&addr, timeout)
        .inspect(|_| tracing::trace!(%addr, port, took = ?started.elapsed(), "connected"))
        .map_err(|_| {
            let took = started.elapsed();
            // `debug`, not `warn`: on this target a shut port is the ordinary answer for a
            // service the console is not currently running, and the caller is asking in order
            // to find that out. A warning here would fire on a *successful* check.
            tracing::debug!(%addr, port, ?took, "connection refused");
            Error::Refused { port, took }
        })
}

/// Reads for a fixed window, whatever arrives.
///
/// For a stream with no end. Returning nothing is a **result**, not a failure: a quiet log
/// is a fact about the target, and reporting it as an error would make silence look like
/// a broken tool.
///
/// # Errors
///
/// Propagates a socket failure that is not a timeout. A timeout is expected here - it is
/// how a quiet connection announces itself - and is not one.
pub(crate) fn read_for(stream: &mut TcpStream, window: Duration) -> Result<String> {
    stream.set_read_timeout(Some(POLL))?;
    let started = Instant::now();
    let mut out = String::new();
    let mut buffer = [0_u8; 4096];
    while started.elapsed() < window {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(got) => push_lossy(&mut out, buffer.get(..got).unwrap_or_default()),
            Err(error) if is_quiet(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(out)
}

/// Reads until nothing has arrived for `settle`.
///
/// For an interface with no framing, where the only signal that a response has finished is
/// that it stopped. The window is generous on purpose: guessing short truncates output,
/// and guessing long costs a moment.
///
/// # Errors
///
/// As [`read_for`].
pub(crate) fn read_until_quiet(stream: &mut TcpStream, settle: Duration) -> Result<String> {
    stream.set_read_timeout(Some(POLL))?;
    let mut out = String::new();
    let mut buffer = [0_u8; 4096];
    let mut last = Instant::now();
    while last.elapsed() < settle {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(got) => {
                push_lossy(&mut out, buffer.get(..got).unwrap_or_default());
                last = Instant::now();
            }
            Err(error) if is_quiet(&error) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(out)
}

/// Drains whatever a server says before it is spoken to.
///
/// A banner and a prompt arrive unprompted, and typing over them puts the command in the
/// middle of somebody else sentence. Anything other than bytes arriving ends it: a closed
/// stream, or a read timeout saying the server has stopped talking and is waiting.
pub(crate) fn drain_banner(stream: &mut TcpStream, window: Duration) {
    if stream.set_read_timeout(Some(POLL)).is_err() {
        return;
    }
    let started = Instant::now();
    let mut buffer = [0_u8; 4096];
    while started.elapsed() < window {
        match stream.read(&mut buffer) {
            Ok(got) if got > 0 => {}
            _ => break,
        }
    }
}

/// Whether an error means nothing arrived, rather than something broke.
fn is_quiet(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

/// Appends bytes as text, replacing anything that is not.
///
/// Target output is a log, not a document: it can contain a truncated multi-byte sequence
/// at the edge of a read, and losing the whole window to one bad byte would throw away the
/// message somebody is reading this to find.
fn push_lossy(out: &mut String, bytes: &[u8]) {
    out.push_str(&String::from_utf8_lossy(bytes));
}
