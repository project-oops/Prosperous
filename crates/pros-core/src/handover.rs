//! Holding one file out for the target to take, and stopping.
//!
//! # Why this program listens at all, having said it would not
//!
//! Everything else here connects. This is the one thing that waits to be connected to, and it
//! exists because of a measurement rather than a preference.
//!
//! `pkg_install` takes a url and fetches it itself. A path on the target's own disk does not
//! work - measured, with a real package in `/data/pkg`, producing the identical empty answer a
//! missing file gives. Nor is there anything already running on the target that could hand it
//! one: the payload manager's web server has no file route and restricts what it will touch to
//! payload extensions under its own directories, and the homebrew server was not running.
//!
//! So either the package is served from here, or packages are not installable. This module was
//! written after that was established and not before.
//!
//! # A handover, not a file server
//!
//! The difference is the whole design.
//!
//! - **One file.** Not a directory, not a root. There is no path handling, so there is no path
//!   traversal to get wrong - every request gets the same bytes whatever it asks for.
//! - **It stops.** After the file has been taken, or after a deadline, whichever is first.
//!   Something that stays listening is a service, and this project does not run one.
//! - **It binds to the one interface the target can reach**, discovered by connecting to the
//!   target and looking at which address that connection went out from. Not guessed, not
//!   `0.0.0.0`, not enumerated - **asked**, in the only way that cannot be wrong.
//!
//! That last point matters more than it sounds. A machine with a virtual adapter, a container
//! bridge and a wireless card has several addresses, most of which the target cannot route to.
//! Picking one and building a url out of it produces a link that looks right and times out.

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a handover waits to be taken before giving up.
///
/// **Generous, because the target is doing the work.** It fetches the whole package before it
/// says anything, and a large one over a slow link is minutes. A window that gave up first
/// would report a failure over an install that was going fine.
const PATIENCE: Duration = Duration::from_mins(10);

/// What the target should ask for, when it asks for anything.
///
/// The name is in the url only so a person reading a log can see what went across. Nothing
/// dispatches on it.
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
    /// The first line of each request, in order.
    ///
    /// **Kept because the traffic did not make sense.** One install of a sixty-two megabyte
    /// package produced nine complete fetches. Whether that is ranges, retries, or something
    /// else cannot be told from a count - so what was asked is recorded, and the question
    /// becomes a fact rather than a guess about a number.
    asked: Arc<Mutex<Vec<String>>>,
    /// Set to stop the thread waiting for a connection.
    stopping: Arc<AtomicBool>,
    /// A connection to itself is how the accept loop is woken to notice that.
    address: SocketAddr,
}

impl Handover {
    /// Starts holding `file` out on the interface that reaches `target`.
    ///
    /// # The server has to run somewhere the target can reach back to
    ///
    /// This binds the interface that routes to the target, which is the right answer for the
    /// machine it runs on - and the wrong one when that machine is behind NAT. Under WSL2's
    /// default networking the chosen address is `172.24.x.x`: correct from inside WSL, and
    /// unreachable from a console on the LAN.
    ///
    /// The failure is quiet, which is the part worth warning about. The target never sends a
    /// request, so there is nothing in a log and no error from the loader - only
    /// [`Handover::taken`] returning zero. **`taken() == 0` means the target never came**, so
    /// whatever was being installed was never judged and nothing about it is implicated.
    ///
    /// Run the sender where the target can reach it (on Windows rather than in WSL, or with
    /// `networkingMode=mirrored`). Measured against a real console: from WSL, zero fetches; from
    /// the host, the same package fetched repeatedly with `libhttp/12.40 (PlayStation 5)` range
    /// requests. (obSCEne hardware session, 2026-08-27)
    /// # Errors
    ///
    /// When the file cannot be read, the target cannot be reached to work out which interface
    /// faces it, or nothing will bind.
    pub fn offer_to(file: &Path, target: &str) -> Result<Self, String> {
        let bytes = std::fs::read(file).map_err(|why| format!("{}: {why}", file.display()))?;
        let mine = facing(target)?;

        // Port zero: the system picks one that is free. A fixed port is one more thing to
        // collide with something else on this machine.
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
    /// **Zero after an install is the useful finding.** It means the target never came for the
    /// file, which is a different problem from a package it fetched and disliked - and the two
    /// are indistinguishable from the target's reply alone.
    #[must_use]
    pub fn taken(&self) -> usize {
        self.taken.lock().map(|count| *count).unwrap_or_default()
    }

    /// What was asked for, in order.
    ///
    /// The request line and any header that would explain repeated fetches. Recorded rather
    /// than parsed: **nothing here dispatches on a request**, and the moment it did there
    /// would be a path to get wrong.
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
        // The thread is blocked in accept and will not look at the flag until something
        // connects. Knocking is how it is woken to notice - the same trick the fake target
        // uses, and for the same reason.
        let _ = TcpStream::connect(self.address);
    }
}

/// What part of the file a request asked for.
///
/// **Measured, not assumed.** A target fetching a package sends
/// `Range: bytes=0-65535`, then `bytes=65536-524287`, in sixty-four kilobyte steps, from
/// `libhttp/12.40 (PlayStation 5)`.
///
/// Answering all of those with the whole file and a `200` made it ask for the first chunk
/// **eight times** before continuing - so this is not only nine times the traffic, it is a
/// client retrying because it did not get what it asked for.
fn range_in(request: &str, len: usize) -> Option<(usize, usize)> {
    let at = request.to_ascii_lowercase().find("range: bytes=")?;
    let spec = request.get(at + "range: bytes=".len()..)?;
    let spec = spec.split(['|', '\r', '\n']).next()?.trim();
    let (from, to) = spec.split_once('-')?;

    let from: usize = from.trim().parse().ok()?;
    // An open-ended range - `bytes=N-` - means the rest of the file.
    let to: usize = match to.trim() {
        "" => len.saturating_sub(1),
        end => end.parse().ok()?,
    };
    // A range past the end is clamped rather than refused: the last chunk of a file is
    // routinely asked for by a client that rounded up.
    let to = to.min(len.saturating_sub(1));
    (from <= to && from < len).then_some((from, to))
}

/// Reads the request, keeps a note of it, and sends what it asked for.
///
/// **The request is read rather than not read at all**: a client that has not finished sending
/// when the reply arrives can see a reset instead of the response, and the symptom is a fetch
/// that fails for no reason anybody can see.
///
/// The only thing acted on is the range. **The path is still ignored entirely** - there is one
/// file and every request gets it, which is why no path can be asked for that this could get
/// wrong. A range is an offset into that one file and cannot name another.
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
/// Found by opening a connection to it and asking the socket where it went out from. A machine
/// with a virtual adapter, a container bridge and a wireless card has several addresses and
/// most of them the target cannot route to; **this is the only one it has demonstrably
/// reached**, because the connection proving it is in hand.
fn facing(target: &str) -> Result<std::net::IpAddr, String> {
    // The file service, because it is the one a target running any of this will have.
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

    /// **Every request gets the same bytes**, whatever it asks for.
    ///
    /// That is what makes path traversal impossible here rather than merely guarded against:
    /// there is no path handling to get wrong.
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

    /// **Nothing is listening once it is dropped.**
    ///
    /// A handover that outlived its install would be a file quietly available on the network
    /// for as long as the window stayed open.
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

    /// A target that cannot be reached says so, rather than binding something nothing can
    /// fetch from.
    #[test]
    fn a_target_that_cannot_be_reached_is_not_served_to() {
        let file = std::env::temp_dir().join("prosperous-handover-nowhere.bin");
        std::fs::write(&file, b"x").expect("writes");

        // Port 9 discards; nothing accepts on it here.
        let refused = Handover::offer_to(&file, "127.0.0.1:9");
        assert!(refused.is_err(), "it should not have offered");

        let _ = std::fs::remove_file(&file);
    }

    /// **The range a target actually sends**, read back.
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

    /// **A range past the end is clamped, not refused.** The last chunk of a file is routinely
    /// asked for by a client that rounded up, and refusing it would fail the final fetch of
    /// every transfer.
    #[test]
    fn a_range_running_past_the_end_is_trimmed_to_it() {
        assert_eq!(
            super::range_in("range: bytes=990-9999\n", 1000),
            Some((990, 999))
        );
    }

    /// A request with no range at all gets the whole file, which is what the `None` says.
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
