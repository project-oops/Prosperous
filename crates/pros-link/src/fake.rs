//! A target that is not a target, for tests.
//!
//! # Why this ships rather than hiding in a test module
//!
//! The hardest constraint on this project is that its subject is a physical object on a
//! network that is usually switched off. Every consumer of this crate has the same problem,
//! and a fake behind `#[cfg(test)]` would be invisible to all of them - so each would build
//! its own, which is three copies of the thing this crate exists to stop being copied.
//!
//! It is small, it is std-only like the rest, and it costs a consumer nothing to ignore.
//!
//! # What is worth faking
//!
//! Not the happy path. The awkward parts, because they are where the bugs are:
//!
//! - a stream with **no end**, so a reader has to stop on its own window
//! - a server with **no framing**, so a reader has to stop on silence
//! - a loader that **may not answer**, so a reader has to be correct when it does not
//! - a port that refuses **instantly** against one that refuses **slowly**
//!
//! What it deliberately cannot test is whether the real target agrees. That is what a
//! registered target and a manual run are for, and the difference between the two should
//! stay visible in how results are reported.

use std::fmt::Write as _;
use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

/// What a fake file service holds, and where anything written to it lands.
///
/// Shared with whoever started the fake, because **a store that worked and a store that
/// reported success are different things** and only the contents afterwards tell them
/// apart.
///
/// Names are matched exactly as the client asks for them. There is no directory tree here
/// and inventing one would be faking the wrong thing.
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
    /// Say nothing, ever, and hold the connection open.
    ///
    /// The quiet log, and the loader that does not echo.
    Silent,
    /// Send this once, then hold the connection open without closing.
    ///
    /// **Holding it open is the point.** A server that closes gives the reader an EOF to
    /// stop on, which is precisely the signal the real ones do not provide.
    Says(String),
    /// Send this repeatedly until the client goes away.
    ///
    /// The log, which streams and never ends.
    Streams(String),
    /// Read a line, then answer it. Repeats.
    ///
    /// The shell: a banner first, then a reply per command, with no marker saying where a
    /// reply stops.
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
    /// A file service, with a second connection per transfer.
    ///
    /// The awkward part is not the commands, it is that a transfer happens somewhere else:
    /// the server names a port, the client dials it, and everything worth getting wrong is
    /// in the handover.
    Files {
        /// What it holds, and where a stored file lands.
        contents: Store,
        /// The address it claims when it names a data port.
        ///
        /// **Wrong on purpose in the tests.** A server behind any translation reports the
        /// address it believes it has, and a client that dials what it is told instead of
        /// what already reached it works on a bench and fails in a house.
        claims: [u8; 4],
        /// Whether it agrees to binary mode.
        ///
        /// A server that says no is the interesting case: continuing anyway is a transfer
        /// that arrives with its bytes quietly edited.
        binary: bool,
    },
    /// A web service answering one request.
    ///
    /// Enough of one to be worth testing against: a status that is not success, and a body
    /// framed by pieces rather than by a length.
    Serves {
        /// The status code to answer with.
        status: u16,
        /// The body to send.
        body: String,
        /// Whether to send it in sized pieces with no overall length.
        ///
        /// What a server does when it is generating the answer as it goes and does not
        /// know how long it will be.
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
    /// The port is chosen rather than requested so that tests can run at the same time as
    /// each other, and at the same time as a real target on the same machine.
    ///
    /// # Errors
    ///
    /// Propagates the failure to bind, which on a machine with no loopback is worth seeing
    /// rather than papering over.
    pub fn start(behaviour: Behaviour) -> std::io::Result<Self> {
        Self::start_at(0, behaviour)
    }

    /// Starts a fake on a port of the caller's choosing, or any port when given zero.
    ///
    /// # Why this exists
    ///
    /// A target answers on known ports, and anything tested through a real command line
    /// reaches for those numbers rather than being handed one. A stand-in that can only be
    /// put somewhere arbitrary cannot stand in for a target **end to end** - only for the
    /// parts that were already willing to be told where to look.
    ///
    /// # Errors
    ///
    /// Propagates the failure to bind, which for a named port usually means something else
    /// on this machine already holds it. Worth failing on rather than working around: a
    /// test that quietly moves elsewhere is no longer testing what it says it is.
    pub fn start_at(port: u16, behaviour: Behaviour) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, waiting) = mpsc::channel();
        let thread = {
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve(&listener, &behaviour, &stop, &ready))
        };

        // **Wait for the thread before saying the fake has started.**
        //
        // The socket is bound above, so a client can connect straight away and its
        // connection sits in the backlog - which means a fake that is not yet serving looks
        // exactly like one that is silent. Measured on the machine this was written on, a
        // spawned thread took **230 ms** to be scheduled, and a caller reading on a 300 ms
        // window spent all of it waiting for something that had not started. (D003)
        //
        // Bounded, so a machine that cannot start a thread reports a slow fake rather than
        // hanging inside a test.
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
        // **Knock, so the accept notices.**
        //
        // The listener blocks rather than polling, which is what makes it answer a real
        // client in microseconds - and a blocking accept cannot be interrupted by setting a
        // flag. One throwaway connection wakes it, it sees the flag, and it stops. The
        // alternative, a non-blocking listener asked over and over whether anything had
        // arrived, was measured taking up to **487 ms** to notice a connection that was
        // already there. (D003)
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Accepts connections until told to stop.
///
/// `ready` is signalled once, before the first accept, so whoever started this knows the
/// thread is running rather than merely spawned.
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
        // Checked after the accept rather than before it: the connection that woke this may
        // be the knock from `Drop`, and answering it would be talking to nobody.
        if ended(stop) {
            return;
        }
        handle(stream, behaviour, stop);
    }
}

/// Plays one behaviour at one client.
fn handle(mut stream: TcpStream, behaviour: &Behaviour, stop: &Arc<AtomicBool>) {
    // **A connection accepted from a listener inherits its non-blocking mode**, and the
    // listener has to be non-blocking so the accept loop can be told to stop. Left that
    // way, every read here returns immediately whether or not anything arrived - so a
    // wait for the client to go quiet does not wait, and a read that means "nothing yet"
    // is indistinguishable from one that means "gone". Blocking with a short timeout is
    // what the code below is written against, so it is set before anything reads.
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
            // Drain until the client stops sending, then switch. A real loader reads the
            // whole payload before anything it runs can say a word.
            //
            // **One quiet read ends it, but only after something has arrived.** Waiting for
            // several would be a stricter rule that costs a fixed delay before the payload
            // can speak, and a caller listening on a short window would miss the answer for
            // reasons that have nothing to do with what it is testing. Before any bytes
            // arrive there is nothing to be quiet after, so silence there is just a client
            // that has not started.
            let mut buffer = [0_u8; 4096];
            let mut arrived = false;
            let mut waited = 0_u32;
            while !ended(stop) {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => arrived = true,
                    // The bound is on a client that connects and then says nothing at all,
                    // which would otherwise hold this thread until the fake is dropped.
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
        } => serve_files(stream, contents, *claims, *binary, stop),
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
    stop: &Arc<AtomicBool>,
) {
    // Long enough that a client thinking between commands is not mistaken for one that
    // has gone away.
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
    // Where a transfer will happen, once the client has been told about it.
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
                    &mut say,
                );
            }
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
    say: &mut impl FnMut(&mut TcpStream, &str),
) {
    // A missing file is answered before the transfer starts, because that is when a real
    // server knows - and a client that has already opened a data connection has to cope.
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
            // A header, which is part of the format, and a line that is genuinely not an
            // entry - so a client can be tested on telling those two apart.
            let mut listing = String::from("total 2\nthis line is not a listing entry\n");
            for name in contents.names() {
                let size = contents.get(&name).map_or(0, |bytes| bytes.len());
                let _ = writeln!(
                    listing,
                    "-rw-r--r--   1 root root {size:>8} Aug 25 12:00 {name}"
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
            contents.put(argument, bytes);
        }
    }
    drop(data);
    say(writing, "226 transfer complete");
}

/// Answers one web request and closes.
fn serve_web(mut stream: TcpStream, status: u16, body: &str, chunked: bool) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    // Read the request to its blank line, so the client is not writing into a socket
    // nobody drained.
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
        // Two pieces rather than one, so a reader that handles the first and stops is
        // caught rather than passed.
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
