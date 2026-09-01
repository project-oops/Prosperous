//! A stand-in payload that is not a payload, for tests.
//!
//! # Why this exists
//!
//! `docs/VIDEO.md` part three describes two sockets: encoded video out on one, controller
//! state in on the other. **This side of both is built and the target side of neither is**,
//! because the target half is gated on a question only real hardware can answer.
//!
//! That left the whole stand-in in a position this project has a name for: every piece
//! individually tested, and no seam between any two of them ever exercised. A video pump that
//! has never read a socket and a controller sender that has never been parsed by anything are
//! two halves of a feature nobody can demonstrate.
//!
//! So this plays the payload. It is the same argument [`crate::fake`] already won for the
//! transport, applied to the part that does not exist yet.
//!
//! # It is also the specification
//!
//! Whoever writes the real payload has to match something. A paragraph is a worse thing to
//! match than a program: this one **is** the wire format, and a target-side implementation
//! that satisfies the same tests is one that will work with the client as shipped.
//!
//! # What is worth faking
//!
//! Not a working stream. The ways of failing that a player cannot tell apart, because
//! distinguishing them is the entire reason this project reads a stream it does not decode:
//!
//! - a stream **with no keyframe**, which decodes to nothing and looks exactly like no stream
//! - bytes that **never frame**, which is a socket serving something else entirely
//! - a stream that **stops**, which is not the same as one that never started
//! - a stream **split at the worst place**, with a start code straddling two writes

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::pad::RECORD;

/// What the fake payload does with its video socket.
#[derive(Debug, Clone)]
pub enum Serves {
    /// A stream a decoder could actually start from.
    ///
    /// A keyframe first, then dependent pictures, repeating. What a working payload looks
    /// like.
    Video {
        /// How many units to send before closing.
        units: usize,
        /// How long to wait between them.
        ///
        /// **Zero is the useful default.** A test that sleeps for a real frame interval is a
        /// test somebody turns off.
        apart: Duration,
    },
    /// A stream with no keyframe in it at all.
    ///
    /// **The fault worth having a fake for.** Every count climbs, the framing is valid, a
    /// player attaches happily and shows nothing - because there is nothing to start from.
    /// Indistinguishable from a dead socket unless somebody counted.
    Dependent {
        /// How many units to send before closing.
        units: usize,
    },
    /// Bytes with no start code anywhere in them.
    ///
    /// A socket serving something that is not this. Arrives, counts as bytes, frames as
    /// nothing.
    Noise {
        /// How many bytes to send.
        bytes: usize,
    },
    /// A valid stream, deliberately cut into pieces at the worst places.
    ///
    /// **Start codes straddle the boundaries.** A real network does this constantly and a
    /// reader that assumed a read contains whole units would pass every other test here.
    Awkward,
    /// Accept the connection and send nothing at all.
    ///
    /// A payload that is running and not producing, which reads as a target that is switched
    /// off unless something says otherwise.
    Silence,
}

/// A start code. Four bytes, which is the form an encoder emits at a unit boundary.
const START: [u8; 4] = [0, 0, 0, 1];

/// The header byte of a unit a decoder can start from.
///
/// Type 5 in the low five bits: a coded picture that depends on nothing before it.
const KEYFRAME: u8 = 0x65;

/// The header byte of a unit that depends on what came before.
const DEPENDENT: u8 = 0x41;

/// One unit, made of a start code, a header byte, and filler.
fn unit(header: u8, filler: usize) -> Vec<u8> {
    let mut made = Vec::with_capacity(START.len() + 1 + filler);
    made.extend_from_slice(&START);
    made.push(header);
    // Filler that contains no start code of its own, so a unit count is the count of units
    // this deliberately produced rather than of accidents in the padding.
    made.extend(std::iter::repeat_n(0xAA, filler));
    made
}

/// Everything the fake payload sends on its video socket, as one run of bytes.
///
/// Public because a test that wants to feed the client without a socket at all - the smallest
/// possible exercise of the reader - needs the same bytes the socket would have carried.
#[must_use]
pub fn video(serves: &Serves) -> Vec<u8> {
    let mut all = Vec::new();
    match serves {
        Serves::Video { units, .. } => {
            for at in 0..*units {
                // A keyframe first and then every eighth, which is roughly what an encoder
                // does and, more to the point, means a stream of any length has one.
                let header = if at % 8 == 0 { KEYFRAME } else { DEPENDENT };
                all.extend(unit(header, 32));
            }
        }
        Serves::Dependent { units } => {
            for _ in 0..*units {
                all.extend(unit(DEPENDENT, 32));
            }
        }
        Serves::Noise { bytes } => {
            // Deliberately not zeroes: three zero bytes in a row would be a start code, and a
            // fake that accidentally framed would be testing the opposite of what it claims.
            all.extend((0..*bytes).map(|at| 0x80 | u8::try_from(at % 64).unwrap_or(0)));
        }
        Serves::Awkward => {
            for at in 0..8 {
                all.extend(unit(if at == 0 { KEYFRAME } else { DEPENDENT }, 16));
            }
        }
        Serves::Silence => {}
    }
    all
}

/// How the bytes are broken up on their way out.
///
/// **Not a detail.** A reader that only ever sees whole units has not been tested against
/// anything a network does.
fn pieces(serves: &Serves, all: &[u8]) -> Vec<Vec<u8>> {
    match serves {
        // Cut so that a start code lands across a boundary: three bytes of one piece and the
        // fourth at the head of the next. Nothing else in this file is as likely to find a
        // real defect.
        Serves::Awkward => {
            let mut cut = Vec::new();
            let mut at = 0;
            let mut take = 3;
            while at < all.len() {
                let end = (at + take).min(all.len());
                cut.push(all[at..end].to_vec());
                at = end;
                // Vary it, so the boundary lands somewhere different in each unit rather than
                // in the same place every time.
                take = if take >= 7 { 1 } else { take + 2 };
            }
            cut
        }
        // **A paced stream is cut per unit, because otherwise pacing does nothing.**
        //
        // A unit here is under forty bytes, so a stream of eighty of them fits inside one
        // four-kilobyte piece and leaves in a single write - and the delay, applied between
        // pieces, is applied once at the end where it changes nothing.
        //
        // That was the bug: the knob existed, was documented, and turned nothing. The rate
        // this fake was supposed to be able to produce could not be produced.
        Serves::Video { apart, .. } if !apart.is_zero() => by_unit(all),
        _ => all.chunks(4096).map(<[u8]>::to_vec).collect(),
    }
}

/// Cuts a stream at its unit boundaries, one piece per unit.
///
/// The pieces are whole units, which is the opposite of what [`Serves::Awkward`] does and is
/// the point: pacing is about **when** bytes arrive, so the cut has to be somewhere that does
/// not also test framing.
fn by_unit(all: &[u8]) -> Vec<Vec<u8>> {
    let mut cut = Vec::new();
    let mut begins = 0;
    let mut at = 1;
    while at + START.len() <= all.len() {
        if all[at..at + START.len()] == START {
            cut.push(all[begins..at].to_vec());
            begins = at;
            at += START.len();
        } else {
            at += 1;
        }
    }
    if begins < all.len() {
        cut.push(all[begins..].to_vec());
    }
    cut
}

/// What arrived on the input socket.
///
/// Shared with whoever started the fake, because **a sender that worked and a sender that
/// reported success are different things**, and only what the other end actually holds tells
/// them apart.
#[derive(Debug, Clone, Default)]
pub struct Received(Arc<Mutex<Vec<[u8; RECORD]>>>);

impl Received {
    /// Every whole record that has arrived, in order.
    #[must_use]
    pub fn records(&self) -> Vec<[u8; RECORD]> {
        // A poisoned lock still holds every record that arrived before the panic, and losing
        // them would turn one test's failure into a second, misleading one somewhere else.
        self.0
            .lock()
            .map_or_else(|held| held.into_inner().clone(), |held| held.clone())
    }

    /// How many have arrived.
    #[must_use]
    pub fn count(&self) -> usize {
        self.0.lock().map_or(0, |held| held.len())
    }

    /// Waits until at least this many have arrived, or gives up.
    ///
    /// **Returns whether it got there** rather than panicking, so a test can say what it was
    /// waiting for in its own words.
    #[must_use]
    pub fn wait_for(&self, many: usize, patience: Duration) -> bool {
        let until = std::time::Instant::now() + patience;
        while std::time::Instant::now() < until {
            if self.count() >= many {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        self.count() >= many
    }
}

/// A fake stand-in payload, listening on real ports on the loopback interface.
///
/// Dropping it stops both listeners.
#[derive(Debug)]
pub struct Standin {
    video: u16,
    input: u16,
    received: Received,
    stop: Arc<AtomicBool>,
    threads: Vec<thread::JoinHandle<()>>,
}

impl Standin {
    /// Starts one, on ports the operating system chooses.
    ///
    /// Chosen rather than requested so that tests can run beside each other, and beside a real
    /// target on the same machine - the same reason [`crate::fake::Fake`] does it.
    ///
    /// # Errors
    ///
    /// Propagates the failure to bind, which is worth seeing rather than papering over.
    pub fn start(serves: Serves) -> std::io::Result<Self> {
        let video_on = TcpListener::bind("127.0.0.1:0")?;
        let input_on = TcpListener::bind("127.0.0.1:0")?;
        let video = video_on.local_addr()?.port();
        let input = input_on.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let received = Received::default();

        // Short, so a listener notices it has been asked to stop rather than sitting in
        // `accept` until a test's process exits.
        video_on.set_nonblocking(true)?;
        input_on.set_nonblocking(true)?;

        let mut threads = Vec::new();
        {
            let stop = Arc::clone(&stop);
            threads.push(thread::spawn(move || serving(&video_on, &serves, &stop)));
        }
        {
            let stop = Arc::clone(&stop);
            let received = received.clone();
            threads.push(thread::spawn(move || {
                listening(&input_on, &received, &stop);
            }));
        }

        Ok(Self {
            video,
            input,
            received,
            stop,
            threads,
        })
    }

    /// The port video is served on.
    #[must_use]
    pub const fn video_port(&self) -> u16 {
        self.video
    }

    /// The port controller records are accepted on.
    #[must_use]
    pub const fn input_port(&self) -> u16 {
        self.input
    }

    /// The loopback address, for a client that wants one.
    #[must_use]
    pub const fn address(&self) -> &'static str {
        "127.0.0.1"
    }

    /// What has arrived on the input socket.
    #[must_use]
    pub fn received(&self) -> Received {
        self.received.clone()
    }
}

impl Drop for Standin {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

/// Accepts on the video port and sends whatever this fake serves.
fn serving(on: &TcpListener, serves: &Serves, stop: &Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match on.accept() {
            Ok((mut to, _)) => {
                let all = video(serves);
                let apart = match serves {
                    Serves::Video { apart, .. } => *apart,
                    _ => Duration::ZERO,
                };
                for piece in pieces(serves, &all) {
                    if stop.load(Ordering::Relaxed) || to.write_all(&piece).is_err() {
                        break;
                    }
                    if !apart.is_zero() {
                        thread::sleep(apart);
                    }
                }
                if matches!(serves, Serves::Silence) {
                    // Hold the connection open with nothing on it, because *connected and
                    // sending nothing* is a distinct state from *closed* and the client is
                    // supposed to say which.
                    while !stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                // Closing is the signal the stream is over. A fake that lingered would test a
                // stall instead of an end.
                drop(to);
            }
            Err(why) if why.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return,
        }
    }
}

/// Accepts on the input port and reassembles whole records.
///
/// **Whole records, reassembled**, because a sender batching a frame's worth into one write is
/// not a promise that they arrive that way - and a fake that assumed one read is one record
/// would be a fake that agreed with a client's bug.
fn listening(on: &TcpListener, into: &Received, stop: &Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match on.accept() {
            Ok((from, _)) => reading(from, into, stop),
            Err(why) if why.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(_) => return,
        }
    }
}

/// Reads one connection until it ends, keeping every whole record.
fn reading(mut from: TcpStream, into: &Received, stop: &Arc<AtomicBool>) {
    let _ = from.set_read_timeout(Some(Duration::from_millis(50)));
    let mut held: Vec<u8> = Vec::new();
    let mut buffer = [0_u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        match from.read(&mut buffer) {
            Ok(0) => return,
            Ok(some) => {
                held.extend_from_slice(&buffer[..some]);
                while held.len() >= RECORD {
                    let mut record = [0_u8; RECORD];
                    record.copy_from_slice(&held[..RECORD]);
                    held.drain(..RECORD);
                    if let Ok(mut keeping) = into.0.lock() {
                        keeping.push(record);
                    }
                }
            }
            Err(why) if why.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DEPENDENT, KEYFRAME, START, Serves, pieces, video};

    /// The fake's own output is what it claims, checked before anything is tested against it.
    ///
    /// **A fake that is wrong is worse than no fake**, because every test built on it passes
    /// while describing something that never happens.
    #[test]
    fn what_it_serves_is_what_it_says() {
        let working = video(&Serves::Video {
            units: 8,
            apart: std::time::Duration::ZERO,
        });
        assert!(working.starts_with(&START), "a unit begins at a start code");
        assert_eq!(
            working[4], KEYFRAME,
            "a decoder must have somewhere to begin"
        );

        let blind = video(&Serves::Dependent { units: 8 });
        assert!(
            !blind
                .windows(5)
                .any(|at| at[..4] == START && at[4] == KEYFRAME),
            "the point of this one is that there is nothing to start from"
        );
        assert!(
            blind
                .windows(5)
                .any(|at| at[..4] == START && at[4] == DEPENDENT),
            "it is still a valid stream, which is what makes it deceptive"
        );

        // The noise must genuinely not frame, or it tests the opposite of what it claims.
        let noise = video(&Serves::Noise { bytes: 4096 });
        assert_eq!(noise.len(), 4096);
        assert!(
            !noise.windows(3).any(|at| at == [0, 0, 1]),
            "three zeroes in a row would make this a stream after all"
        );

        assert!(video(&Serves::Silence).is_empty());
    }

    /// **Pacing actually paces**, which it did not when it was first written.
    ///
    /// A unit is under forty bytes, so an entire short stream fits in one four-kilobyte piece
    /// and leaves in a single write. The delay between pieces was therefore applied once, at
    /// the end, where it changed nothing - a documented knob that turned nothing, and the
    /// reason the client's rate measurement went untested.
    ///
    /// This is the guard: asking for pacing must produce one piece per unit, or no rate can be
    /// measured against this fake at all.
    #[test]
    fn asking_for_pacing_cuts_the_stream_per_unit() {
        let paced = Serves::Video {
            units: 40,
            apart: std::time::Duration::from_millis(5),
        };
        let all = video(&paced);
        let cut = pieces(&paced, &all);

        assert_eq!(
            cut.len(),
            40,
            "a paced stream must leave one unit at a time"
        );
        for piece in &cut {
            assert!(
                piece.starts_with(&START),
                "each piece must be a whole unit, not a fragment of one"
            );
            assert_eq!(
                piece.windows(START.len()).filter(|at| *at == START).count(),
                1,
                "and exactly one unit, or the pacing is coarser than it claims"
            );
        }
        let back: Vec<u8> = cut.iter().flatten().copied().collect();
        assert_eq!(back, all, "cutting must not change the bytes");

        // Unpaced, the same stream leaves in one piece - which is what made the knob useless.
        let hurried = Serves::Video {
            units: 40,
            apart: std::time::Duration::ZERO,
        };
        assert_eq!(
            pieces(&hurried, &video(&hurried)).len(),
            1,
            "the contrast is the whole point: unpaced, this is a single write"
        );
    }

    /// **The awkward cut puts a start code across a boundary**, which is the only reason it
    /// exists. If it ever stopped doing that the test built on it would keep passing.
    #[test]
    fn the_awkward_cut_actually_splits_a_start_code() {
        let all = video(&Serves::Awkward);
        let cut = pieces(&Serves::Awkward, &all);
        assert!(cut.len() > 8, "it has to be cut small to split anything");

        // Reassembling must give back exactly what went in - a fake that lost a byte would
        // look like a client that dropped one.
        let back: Vec<u8> = cut.iter().flatten().copied().collect();
        assert_eq!(back, all, "cutting must not change the bytes");

        // Find a start code that does not sit wholly inside one piece.
        let mut boundaries = Vec::new();
        let mut at = 0;
        for piece in &cut {
            at += piece.len();
            boundaries.push(at);
        }
        let split = all
            .windows(4)
            .enumerate()
            .filter(|(_, at)| *at == START)
            .any(|(begins, _)| {
                boundaries
                    .iter()
                    .any(|edge| *edge > begins && *edge < begins + 4)
            });
        assert!(
            split,
            "no start code straddles a write, so this tests nothing"
        );
    }
}
