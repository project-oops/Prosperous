//! Holding one file out for the target to fetch, then stopping.
//!
//! The only listener in this project. `pkg_install` takes a url and fetches it itself; a path
//! on the target's own disk gives the same empty answer as a missing file (measured on a
//! target), and nothing on the target serves files, so the package is served from here.
//!
//! A handover, not a file server: every request gets the same one file whatever path it names,
//! so there is no path handling to get wrong; it stops when dropped or after a deadline; and it
//! binds the interface a connection to the target actually went out from, never `0.0.0.0` or a
//! guessed address.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a handover waits to be taken before giving up.
///
/// Generous: the target fetches the whole package before it answers, and a large one over a
/// slow link takes minutes.
const PATIENCE: Duration = Duration::from_mins(10);

/// What the target should ask for, when it asks for anything.
///
/// The name is in the url only so a log shows what went across; nothing dispatches on it.
fn url_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || "file".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// A file being held out for one taker.
#[derive(Debug)]
pub struct Handover {
    /// The url to give the target.
    pub url: String,
    /// How many times the file has been handed over.
    taken: Arc<Mutex<usize>>,
    /// The request line and range/user-agent headers of each request, in order, so repeated
    /// fetches can be told apart from a count alone.
    asked: Arc<Mutex<Vec<String>>>,
    /// Set to stop the thread waiting for a connection.
    stopping: Arc<AtomicBool>,
    /// Where to connect to wake the accept loop so it sees `stopping`.
    address: SocketAddr,
}

impl Handover {
    /// Starts holding `file` out on the interface that reaches `target`.
    ///
    /// This binds the interface that routes to the target, which is unreachable from the
    /// target when this machine is behind NAT (WSL2's default networking gives `172.24.x.x`).
    /// The failure is quiet: the target never sends a request, and [`Handover::taken`] stays
    /// zero. Run the sender on the host, or with `networkingMode=mirrored`.
    ///
    /// # Errors
    ///
    /// When the file cannot be read, the target cannot be reached to work out which interface
    /// faces it, or nothing will bind.
    pub fn offer_to(file: &Path, target: &str) -> Result<Self, String> {
        let bytes = std::fs::read(file).map_err(|why| format!("{}: {why}", file.display()))?;
        let mine = facing(target)?;

        // Port zero: the system picks a free one, so nothing on this machine collides.
        let listener = TcpListener::bind((mine, 0)).map_err(|why| why.to_string())?;
        let address = listener.local_addr().map_err(|why| why.to_string())?;
        let url = format!("http://{address}/{}", url_name(file));

        let taken = Arc::new(Mutex::new(0));
        let asked: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let counted = Arc::clone(&taken);
        let recording = Arc::clone(&asked);
        let stop = Arc::clone(&stopping);

        std::thread::spawn(move || {
            let until = Instant::now() + PATIENCE;
            for incoming in listener.incoming() {
                if stop.load(Ordering::Relaxed) || Instant::now() > until {
                    break;
                }
                let Ok(stream) = incoming else { break };
                if let Ok(request) = hand_over(stream, &bytes) {
                    if let Ok(mut count) = counted.lock() {
                        *count += 1;
                    }
                    if let Ok(mut seen) = recording.lock() {
                        seen.push(request);
                    }
                }
            }
        });

        Ok(Self {
            url,
            taken,
            asked,
            stopping,
            address,
        })
    }

    /// How many times it has been fetched.
    ///
    /// Zero after an install means the target never came for the file, which the target's
    /// reply alone cannot distinguish from a package it fetched and rejected.
    #[must_use]
    pub fn taken(&self) -> usize {
        self.taken.lock().map(|count| *count).unwrap_or_default()
    }

    /// What was asked for, in order.
    ///
    /// The request line and any header that would explain repeated fetches, recorded rather
    /// than parsed.
    #[must_use]
    pub fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }
}

impl Drop for Handover {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
        // The thread is blocked in accept and sees the flag only when something connects.
        let _ = TcpStream::connect(self.address);
    }
}

/// What part of the file a request asked for.
///
/// Measured on a target: its package fetcher (`libhttp/12.40`) sends `Range: bytes=0-65535`,
/// then `bytes=65536-524287`, and so on. Answered with the whole file and a `200`, it retries
/// the first chunk repeatedly.
fn range_in(request: &str, len: usize) -> Option<(usize, usize)> {
    let at = request.to_ascii_lowercase().find("range: bytes=")?;
    let spec = request.get(at + "range: bytes=".len()..)?;
    let spec = spec.split(['|', '\r', '\n']).next()?.trim();
    let (from, to) = spec.split_once('-')?;

    let from: usize = from.trim().parse().ok()?;
    // An open-ended range, `bytes=N-`, means the rest of the file.
    let to: usize = match to.trim() {
        "" => len.saturating_sub(1),
        end => end.parse().ok()?,
    };
    // Clamped rather than refused: clients round the last chunk up past the end.
    let to = to.min(len.saturating_sub(1));
    (from <= to && from < len).then_some((from, to))
}

/// Reads the request, keeps a note of it, and sends what it asked for.
///
/// The request is read in full because a client still sending when the reply arrives can see
/// a reset instead of the response.
///
/// Only the range is acted on. The path is ignored: a range is an offset into the one file and
/// cannot name another.
fn hand_over(mut stream: TcpStream, bytes: &[u8]) -> std::io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;

    let mut reading = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    let mut request = String::new();
    let mut headers = String::new();
    while reading.read_line(&mut line)? > 0 {
        if line == "\r\n" || line == "\n" {
            break;
        }
        let trimmed = line.trim_end().to_owned();
        let lower = trimmed.to_ascii_lowercase();
        headers.push_str(&lower);
        headers.push('\n');
        // The request line, plus the two headers worth reading back afterwards.
        if request.is_empty() || lower.starts_with("range:") || lower.starts_with("user-agent:") {
            if !request.is_empty() {
                request.push_str(" | ");
            }
            request.push_str(&trimmed);
        }
        line.clear();
    }

    let whole = bytes.len();
    if let Some((from, to)) = range_in(&headers, whole) {
        let part = bytes.get(from..=to).unwrap_or_default();
        write!(
            stream,
            "HTTP/1.1 206 Partial Content\r\n\
             Content-Type: application/octet-stream\r\n\
             Content-Range: bytes {from}-{to}/{whole}\r\n\
             Content-Length: {}\r\n\
             Accept-Ranges: bytes\r\n\
             Connection: close\r\n\
             \r\n",
            part.len()
        )?;
        stream.write_all(part)?;
    } else {
        write!(
            stream,
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/octet-stream\r\n\
             Content-Length: {whole}\r\n\
             Accept-Ranges: bytes\r\n\
             Connection: close\r\n\
             \r\n"
        )?;
        stream.write_all(bytes)?;
    }
    stream.flush()?;
    let _ = stream.shutdown(Shutdown::Write);
    Ok(request)
}

/// Which of this machine's addresses faces the target.
///
/// Found by connecting to it and asking the socket which local address it went out from. A
/// machine with virtual adapters and bridges has several addresses, most unroutable from the
/// target; this one is demonstrably reachable.
fn facing(target: &str) -> Result<std::net::IpAddr, String> {
    // The file service, which every target this program talks to runs.
    let to = if target.contains(':') {
        target.to_owned()
    } else {
        format!("{target}:2121")
    };
    let probe = TcpStream::connect_timeout(
        &to.parse::<SocketAddr>()
            .or_else(|_| resolve(&to))
            .map_err(|why| format!("{to}: {why}"))?,
        Duration::from_secs(6),
    )
    .map_err(|why| format!("could not reach {to} to see which way it is: {why}"))?;
    let mine = probe.local_addr().map_err(|why| why.to_string())?;
    Ok(mine.ip())
}

/// Turns a name into an address, taking the first that answers.
fn resolve(what: &str) -> Result<SocketAddr, String> {
    use std::net::ToSocketAddrs as _;
    what.to_socket_addrs()
        .map_err(|why| why.to_string())?
        .next()
        .ok_or_else(|| "no address".to_owned())
}

/// Holds a file out for a target to fetch, as a free function.
///
/// # Errors
///
/// As [`crate::handover::Handover::offer_to`].
pub fn offer_to(file: &Path, target: &str) -> Result<Handover, String> {
    Handover::offer_to(file, target)
}

/// Where a package would be put for the target to fetch it.
#[must_use]
pub fn staging() -> Option<PathBuf> {
    crate::target::cache_directory().map(|dir| dir.join("packages"))
}

#[cfg(test)]
mod tests {
    use super::{Handover, url_name};
    use std::io::{Read as _, Write as _};
    use std::path::Path;

    /// The url names the file, so a log says what went across.
    #[test]
    fn the_url_says_which_file_it_is() {
        assert_eq!(url_name(Path::new("/a/b/thing.pkg")), "thing.pkg");
    }

    /// Every request gets the same file whatever path it names, so no path can escape.
    #[test]
    fn whatever_is_asked_for_the_one_file_comes_back() {
        let file = std::env::temp_dir().join("prosperous-handover.bin");
        std::fs::write(&file, b"the package").expect("writes");

        // Facing itself: the loopback is the interface that reaches a listener on it.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let at = listener.local_addr().expect("has an address");
        let offered = Handover::offer_to(&file, &at.to_string()).expect("offers");
        drop(listener);

        for asked in ["/thing.pkg", "/../../etc/passwd", "/"] {
            let mut stream = std::net::TcpStream::connect(
                offered
                    .url
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .expect("host"),
            )
            .expect("connects");
            write!(stream, "GET {asked} HTTP/1.1\r\nHost: x\r\n\r\n").expect("asks");
            let mut said = Vec::new();
            stream.read_to_end(&mut said).expect("answers");

            let text = String::from_utf8_lossy(&said);
            assert!(text.starts_with("HTTP/1.1 200"), "{asked}: {text}");
            assert!(
                said.ends_with(b"the package"),
                "{asked} got something other than the one file"
            );
        }
        assert_eq!(offered.taken(), 3, "each fetch should have been counted");

        let _ = std::fs::remove_file(&file);
    }

    /// Nothing is listening once the handover is dropped.
    #[test]
    fn it_stops_when_it_is_let_go() {
        let file = std::env::temp_dir().join("prosperous-handover-stop.bin");
        std::fs::write(&file, b"x").expect("writes");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let at = listener.local_addr().expect("has an address");
        let offered = Handover::offer_to(&file, &at.to_string()).expect("offers");
        drop(listener);

        let host = offered
            .url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .expect("host")
            .to_owned();
        drop(offered);

        // The accept loop is woken by its own knock, and the port goes.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let after = std::net::TcpStream::connect(&host);
        if let Ok(mut open) = after {
            let mut said = Vec::new();
            let _ = open.read_to_end(&mut said);
            assert!(
                said.is_empty(),
                "something still answered after the handover was dropped"
            );
        }

        let _ = std::fs::remove_file(&file);
    }

    /// An unreachable target is an error, not a listener nothing can fetch from.
    #[test]
    fn a_target_that_cannot_be_reached_is_not_served_to() {
        let file = std::env::temp_dir().join("prosperous-handover-nowhere.bin");
        std::fs::write(&file, b"x").expect("writes");

        // Port 9 discards; nothing accepts on it here.
        let refused = Handover::offer_to(&file, "127.0.0.1:9");
        assert!(refused.is_err(), "it should not have offered");

        let _ = std::fs::remove_file(&file);
    }

    /// The ranges a target sends are parsed.
    #[test]
    fn the_range_a_target_asks_for_is_understood() {
        let asked = "get /thing.pkg http/1.1\nrange: bytes=0-65535\nuser-agent: libhttp/12.40\n";
        assert_eq!(super::range_in(asked, 1_000_000), Some((0, 65_535)));

        let next = "range: bytes=65536-524287\n";
        assert_eq!(super::range_in(next, 1_000_000), Some((65_536, 524_287)));
    }

    /// An open-ended range is the rest of the file.
    #[test]
    fn a_range_with_no_end_means_the_rest() {
        assert_eq!(
            super::range_in("range: bytes=900-\n", 1000),
            Some((900, 999))
        );
    }

    /// A range past the end is clamped, not refused.
    #[test]
    fn a_range_running_past_the_end_is_trimmed_to_it() {
        assert_eq!(
            super::range_in("range: bytes=990-9999\n", 1000),
            Some((990, 999))
        );
    }

    /// A request with no range asks for the whole file.
    #[test]
    fn a_request_without_a_range_asks_for_everything() {
        assert_eq!(super::range_in("get / http/1.1\nhost: x\n", 1000), None);
    }

    /// A range that starts past the end, or is backwards, is not a range.
    #[test]
    fn a_range_that_cannot_be_satisfied_is_not_one() {
        assert_eq!(super::range_in("range: bytes=2000-3000\n", 1000), None);
        assert_eq!(super::range_in("range: bytes=500-100\n", 1000), None);
        assert_eq!(super::range_in("range: bytes=abc-def\n", 1000), None);
    }
}
