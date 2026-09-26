//! A stand-in payload that is not a payload, for tests.
//!
//! Plays the target side of the two sockets in `docs/VIDEO.md`: encoded video out, controller
//! records in. It exercises the seam between the client halves the way [`crate::fake`] does
//! for the transport, and it is the executable specification of the wire format a real
//! payload must match.
//!
//! Beyond a working stream it serves the failures a player cannot tell apart: no keyframe,
//! bytes that never frame, a stream that stops, and start codes split across writes.

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
    /// A stream a decoder can start from: a keyframe, then dependent pictures, repeating.
    Video {
        /// How many units to send before closing.
        units: usize,
        /// How long to wait between them.
        ///
        /// Zero keeps tests fast.
        apart: Duration,
    },
    /// A stream with no keyframe in it at all.
    ///
    /// The framing is valid and every count climbs, yet a player shows nothing - it looks
    /// like a dead socket unless something counts keyframes.
    Dependent {
        /// How many units to send before closing.
        units: usize,
    },
    /// Bytes with no start code anywhere in them: a socket serving something else.
    Noise {
        /// How many bytes to send.
        bytes: usize,
    },
    /// A valid stream cut so that start codes straddle write boundaries, as a network does.
    Awkward,
    /// Accept the connection and send nothing: a payload running but not producing.
    Silence,
}

/// A start code, in the four-byte form an encoder emits at a unit boundary.
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
    // Filler holds no start code, so every counted unit is one made here.
    made.extend(std::iter::repeat_n(0xAA, filler));
    made
}

/// Everything the fake payload sends on its video socket, as one run of bytes.
///
/// Public so a test can feed the reader the same bytes without a socket.
#[must_use]
pub fn video(serves: &Serves) -> Vec<u8> {
    let mut all = Vec::new();
    match serves {
        Serves::Video { units, .. } => {
            for at in 0..*units {
                // A keyframe first and every eighth after, so a stream of any length has one.
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
            // Every byte has the top bit set, so no run of zeroes can form a start code.
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

/// How the bytes are broken up into writes on their way out.
fn pieces(serves: &Serves, all: &[u8]) -> Vec<Vec<u8>> {
    match serves {
        // Cut so that a start code lands across a boundary.
        Serves::Awkward => {
            let mut cut = Vec::new();
            let mut at = 0;
            let mut take = 3;
            while at < all.len() {
                let end = (at + take).min(all.len());
                cut.push(all[at..end].to_vec());
                at = end;
                // Vary the size so the boundary lands somewhere different in each unit.
                take = if take >= 7 { 1 } else { take + 2 };
            }
            cut
        }
        // A paced stream is cut per unit: units are under forty bytes, so a 4096-byte piece
        // would carry the whole stream in one write and the delay between pieces would never
        // apply.
        Serves::Video { apart, .. } if !apart.is_zero() => by_unit(all),
        _ => all.chunks(4096).map(<[u8]>::to_vec).collect(),
    }
}

/// Cuts a stream at its unit boundaries, one piece per unit.
///
/// Unlike [`Serves::Awkward`], the pieces are whole units, so pacing tests timing and not
/// framing.
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
/// Shared with whoever started the fake, so a test checks what arrived rather than what the
/// sender reported.
#[derive(Debug, Clone, Default)]
pub struct Received(Arc<Mutex<Vec<[u8; RECORD]>>>);

impl Received {
    /// Every whole record that has arrived, in order.
    #[must_use]
    pub fn records(&self) -> Vec<[u8; RECORD]> {
        // A poisoned lock still holds every record that arrived before the panic.
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
    /// Returns whether it got there rather than panicking, so the test words the failure.
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
    /// OS-chosen ports let tests run in parallel, as with [`crate::fake::Fake`].
    ///
    /// # Errors
    ///
    /// Propagates a failure to bind.
    pub fn start(serves: Serves) -> std::io::Result<Self> {
        let video_on = TcpListener::bind("127.0.0.1:0")?;
        let input_on = TcpListener::bind("127.0.0.1:0")?;
        let video = video_on.local_addr()?.port();
        let input = input_on.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let received = Received::default();

        // Non-blocking, so a listener notices the stop flag instead of sitting in `accept`.
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
                    // Held open: connected-and-silent is a distinct state from closed.
                    while !stop.load(Ordering::Relaxed) {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
                // Closing signals the end of the stream.
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
/// Records are reassembled across reads, since one write is not one read.
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

    /// Each served variant contains exactly what it claims to.
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

        let noise = video(&Serves::Noise { bytes: 4096 });
        assert_eq!(noise.len(), 4096);
        assert!(
            !noise.windows(3).any(|at| at == [0, 0, 1]),
            "three zeroes in a row would make this a stream after all"
        );

        assert!(video(&Serves::Silence).is_empty());
    }

    /// A paced stream leaves one whole unit per write, so the pacing delay applies.
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

        // Unpaced, the same stream leaves in one piece.
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

    /// The awkward cut puts at least one start code across a write boundary.
    #[test]
    fn the_awkward_cut_actually_splits_a_start_code() {
        let all = video(&Serves::Awkward);
        let cut = pieces(&Serves::Awkward, &all);
        assert!(cut.len() > 8, "it has to be cut small to split anything");

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
