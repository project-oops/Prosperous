//! A target that is not a target, for driving the bridge without a console.
//!
//! # Why this exists
//!
//! The bridge's whole point is that it can be built and tested against a stock Moonlight client
//! on one LAN with no console in the room (`docs/VIDEO.md` part four, item 5). That is only true
//! if something stands in for Porthole's payload: a thing that **serves encoded video on 9805**
//! and **reads controller records on 9806**, the two ports the real payload will serve. This is
//! that thing.
//!
//! It is deliberately dumb. It does not encode - it loops a canned Annex-B clip, because the
//! bridge only reads the bytes and never decodes them, so a real recording played on repeat is
//! indistinguishable from a live encoder as far as the code under test can tell. And it does not
//! act on input - it decodes each [`Pad`] record and prints it, because the thing being tested is
//! that the bridge *produces* correct records, and a printed line is the cheapest possible proof.
//!
//! # The direction of each port, which is easy to get backwards
//!
//! The **target listens; the host connects.** Porthole's payload is the server on both ports -
//! the host (the bridge, or Porthole's own `watch`/`feed`) connects *out* to it. So the fake
//! target is two listeners: a client that connects to [`Ports::video`] is fed the clip, and a
//! client that connects to [`Ports::input`] has its records read.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use pros_link::pad::{Button, Pad, RECORD};

/// Porthole's video port, from `docs/VIDEO.md` part three's measured port map.
///
/// Chosen there, not measured - 9805 sits with 9806 as a memorable pair and clear of every port
/// the boot chain uses. It lives as a constant here because the fake has to serve the same port
/// the bridge will read.
pub const VIDEO_PORT: u16 = 9805;

/// The two ports a fake target listens on, mirroring Porthole.
#[derive(Debug, Clone, Copy)]
pub struct Ports {
    /// Where encoded video is served: a client connects and is fed the clip. Porthole's 9805.
    pub video: u16,
    /// Where controller records are read: a client connects and writes. Porthole's 9806.
    pub input: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            video: VIDEO_PORT,
            input: pros_link::feed::PORT,
        }
    }
}

/// How much of the clip is written per burst, and the pauses that pace it.
///
/// The clip is looped rather than streamed from an encoder, so it has to be paced to something a
/// reader treats like a live stream instead of one enormous burst. These are not measured - they
/// are a rate that reads smoothly and is cheap to change; the bridge's own counts are the thing
/// that has to be right, not this.
const BURST: usize = 32 * 1024;
/// The pause between bursts within one pass of the clip.
const BURST_PAUSE: Duration = Duration::from_millis(2);
/// The pause at the end of the clip before it is played again.
const LOOP_PAUSE: Duration = Duration::from_millis(16);

/// A one-line human description of what a pad is doing.
///
/// Names the held buttons and any stick or trigger that has left its rest position, so a record
/// arriving on 9806 can be read at a glance. A pad at rest is said to be so rather than printed as
/// an empty line, because "nothing held" and "no record arrived" must not look the same.
#[must_use]
pub fn describe(pad: &Pad) -> String {
    if pad.is_at_rest() {
        return format!("pad {} #{}: at rest", pad.slot, pad.sequence);
    }
    let mut parts: Vec<String> = Vec::new();
    for button in Button::ALL {
        if pad.holds(button) && !button.is_a_trigger() {
            parts.push(button.name().to_owned());
        }
    }
    if pad.l2 > 0 {
        parts.push(format!("L2={}", pad.l2));
    }
    if pad.r2 > 0 {
        parts.push(format!("R2={}", pad.r2));
    }
    if pad.left_x != pros_link::pad::CENTRE || pad.left_y != pros_link::pad::CENTRE {
        parts.push(format!("Lstick=({},{})", pad.left_x, pad.left_y));
    }
    if pad.right_x != pros_link::pad::CENTRE || pad.right_y != pros_link::pad::CENTRE {
        parts.push(format!("Rstick=({},{})", pad.right_x, pad.right_y));
    }
    format!("pad {} #{}: {}", pad.slot, pad.sequence, parts.join(" "))
}

/// Read 24-byte controller records from `from` until it closes, handing each decoded [`Pad`] to
/// `on_pad`. Returns how many valid records were read.
///
/// A record that does not decode - wrong magic, a reserved byte set, a slot out of range - is
/// counted as read but reported through `tracing` and not passed on, because a bridge that sent a
/// malformed record is exactly the fault this stand-in exists to catch, and swallowing it would
/// hide it.
///
/// # Errors
///
/// Propagates any read error other than a clean end of stream. A connection that closes on a
/// record boundary is the normal way input ends and returns `Ok`.
pub fn drain_input<R: Read>(from: &mut R, mut on_pad: impl FnMut(Pad)) -> io::Result<usize> {
    let mut record = [0_u8; RECORD];
    let mut read = 0_usize;
    loop {
        match from.read_exact(&mut record) {
            Ok(()) => {
                read += 1;
                match Pad::from_wire(&record) {
                    Ok(pad) => on_pad(pad),
                    Err(why) => tracing::warn!(?why, "a record on the input port did not decode"),
                }
            }
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(read),
            Err(error) => return Err(error),
        }
    }
}

/// Write `clip` to `to` once, paced into [`BURST`]-sized writes so a reader sees a stream rather
/// than one burst.
///
/// # Errors
///
/// Propagates a write error. A peer that hangs up mid-clip surfaces as a broken pipe, which the
/// caller treats as the client having stopped watching rather than as a fault.
fn pump_once<W: Write>(to: &mut W, clip: &[u8]) -> io::Result<()> {
    for burst in clip.chunks(BURST) {
        to.write_all(burst)?;
        to.flush()?;
        thread::sleep(BURST_PAUSE);
    }
    Ok(())
}

/// Feed `clip` to one connected client in a loop until it disconnects.
fn serve_video(mut stream: TcpStream, clip: &[u8]) {
    loop {
        match pump_once(&mut stream, clip) {
            Ok(()) => thread::sleep(LOOP_PAUSE),
            Err(error) => {
                tracing::debug!(%error, "video client went away");
                return;
            }
        }
    }
}

/// Bind both listeners. Port `0` asks the OS for a free one, which is how a test gets an address
/// nothing else is using.
///
/// # Errors
///
/// If either port cannot be bound - most often because a previous run is still holding it.
pub fn bind(ports: &Ports) -> io::Result<(TcpListener, TcpListener)> {
    let video = TcpListener::bind(("0.0.0.0", ports.video))?;
    let input = TcpListener::bind(("0.0.0.0", ports.input))?;
    Ok((video, input))
}

/// Run the fake target forever: feed the clip to every client of the video listener, and print
/// every record that arrives on the input listener.
///
/// Blocks. Each connection is handled on its own thread, so several clients - or a client that
/// reconnects - are all served. A single failed accept is logged and does not bring the stand-in
/// down, because one client hanging up is not a reason to stop serving the next.
///
/// # Errors
///
/// Reserved for a future fatal condition; today it runs until the process is stopped.
pub fn run(video: &TcpListener, input: &TcpListener, clip: &[u8]) -> io::Result<()> {
    thread::scope(|scope| {
        scope.spawn(|| {
            for client in input.incoming() {
                match client {
                    Ok(mut stream) => {
                        scope.spawn(move || {
                            let drained =
                                drain_input(&mut stream, |pad| println!("{}", describe(&pad)));
                            if let Err(error) = drained {
                                tracing::debug!(%error, "input client ended with an error");
                            }
                        });
                    }
                    Err(error) => {
                        tracing::debug!(%error, "an input connection could not be accepted");
                    }
                }
            }
        });
        for client in video.incoming() {
            match client {
                Ok(stream) => {
                    scope.spawn(move || serve_video(stream, clip));
                }
                Err(error) => tracing::debug!(%error, "a video connection could not be accepted"),
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::{Ports, bind, describe, drain_input, run};
    use pros_link::pad::{Button, Pad};
    use std::io::{Cursor, Read, Write};
    use std::net::TcpStream;
    use std::thread;

    #[test]
    fn a_pad_at_rest_says_so_rather_than_printing_nothing() {
        let line = describe(&Pad::rest());
        assert!(line.contains("at rest"), "{line}");
    }

    #[test]
    fn a_held_button_is_named() {
        let mut pad = Pad::rest();
        pad.hold(Button::Cross, true);
        let line = describe(&pad);
        assert!(line.contains(Button::Cross.name()), "{line}");
    }

    #[test]
    fn a_pulled_trigger_shows_its_pressure_not_just_its_bit() {
        let mut pad = Pad::rest();
        pad.pull(Button::R2, 200);
        let line = describe(&pad);
        assert!(line.contains("R2=200"), "{line}");
    }

    #[test]
    fn draining_reads_every_whole_record_and_stops_clean_at_eof() {
        let mut wire = Vec::new();
        for sequence in 0..3 {
            let mut pad = Pad::rest();
            pad.sequence = sequence;
            wire.extend_from_slice(&pad.to_wire());
        }
        let mut seen = Vec::new();
        let read = drain_input(&mut Cursor::new(wire), |pad| seen.push(pad.sequence)).unwrap();
        assert_eq!(read, 3);
        assert_eq!(seen, vec![0, 1, 2]);
    }

    #[test]
    fn a_trailing_partial_record_is_not_a_read() {
        let mut wire = Pad::rest().to_wire().to_vec();
        wire.extend_from_slice(&[1, 2, 3]); // half a record, then the stream ends
        let read = drain_input(&mut Cursor::new(wire), |_| {}).unwrap();
        assert_eq!(read, 1, "the whole record counts; the fragment does not");
    }

    #[test]
    fn a_client_reads_the_clip_and_its_records_arrive() {
        // Bind ephemeral ports so the test uses addresses nothing else holds.
        let clip = b"\x00\x00\x00\x01\x67 a fake stream of bytes \x00\x00\x00\x01\x65 keyframe";
        let (video, input) = bind(&Ports { video: 0, input: 0 }).unwrap();
        // Bound on 0.0.0.0 so a real fake target accepts LAN clients; a test connects on the
        // loopback with the port the OS actually chose.
        let video_addr = ("127.0.0.1", video.local_addr().unwrap().port());
        let input_addr = ("127.0.0.1", input.local_addr().unwrap().port());

        // The bridge side: connect to the video port and read some bytes.
        let reader = thread::spawn(move || {
            let mut stream = TcpStream::connect(video_addr).unwrap();
            let mut got = [0_u8; 16];
            stream.read_exact(&mut got).unwrap();
            got
        });
        // ... and connect to the input port and send one record.
        let mut pad = Pad::rest();
        pad.hold(Button::Triangle, true);
        pad.sequence = 7;
        let record = pad.to_wire();

        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_seen = std::sync::Arc::clone(&seen);
        let clip_owned = clip.to_vec();
        let server = thread::spawn(move || {
            thread::scope(|scope| {
                scope.spawn(|| {
                    if let Ok((stream, _)) = video.accept() {
                        super::serve_video(stream, &clip_owned);
                    }
                });
                if let Ok((mut stream, _)) = input.accept() {
                    let _ = drain_input(&mut stream, |pad| sink_seen.lock().unwrap().push(pad));
                }
            });
        });

        let mut writer = TcpStream::connect(input_addr).unwrap();
        writer.write_all(&record).unwrap();
        drop(writer); // close so the input drain ends cleanly

        let head = reader.join().unwrap();
        assert_eq!(&head, &clip[..16], "the client read the start of the clip");
        server.join().unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert!(seen[0].holds(Button::Triangle));
        assert_eq!(seen[0].sequence, 7);
    }

    #[test]
    fn run_is_wired_up() {
        // A smoke test that the accept-loop signature composes; it never returns, so only its
        // type is exercised here.
        fn _assert(video: &std::net::TcpListener, input: &std::net::TcpListener, clip: &[u8]) {
            let _: fn(
                &std::net::TcpListener,
                &std::net::TcpListener,
                &[u8],
            ) -> std::io::Result<()> = run;
            let _ = (video, input, clip);
        }
    }
}
