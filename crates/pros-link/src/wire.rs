//! Connecting, and reading from things that do not say when they have finished.
//!
//! None of the services is request-response. The log streams without end, so a reader
//! stops on a chosen window; the shell has no framing, so a reader stops when nothing more
//! arrives; the loader may not answer at all. One helper per rule keeps the rule visible at
//! the call site.

use std::io::Read as _;
use std::net::{TcpStream, ToSocketAddrs as _};
use std::time::{Duration, Instant};

use crate::error::{Error, Result};

/// How long a read blocks before the loop checks its own clock again.
///
/// Short enough to honour a deadline promptly, long enough not to spin.
const POLL: Duration = Duration::from_millis(250);

/// Opens a connection, telling an unresolved name apart from a refusal.
///
/// An unresolved name is a typo to fix; a refusing port is usually a payload not loaded.
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
            tracing::debug!(%address, port, "address did not resolve");
            Error::Unresolved {
                address: address.to_owned(),
            }
        })?;
    TcpStream::connect_timeout(&addr, timeout)
        .inspect(|_| tracing::trace!(%addr, port, took = ?started.elapsed(), "connected"))
        .map_err(|_| {
            let took = started.elapsed();
            // `debug`, not `warn`: a shut port is the ordinary answer for a service that is
            // not running, and a check asks in order to find that out.
            tracing::debug!(%addr, port, ?took, "connection refused");
            Error::Refused { port, took }
        })
}

/// Reads for a fixed window, whatever arrives.
///
/// For a stream with no end. Returning nothing is a result, not a failure: a quiet log is a
/// fact about the target.
///
/// # Errors
///
/// Propagates a socket failure other than a timeout, which is how a quiet connection reads.
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
/// For an interface with no framing, where a response has finished when it stops.
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
/// A banner and prompt arrive unprompted and a command typed over them is garbled. A closed
/// stream or a read timeout ends the drain.
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
/// A read can end mid multi-byte sequence; one bad byte must not lose the whole window.
fn push_lossy(out: &mut String, bytes: &[u8]) {
    out.push_str(&String::from_utf8_lossy(bytes));
}
