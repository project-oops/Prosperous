//! Reading from the payload manager's own web service.
//!
//! The manager loaded everything else and describes where each payload came from, in the
//! same shape as this project's manifest, so a configured target can be read as a source.
//!
//! No endpoint path is a constant here: the caller passes the path it knows, and measured
//! paths belong in `pros-core` beside the code that interprets the reply.
//!
//! The client is a small HTTP/1.1 subset: `GET` only, no redirects, compression or TLS. A
//! response it cannot frame is an error, never a body with the framing left in.

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
/// The length comes from the server, so it is capped rather than trusted. Sixteen megabytes
/// is far above any payload repository description.
const CEILING: u64 = 16 * 1024 * 1024;

/// Fetches a path as text.
///
/// # Errors
///
/// [`Error::Refused`] when the manager is not answering; it is a separate listener, so it
/// can answer while the loader is down. [`Error::Rejected`] for any non-success status,
/// carrying the server's status line.
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

    // `Connection: close` asks the server to end the body by closing, but the reply is
    // framed by what its headers say. See `read_body`.
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
    /// Until the connection closes, as `Connection: close` produces.
    UntilClosed,
}

/// Reads the status line, returning the code and the line itself.
///
/// The line carries the server's own reason phrase for the error message.
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
        // Chunked wins over a length if both appear, as the specification says.
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
            // A short body is a failure, never returned as if complete.
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
/// A server streaming a generated description does not know its length in advance.
fn read_chunks(
    connection: &mut BufReader<TcpStream>,
    body: &mut Vec<u8>,
    path: &str,
) -> Result<()> {
    loop {
        let header = read_line(connection, path)?;
        // Chunk extensions after a semicolon are not part of the size.
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
        // The line ending after each chunk is framing, not data.
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
