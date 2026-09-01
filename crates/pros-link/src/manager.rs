//! Reading from the payload manager's own web service.
//!
//! # What it is for here
//!
//! The manager is the thing that loaded everything else, and it keeps a description of
//! where each payload came from. That description is the same shape as the manifest this
//! project ships, on purpose - so a target that is already configured is already
//! described, and can be read as a source rather than re-entered by hand.
//!
//! # No endpoint paths are written down in this crate
//!
//! Not an omission. Which paths that service answers on has not been measured, and a
//! plausible-looking constant would be a guess wearing the clothes of a fact - the exact
//! failure the sibling projects grade evidence to avoid. A caller passes the path it knows.
//! When the paths are measured they belong with the code that interprets what comes back,
//! one layer up, where the manifest already lives.
//!
//! # A deliberately small subset
//!
//! One method, no bodies, no redirects, no compression, no security layer. The server is a
//! small embedded one on the local network, and the client that talks to it should be
//! readable in a sitting. What it does **not** do quietly is the part that matters: a
//! response it cannot frame is an error rather than a body with the framing left in it.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::wire;

/// Port the manager's web service listens on.
const PORT: u16 = 8084;

/// How long to wait for the connection.
const CONNECT: Duration = Duration::from_secs(4);

/// How long the server may be silent mid-response before it is called dead.
const QUIET: Duration = Duration::from_secs(15);

/// The largest response this will assemble.
///
/// A cap rather than trust: the length arrives from the other end, and a client that
/// believes an arbitrary one has agreed to allocate whatever it is told to. Sixteen
/// megabytes is far above any description of a payload repository and far below anything
/// that hurts.
const CEILING: u64 = 16 * 1024 * 1024;

/// Fetches a path as text.
///
/// # Errors
///
/// [`Error::Refused`] when the manager is not answering - which, note, it does even when
/// the loader beneath it is dead, since it is a separate listener. [`Error::Rejected`] for
/// any status that is not a success, carrying what the server called it.
pub fn get(address: &str, path: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&fetch(address, path)?).into_owned())
}

/// Fetches a path as bytes.
///
/// # Errors
///
/// As [`get`].
pub fn fetch(address: &str, path: &str) -> Result<Vec<u8>> {
    fetch_at(address, PORT, path)
}

/// Fetches from a manager on a port other than the usual one.
///
/// Public for the same reason as [`crate::loader::send_at`].
///
/// # Errors
///
/// As [`get`].
pub fn fetch_at(address: &str, port: u16, path: &str) -> Result<Vec<u8>> {
    let stream = wire::connect(address, port, CONNECT)?;
    stream.set_read_timeout(Some(QUIET))?;
    stream.set_write_timeout(Some(QUIET))?;
    let mut connection = BufReader::new(stream);

    // `Connection: close` asks the server to end the body by ending the conversation,
    // which is the simplest framing there is. Asking is not the same as being obeyed,
    // though, so the reply is framed by what it actually says - see `read_body`.
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {address}:{port}\r\nUser-Agent: pros\r\nConnection: close\r\nAccept: */*\r\n\r\n"
    );
    let socket = connection.get_mut();
    socket.write_all(request.as_bytes())?;
    socket.flush()?;

    let status = read_status(&mut connection, path)?;
    let framing = read_headers(&mut connection, path)?;
    let body = read_body(&mut connection, framing, path)?;

    if !(200..300).contains(&status.0) {
        return Err(Error::Rejected {
            doing: format!("fetching {path}"),
            reply: status.1,
        });
    }
    Ok(body)
}

/// How the body of a response is bounded.
#[derive(Debug, Clone, Copy)]
enum Framing {
    /// A stated number of bytes.
    Length(u64),
    /// A sequence of sized pieces, ending with one of size zero.
    Chunked,
    /// Until the connection closes.
    ///
    /// What `Connection: close` produces, and the only framing available to a server that
    /// does not know the length before it starts.
    UntilClosed,
}

/// Reads the status line, returning the code and the line itself.
///
/// The whole line is carried because the number alone is a worse error message than the
/// number with the server's own words after it.
fn read_status(connection: &mut BufReader<TcpStream>, path: &str) -> Result<(u16, String)> {
    let line = read_line(connection, path)?;
    let code = line
        .split_whitespace()
        .nth(1)
        .and_then(|field| field.parse().ok())
        .ok_or(Error::Unintelligible {
            doing: format!("fetching {path}"),
            said: line.clone(),
        })?;
    Ok((code, line))
}

/// Reads headers until the blank line, keeping only what decides the framing.
fn read_headers(connection: &mut BufReader<TcpStream>, path: &str) -> Result<Framing> {
    let mut framing = Framing::UntilClosed;
    loop {
        let line = read_line(connection, path)?;
        if line.is_empty() {
            return Ok(framing);
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();
        // Chunked wins over a length if both appear, which is what the specification says
        // and also the safer reading: a stated length that disagrees with the chunk sizes
        // would truncate.
        if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
            framing = Framing::Chunked;
        } else if name == "content-length"
            && !matches!(framing, Framing::Chunked)
            && let Ok(length) = value.parse::<u64>()
        {
            framing = Framing::Length(length);
        }
    }
}

/// Reads the body according to how the server framed it.
fn read_body(
    connection: &mut BufReader<TcpStream>,
    framing: Framing,
    path: &str,
) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    match framing {
        Framing::Length(length) => {
            if length > CEILING {
                return Err(Error::Rejected {
                    doing: format!("fetching {path}"),
                    reply: format!(
                        "the server offered {length} bytes, which is more than this will hold"
                    ),
                });
            }
            // Exactly that many, and short counts as a failure. A body that arrives half
            // finished and is returned anyway is a file that parses to something wrong.
            body.resize(usize::try_from(length).unwrap_or(0), 0);
            connection.read_exact(&mut body)?;
        }
        Framing::Chunked => read_chunks(connection, &mut body, path)?,
        Framing::UntilClosed => {
            connection.take(CEILING).read_to_end(&mut body)?;
        }
    }
    Ok(body)
}

/// Reassembles a chunked body.
///
/// # Why this is implemented rather than refused
///
/// A small server that streams a generated description does not know its length in advance
/// and has no other way to send it. The alternative to reading it properly is returning the
/// chunk headers inside the data, where they look like content and corrupt whatever parses
/// it - success reported for a body that is wrong, which is the defect class this project
/// keeps meeting.
fn read_chunks(
    connection: &mut BufReader<TcpStream>,
    body: &mut Vec<u8>,
    path: &str,
) -> Result<()> {
    loop {
        let header = read_line(connection, path)?;
        // A chunk size may be followed by extensions after a semicolon, which nothing here
        // needs but which must not be parsed as part of the number.
        let size_field = header.split(';').next().unwrap_or_default().trim();
        let size = u64::from_str_radix(size_field, 16).map_err(|_| Error::Unintelligible {
            doing: format!("fetching {path}"),
            said: format!("expected a chunk size, got {header:?}"),
        })?;
        if size == 0 {
            return Ok(());
        }
        let total = body.len() as u64 + size;
        if total > CEILING {
            return Err(Error::Rejected {
                doing: format!("fetching {path}"),
                reply: format!("the response passed {CEILING} bytes and was abandoned"),
            });
        }
        let start = body.len();
        body.resize(start + usize::try_from(size).unwrap_or(0), 0);
        connection.read_exact(&mut body[start..])?;
        // Each chunk is followed by its own line ending, which is framing rather than data.
        read_line(connection, path)?;
    }
}

/// Reads one line, treating a closed connection mid-header as an answer rather than an end.
fn read_line(connection: &mut BufReader<TcpStream>, path: &str) -> Result<String> {
    let mut line = String::new();
    if connection.read_line(&mut line)? == 0 {
        return Err(Error::Unintelligible {
            doing: format!("fetching {path}"),
            said: "the connection closed part-way through the response".to_owned(),
        });
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}
