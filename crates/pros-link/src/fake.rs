//! A target that is not a target, for tests.
//!
//! It ships outside `#[cfg(test)]` so every consumer tests against the same fake instead of
//! building its own; it is std-only and costs nothing to ignore.
//!
//! It fakes the awkward parts: a stream with no end, a server with no framing, a loader that
//! may not answer, a file service whose transfers use a second connection, and a web service
//! with chunked bodies. Whether the real target agrees is for a registered target to show.

use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

/// What a fake file service holds, and where anything written to it lands.
///
/// Shared with whoever started the fake, so a test checks the contents rather than the
/// reply. Names are full paths matched exactly; there is no directory tree.
#[derive(Debug, Clone, Default)]
pub struct Store(Held);

/// What a [`Store`] keeps, named so the type is readable where it appears.
type Held = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

impl Store {
    /// A store holding these files.
    #[must_use]
    pub fn new(files: &[(&str, &[u8])]) -> Self {
        Self(Arc::new(Mutex::new(
            files
                .iter()
                .map(|(name, bytes)| ((*name).to_owned(), (*bytes).to_vec()))
                .collect(),
        )))
    }

    /// What is stored under a name, if anything.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.iter()
            .find(|(stored, _)| stored == name)
            .map(|(_, bytes)| bytes.clone())
    }

    /// Writes a file, replacing anything already under that name.
    pub fn put(&self, name: &str, bytes: Vec<u8>) {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|(stored, _)| stored != name);
        held.push((name.to_owned(), bytes));
    }

    /// Every name held, in the order they were added.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        let held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        held.iter().map(|(name, _)| name.clone()).collect()
    }
}

/// How a fake service behaves once something connects.
#[derive(Debug, Clone)]
pub enum Behaviour {
    /// Say nothing and hold the connection open: the quiet log, the loader that does not echo.
    Silent,
    /// Send this once, then hold the connection open.
    ///
    /// The real services give no EOF to stop on, so neither does this.
    Says(String),
    /// Send this repeatedly until the client goes away, like the log.
    Streams(String),
    /// The shell: a banner, then a reply per line read, with no end-of-reply marker.
    Shell {
        /// Sent unprompted on connect, before anything is typed.
        banner: String,
        /// Sent after each line arrives.
        reply: String,
    },
    /// Accept everything sent, then behave as `then`.
    ///
    /// The loader: a payload arrives before anything comes back, if anything does.
    Accepts {
        /// What to do once the client stops sending.
        then: Box<Behaviour>,
    },
    /// A file service, where each transfer uses a second connection on a port the server
    /// names.
    Files {
        /// What it holds, and where a stored file lands.
        contents: Store,
        /// The address it claims when it names a data port.
        ///
        /// Tests set it wrong: a server behind address translation reports the address it
        /// believes it has, so the client must dial the address that already reached it.
        claims: [u8; 4],
        /// Whether it agrees to binary mode; continuing after a refusal would edit the bytes.
        binary: bool,
        /// Whether a `STOR` is acknowledged but keeps nothing.
        ///
        /// A real target with the title mounted, or an overlay that swallows the write,
        /// answers `226` and leaves the old file in place. With this set the fake drains the
        /// data connection, says `226`, and discards what arrived.
        swallows_stores: bool,
    },
    /// A web service answering one request.
    Serves {
        /// The status code to answer with.
        status: u16,
        /// The body to send.
        body: String,
        /// Whether to send it in sized pieces with no overall length.
        chunked: bool,
    },
}

/// A fake service listening on a real port on the loopback interface.
///
/// Dropping it stops the listener.
#[derive(Debug)]
pub struct Fake {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Fake {
    /// Starts a fake on an operating-system-chosen port.
    ///
    /// An OS-chosen port lets tests run in parallel with each other and with a real target.
    ///
    /// # Errors
    ///
    /// Propagates a failure to bind.
    pub fn start(behaviour: Behaviour) -> std::io::Result<Self> {
        Self::start_at(0, behaviour)
    }

    /// Starts a fake on a port of the caller's choosing, or any port when given zero.
    ///
    /// A fixed port lets it stand in end to end for code that uses a target's known ports.
    ///
    /// # Errors
    ///
    /// Propagates a failure to bind, usually because something else holds a named port.
    pub fn start_at(port: u16, behaviour: Behaviour) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, waiting) = mpsc::channel();
        let thread = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve(&listener, &behaviour, &stop, &ready))
        };

        // Wait for the thread to run: a connection to the bound socket sits in the backlog, so
        // a fake not yet serving looks silent. A spawned thread was measured taking 230 ms to
        // be scheduled. Bounded, so a stuck thread cannot hang a test.
        let _ = waiting.recv_timeout(Duration::from_secs(5));

        Ok(Self {
            port,
            stop,
            thread: Some(thread),
        })
    }

    /// The port it is listening on.
    #[must_use]
    pub const fn port(&self) -> u16 {
        self.port
    }

    /// The address a client should use.
    #[must_use]
    pub fn address(&self) -> &'static str {
        "127.0.0.1"
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Knock: a blocking accept cannot see the flag, so one throwaway connection wakes it.
        // A polling listener instead was measured taking up to 487 ms to notice a
        // connection.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Accepts connections until told to stop.
///
/// `ready` is signalled once before the first accept, when the thread is running.
fn serve(
    listener: &TcpListener,
    behaviour: &Behaviour,
    stop: &Arc<AtomicBool>,
    ready: &mpsc::Sender<()>,
) {
    let _ = ready.send(());
    loop {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        // Checked after the accept: this connection may be the knock from `Drop`.
        if ended(stop) {
            return;
        }
        handle(stream, behaviour, stop);
    }
}

/// Plays one behaviour at one client.
fn handle(mut stream: TcpStream, behaviour: &Behaviour, stop: &Arc<AtomicBool>) {
    // Blocking with a short timeout is what the reads below assume. An accepted stream can
    // inherit a listener's non-blocking mode, so the mode is set explicitly.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    match behaviour {
        Behaviour::Silent => hold(&stream, stop),
        Behaviour::Says(text) => {
            let _ = stream.write_all(text.as_bytes());
            let _ = stream.flush();
            hold(&stream, stop);
        }
        Behaviour::Streams(text) => {
            while !ended(stop) {
                if stream.write_all(text.as_bytes()).is_err() {
                    return;
                }
                let _ = stream.flush();
                thread::sleep(Duration::from_millis(20));
            }
        }
        Behaviour::Shell { banner, reply } => {
            let _ = stream.write_all(banner.as_bytes());
            let _ = stream.flush();
            let mut buffer = [0_u8; 1024];
            while !ended(stop) {
                match stream.read(&mut buffer) {
                    Ok(0) => return,
                    Ok(_) => {
                        let _ = stream.write_all(reply.as_bytes());
                        let _ = stream.flush();
                    }
                    Err(_) => {}
                }
            }
        }
        Behaviour::Accepts { then } => {
            // Drain the payload, then switch, as a real loader reads it all before it runs.
            // One quiet read after some bytes ends the drain, so a caller on a short window
            // still hears the answer; silence before any bytes is a client not started yet.
            let mut buffer = [0_u8; 4096];
            let mut arrived = false;
            let mut waited = 0_u32;
            while !ended(stop) {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => arrived = true,
                    // Bounds a client that connects and never sends.
                    Err(_) if arrived || waited >= 40 => break,
                    Err(_) => waited = waited.saturating_add(1),
                }
            }
            handle(stream, then, stop);
        }
        Behaviour::Files {
            contents,
            claims,
            binary,
            swallows_stores,
        } => serve_files(stream, contents, *claims, *binary, *swallows_stores, stop),
        Behaviour::Serves {
            status,
            body,
            chunked,
        } => serve_web(stream, *status, body, *chunked),
    }
}

/// Plays a file service at one client until it says goodbye.
fn serve_files(
    stream: TcpStream,
    contents: &Store,
    claims: [u8; 4],
    binary: bool,
    swallows_stores: bool,
    stop: &Arc<AtomicBool>,
) {
    // Long enough that a pause between commands is not taken for a departed client.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let Ok(writing) = stream.try_clone() else {
        return;
    };
    let mut reading = BufReader::new(stream);
    let mut writing = writing;
    let mut say = |writing: &mut TcpStream, line: &str| {
        let _ = writing.write_all(line.as_bytes());
        let _ = writing.write_all(b"\r\n");
        let _ = writing.flush();
    };

    say(&mut writing, "220 a target that is not one");
    // The data listener announced by the last `PASV`.
    let mut pending: Option<TcpListener> = None;

    loop {
        if ended(stop) {
            return;
        }
        let mut line = String::new();
        match reading.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = line.trim_end_matches(['\r', '\n']).to_owned();
        let (verb, argument) = match line.split_once(' ') {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.to_owned()),
            None => (line.to_ascii_uppercase(), String::new()),
        };

        match verb.as_str() {
            "USER" => say(&mut writing, "331 any password will do"),
            "PASS" => say(&mut writing, "230 logged in"),
            "TYPE" => {
                if binary {
                    say(&mut writing, "200 binary it is");
                } else {
                    say(&mut writing, "504 text mode only");
                }
            }
            "PASV" => match TcpListener::bind("127.0.0.1:0") {
                Ok(listener) => {
                    let port = listener.local_addr().map_or(0, |addr| addr.port());
                    let [a, b, c, d] = claims;
                    let (high, low) = (port / 256, port % 256);
                    say(
                        &mut writing,
                        &format!("227 Entering Passive Mode ({a},{b},{c},{d},{high},{low})"),
                    );
                    pending = Some(listener);
                }
                Err(_) => say(&mut writing, "425 no data port"),
            },
            "LIST" | "RETR" | "STOR" => {
                let Some(listener) = pending.take() else {
                    say(&mut writing, "425 ask for a data port first");
                    continue;
                };
                transfer(
                    &listener,
                    &mut writing,
                    &verb,
                    &argument,
                    contents,
                    swallows_stores,
                    &mut say,
                );
            }
            // Lets a client confirm a `STOR` landed: `213 <n>`, or 550 when absent.
            "SIZE" => match contents.get(&argument) {
                Some(bytes) => say(&mut writing, &format!("213 {}", bytes.len())),
                None => say(&mut writing, "550 no such file"),
            },
            // The target's ftpsrv answers `MKD` with `226 Directory created`, not the standard
            // `257`; a client reads it as success. There is no tree here, so it only acks.
            "MKD" => say(&mut writing, "226 Directory created"),
            "QUIT" => {
                say(&mut writing, "221 goodbye");
                return;
            }
            _ => say(&mut writing, "500 unknown command"),
        }
    }
}

/// Does one transfer on the data connection the client is expected to have dialled.
fn transfer(
    listener: &TcpListener,
    writing: &mut TcpStream,
    verb: &str,
    argument: &str,
    contents: &Store,
    swallows_stores: bool,
    say: &mut impl FnMut(&mut TcpStream, &str),
) {
    // A missing file is refused before the transfer starts, as a real server does, after the
    // client has already opened a data connection.
    if verb == "RETR" && contents.get(argument).is_none() {
        say(writing, "550 no such file");
        return;
    }
    say(writing, "150 opening data connection");
    let Ok((mut data, _)) = listener.accept() else {
        say(writing, "425 nobody connected");
        return;
    };
    match verb {
        "LIST" => {
            // A header line and a non-entry line, which a client must skip.
            let mut listing = String::from("total 2\nthis line is not a listing entry\n");
            // Only the files directly in the requested folder, by basename: the keys under
            // `<argument>/` with no further slash.
            let prefix = format!("{}/", argument.trim_end_matches('/'));
            for name in contents.names() {
                let Some(base) = name
                    .strip_prefix(&prefix)
                    .filter(|rest| !rest.contains('/'))
                else {
                    continue;
                };
                let size = contents.get(&name).map_or(0, |bytes| bytes.len());
                let _ = writeln!(
                    listing,
                    "-rw-r--r--   1 root root {size:>8} Aug 25 12:00 {base}"
                );
            }
            listing.push_str("drwxr-xr-x   2 root root        0 Aug 25 12:00 a directory\n");
            let _ = data.write_all(listing.as_bytes());
        }
        "RETR" => {
            if let Some(bytes) = contents.get(argument) {
                let _ = data.write_all(&bytes);
            }
        }
        _ => {
            let mut bytes = Vec::new();
            let _ = data.read_to_end(&mut bytes);
            // A swallowed write leaves the old file, or none, for a size check to find.
            if !swallows_stores {
                contents.put(argument, bytes);
            }
        }
    }
    drop(data);
    say(writing, "226 transfer complete");
}

/// Answers one web request and closes.
fn serve_web(mut stream: TcpStream, status: u16, body: &str, chunked: bool) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    // Drain the request to its blank line before answering.
    let Ok(reading) = stream.try_clone() else {
        return;
    };
    let mut reading = BufReader::new(reading);
    loop {
        let mut line = String::new();
        match reading.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if line.trim().is_empty() {
            break;
        }
    }

    let mut response = format!("HTTP/1.1 {status} {}\r\n", reason(status));
    if chunked {
        response.push_str("Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
        // Two pieces, so a reader that stops after the first fails.
        let (first, second) = body.split_at(body.len() / 2);
        for piece in [first, second] {
            let _ = write!(response, "{:x}\r\n{piece}\r\n", piece.len());
        }
        response.push_str("0\r\n\r\n");
    } else {
        let _ = write!(
            response,
            "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    }
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// A word for a status code. Only the ones the fake sends.
const fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Something",
    }
}

/// Keeps a connection open without saying anything.
fn hold(stream: &TcpStream, stop: &Arc<AtomicBool>) {
    while !ended(stop) {
        thread::sleep(Duration::from_millis(10));
        if stream.peer_addr().is_err() {
            return;
        }
    }
}

/// Whether the owner has gone away.
fn ended(stop: &Arc<AtomicBool>) -> bool {
    stop.load(Ordering::Relaxed)
}
