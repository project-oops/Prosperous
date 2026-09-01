//! Reading frames from a grabber on the target.
//!
//! # What this is for
//!
//! Not watching. Watching is Porthole's job - the target serves its own encoded stream and
//! `pros-core::watch` pipes it to a player, see `docs/VIDEO.md` part three.
//!
//! This is part two: **an instrument for diffing what an emulator drew against what a target
//! drew.** A payload holds the display open and answers `GRAB\n` with one frame, and the whole
//! value of it is that the numbers are trustworthy enough to subtract.
//!
//! # The four rules the header exists to enforce
//!
//! Every one of them is a way for a wrong answer to look like a right one:
//!
//! - **The format is reported, never assumed.** A diff against a frame whose stride was
//!   guessed fails as *the emulator is wrong* rather than as *the client guessed*, and that is
//!   a day lost to the wrong question.
//! - **A non-zero status means no pixels follow.** *It did not work* and *it worked and
//!   produced nothing* must not look the same.
//! - **`bytes` is authoritative and a short read is an error.** A truncated transfer must not
//!   arrive as a smaller frame, because a smaller frame diffs perfectly well and says nothing
//!   true.
//! - **The format field is passed through, not interpreted.** A payload that cannot determine
//!   the format reports a status and sends nothing, rather than labelling pixels with a guess.
//!
//! # Why the checksum is not cryptographic
//!
//! The threat is a truncated or corrupted transfer over a local network, not somebody
//! substituting a frame. FNV-1a catches everything actually likely and is six lines in
//! freestanding C, which is what has to write it at the other end.
//!
//! This is the opposite call from the payload manifest, where a digest guards a download about
//! to be executed with kernel-adjacent privileges. **Different threat, different answer**, and
//! the difference is stated so neither gets changed to match the other.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// What the header starts with.
pub const MAGIC: [u8; 4] = *b"PFRM";

/// The version this reads.
pub const VERSION: u16 = 1;

/// How long the header is.
pub const HEADER: usize = 32;

/// The whole request. Deliberately typeable, because most of what goes wrong here is
/// diagnosed by hand with a socket and a keyboard.
pub const REQUEST: &str = "GRAB\n";

/// The port a target's frame grabber listens on.
///
/// From `docs/VIDEO.md` part two. **Chosen and not measured**, like the two ports part three
/// picks and unlike every other port this crate knows: adjacent to the loader so the two are
/// memorable together, and outside every port the chain was measured using on 2026-08-25 -
/// 9021, 2121, 3232, 2323, 8084, and 6967 for scripted input.
///
/// Named here rather than left in the document because [`grab`] takes a port, and a caller
/// that has to read prose to learn which one is a caller that will eventually read it wrong.
/// The parameter stays, for the same reason a registration can override any other port: a
/// number that is right today is a default, not a fact about the target.
///
/// If it turns out to collide with something, this is a one-line change here and a note in
/// that document saying what it collided with.
pub const PORT: u16 = 9022;

/// What a frame says about itself.
///
/// **Nothing here is interpreted.** `format` is whatever the platform called it and `stride`
/// is what the platform reported, because a client that translated either would be answering
/// a question the payload was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The version the payload wrote.
    pub version: u16,
    /// Zero succeeded; anything else is why not.
    pub status: u16,
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// As the platform reports it, untranslated.
    pub format: u32,
    /// Bytes per row, which is **not** width times four.
    pub stride: u32,
    /// How many pixel bytes follow.
    pub bytes: u64,
}

impl Header {
    /// Reads a header out of exactly [`HEADER`] bytes.
    ///
    /// # Errors
    ///
    /// [`NotAFrame::NotAHeader`] when the magic is wrong, and [`NotAFrame::Version`] when it
    /// is a version this does not read. **Distinct on purpose**: something else on the port
    /// and a newer payload are different problems with different next steps.
    pub fn read(raw: &[u8]) -> Result<Self, NotAFrame> {
        let Some(head) = raw.get(..HEADER) else {
            return Err(NotAFrame::Short {
                wanted: HEADER as u64,
                got: raw.len() as u64,
            });
        };
        if head.get(..4) != Some(&MAGIC) {
            return Err(NotAFrame::NotAHeader);
        }
        let two = |at: usize| -> u16 { u16::from_le_bytes([head[at], head[at + 1]]) };
        let four = |at: usize| -> u32 {
            u32::from_le_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]])
        };
        let version = two(4);
        if version != VERSION {
            return Err(NotAFrame::Version(version));
        }
        let mut eight = [0_u8; 8];
        eight.copy_from_slice(&head[24..32]);
        Ok(Self {
            version,
            status: two(6),
            width: four(8),
            height: four(12),
            format: four(16),
            stride: four(20),
            bytes: u64::from_le_bytes(eight),
        })
    }

    /// Whether the grab succeeded.
    #[must_use]
    pub const fn is_a_frame(&self) -> bool {
        self.status == 0
    }

    /// Whether the header agrees with itself.
    ///
    /// **A header that disagrees with its own payload cannot be trusted about anything else.**
    /// Stride times height is the size a frame of this shape occupies, and a `bytes` that
    /// differs means one of the two is wrong - which is worth finding out here rather than
    /// after diffing two frames that were never the same shape.
    #[must_use]
    pub fn is_self_consistent(&self) -> bool {
        u64::from(self.stride) * u64::from(self.height) == self.bytes
    }
}

/// A frame, and what it said about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// What it says it is.
    pub header: Header,
    /// The pixel bytes, exactly `header.bytes` of them.
    pub pixels: Vec<u8>,
}

/// Why a grab did not produce a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotAFrame {
    /// Something answered and it was not a frame.
    NotAHeader,
    /// A version this does not read.
    Version(u16),
    /// The payload reported it could not grab, and sent no pixels.
    ///
    /// **Not a failure of the transfer.** The target answered; the answer was *no*.
    Refused(u16),
    /// Fewer bytes arrived than the header promised.
    ///
    /// **The single most important error here.** A truncated transfer arriving as a smaller
    /// frame would diff perfectly well against another frame and say nothing true.
    Short {
        /// How many were promised.
        wanted: u64,
        /// How many arrived.
        got: u64,
    },
    /// The pixels do not hash to what was sent with them.
    Corrupt {
        /// What the payload said.
        expected: u32,
        /// What arrived.
        found: u32,
    },
    /// The header disagrees with itself.
    Inconsistent {
        /// Bytes per row times rows.
        implied: u64,
        /// What the header said.
        stated: u64,
    },
    /// The connection did not work.
    Unreachable(String),
}

impl std::fmt::Display for NotAFrame {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAHeader => write!(out, "something answered and it was not a frame"),
            Self::Version(saw) => write!(
                out,
                "a frame of version {saw}, and this reads version {VERSION}"
            ),
            Self::Refused(status) => write!(
                out,
                "the target could not grab a frame and said so: status {status}"
            ),
            Self::Short { wanted, got } => write!(
                out,
                "{got} of {wanted} pixel bytes arrived - a short frame is not a smaller frame"
            ),
            Self::Corrupt { expected, found } => write!(
                out,
                "the pixels hash to {found:#010x} and the target said {expected:#010x}"
            ),
            Self::Inconsistent { implied, stated } => write!(
                out,
                "stride times height is {implied} and the header says {stated} bytes"
            ),
            Self::Unreachable(why) => write!(out, "{why}"),
        }
    }
}

impl std::error::Error for NotAFrame {}

/// FNV-1a over the pixel bytes.
///
/// Matches what a freestanding payload can write in six lines. Not cryptographic, and see the
/// module documentation for why that is the right call here and the wrong one for a download.
#[must_use]
pub fn fingerprint(bytes: &[u8]) -> u32 {
    /// The 32-bit offset basis.
    const BASIS: u32 = 0x811c_9dc5;
    /// The 32-bit prime.
    const PRIME: u32 = 0x0100_0193;

    let mut hash = BASIS;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

/// Reads one frame from an already-open stream.
///
/// Separated from connecting so the whole of it can be tested against bytes in memory, which
/// is what lets a client exist before a payload does.
///
/// # Errors
///
/// Every member of [`NotAFrame`]. A short read is an error rather than a smaller frame, and a
/// non-zero status is [`NotAFrame::Refused`] rather than an empty one.
pub fn read_frame(source: &mut impl Read) -> Result<Frame, NotAFrame> {
    let mut head = [0_u8; HEADER];
    read_exactly(source, &mut head)?;
    let header = Header::read(&head)?;

    if !header.is_a_frame() {
        return Err(NotAFrame::Refused(header.status));
    }
    if !header.is_self_consistent() {
        return Err(NotAFrame::Inconsistent {
            implied: u64::from(header.stride) * u64::from(header.height),
            stated: header.bytes,
        });
    }

    let wanted = usize::try_from(header.bytes).map_err(|_| NotAFrame::Short {
        wanted: header.bytes,
        got: 0,
    })?;
    let mut pixels = vec![0_u8; wanted];
    read_exactly(source, &mut pixels)?;

    let mut tail = [0_u8; 4];
    read_exactly(source, &mut tail)?;
    let expected = u32::from_le_bytes(tail);
    let found = fingerprint(&pixels);
    if expected != found {
        return Err(NotAFrame::Corrupt { expected, found });
    }
    Ok(Frame { header, pixels })
}

/// Fills the buffer or says how far it got.
///
/// **`read_exact` would report an error without saying how much arrived**, and how much
/// arrived is the difference between a payload that died mid-frame and a network that never
/// started.
fn read_exactly(source: &mut impl Read, into: &mut [u8]) -> Result<(), NotAFrame> {
    let mut at = 0;
    while at < into.len() {
        match source.read(&mut into[at..]) {
            Ok(0) => {
                return Err(NotAFrame::Short {
                    wanted: into.len() as u64,
                    got: at as u64,
                });
            }
            Ok(some) => at += some,
            Err(why) => return Err(NotAFrame::Unreachable(why.to_string())),
        }
    }
    Ok(())
}

/// Asks a target for one frame.
///
/// # Errors
///
/// As [`read_frame`], plus [`NotAFrame::Unreachable`] when the port will not accept.
pub fn grab(address: &str, port: u16, patience: Duration) -> Result<Frame, NotAFrame> {
    let target = format!("{address}:{port}");
    let mut stream = TcpStream::connect(&target)
        .map_err(|why| NotAFrame::Unreachable(format!("{target}: {why}")))?;
    stream
        .set_read_timeout(Some(patience))
        .map_err(|why| NotAFrame::Unreachable(why.to_string()))?;
    stream
        .write_all(REQUEST.as_bytes())
        .map_err(|why| NotAFrame::Unreachable(why.to_string()))?;
    stream
        .flush()
        .map_err(|why| NotAFrame::Unreachable(why.to_string()))?;

    let mut buffered = BufReader::new(stream);
    // The header may not be the first thing on the socket if a payload greets. Nothing in the
    // specification says it does, so nothing here skips anything - a greeting would be a
    // change to the format and should read as one.
    let _ = buffered.fill_buf();
    read_frame(&mut buffered)
}

/// Why two frames cannot be compared.
///
/// # Why this is a type rather than a sentence
///
/// It was a `String`, and it was the only error in this crate that was. Everything else here
/// names what went wrong - and this is the one a diffing harness will actually branch on: a
/// shape that changed because a title changed mode is a different situation from a format that
/// changed because the grabber was rebuilt, and a caller that has to match on prose to tell
/// them apart is a caller that stops telling them apart.
///
/// `docs/VIDEO.md` part two names this type in the signature it specifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// They are different sizes.
    Shape {
        /// The first one's width and height.
        left: (u32, u32),
        /// The second one's.
        right: (u32, u32),
    },
    /// They are the same size and describe their pixels differently.
    Format {
        /// What the first one reported, untranslated.
        left: u32,
        /// What the second one reported.
        right: u32,
    },
}

impl std::fmt::Display for Mismatch {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shape { left, right } => write!(
                out,
                "{}x{} against {}x{} - a difference between two shapes is not a difference",
                left.0, left.1, right.0, right.1
            ),
            Self::Format { left, right } => write!(
                out,
                "format {left} against {right} - the same pixels in two encodings are not the same pixels"
            ),
        }
    }
}

impl std::error::Error for Mismatch {}

/// How two frames of the same shape differ.
///
/// # Errors
///
/// When the two are not the same shape. **Comparing frames of different shapes is not a
/// small error**: it produces a number, and a number is what somebody would act on.
pub fn differences(left: &Frame, right: &Frame) -> Result<usize, Mismatch> {
    if left.header.width != right.header.width || left.header.height != right.header.height {
        return Err(Mismatch::Shape {
            left: (left.header.width, left.header.height),
            right: (right.header.width, right.header.height),
        });
    }
    if left.header.format != right.header.format {
        return Err(Mismatch::Format {
            left: left.header.format,
            right: right.header.format,
        });
    }
    Ok(left
        .pixels
        .iter()
        .zip(right.pixels.iter())
        .filter(|(a, b)| a != b)
        .count())
}

#[cfg(test)]
mod tests {
    use super::{Frame, Header, NotAFrame, differences, fingerprint, read_frame};

    /// Builds what a payload would write, so the reader is tested against the format rather
    /// than against itself.
    fn wire(status: u16, width: u32, height: u32, stride: u32, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"PFRM");
        out.extend_from_slice(&1_u16.to_le_bytes());
        out.extend_from_slice(&status.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&0x8000_0000_u32.to_le_bytes());
        out.extend_from_slice(&stride.to_le_bytes());
        out.extend_from_slice(&(pixels.len() as u64).to_le_bytes());
        if status == 0 {
            out.extend_from_slice(pixels);
            out.extend_from_slice(&fingerprint(pixels).to_le_bytes());
        }
        out
    }

    /// A whole frame, read back as what was written.
    #[test]
    fn a_frame_reads_back_as_what_the_target_wrote() {
        let pixels: Vec<u8> = (0..64_u8).map(|at| at.wrapping_mul(3)).collect();
        let raw = wire(0, 4, 4, 16, &pixels);

        let frame = read_frame(&mut raw.as_slice()).expect("it reads");
        assert_eq!(frame.header.width, 4);
        assert_eq!(frame.header.stride, 16, "stride is not width times four");
        assert_eq!(frame.header.format, 0x8000_0000, "reported, not translated");
        assert_eq!(frame.pixels, pixels);
    }

    /// **A short transfer is an error, not a smaller frame.**
    ///
    /// The one that matters most: a smaller frame diffs perfectly well against another and
    /// says nothing true, so a truncated grab has to be impossible to mistake for a whole one.
    #[test]
    fn a_truncated_transfer_is_refused_rather_than_returned() {
        let pixels = vec![7_u8; 64];
        let mut raw = wire(0, 4, 4, 16, &pixels);
        raw.truncate(raw.len() - 20);

        let refused = read_frame(&mut raw.as_slice()).expect_err("it must not read");
        assert!(
            matches!(refused, NotAFrame::Short { .. }),
            "expected a short read: {refused}"
        );
        // And it says how far it got, because a payload that died mid-frame and a network
        // that never started are different problems.
        if let NotAFrame::Short { wanted, got } = refused {
            assert!(got < wanted, "{got} of {wanted}");
        }
    }

    /// **A refusal is not an empty frame.** *It did not work* and *it worked and produced
    /// nothing* must not look the same, and a frame of zeros diffs against another frame of
    /// zeros perfectly.
    #[test]
    fn a_status_means_no_pixels_rather_than_a_black_frame() {
        let raw = wire(3, 1920, 1080, 7680, &[]);
        let refused = read_frame(&mut raw.as_slice()).expect_err("it must not read");
        assert_eq!(refused, NotAFrame::Refused(3));
    }

    /// A header that disagrees with itself is caught before anything is diffed.
    #[test]
    fn a_header_that_contradicts_itself_is_refused() {
        let pixels = vec![0_u8; 64];
        // Stride times height is 64; claim a size that is not.
        let mut raw = wire(0, 4, 4, 16, &pixels);
        raw[24..32].copy_from_slice(&99_u64.to_le_bytes());

        let refused = read_frame(&mut raw.as_slice()).expect_err("it must not read");
        assert!(
            matches!(refused, NotAFrame::Inconsistent { .. }),
            "{refused}"
        );
    }

    /// Corruption in the pixels is caught by the checksum that travelled with them.
    #[test]
    fn pixels_that_changed_in_transit_are_caught() {
        let pixels = vec![1_u8; 64];
        let mut raw = wire(0, 4, 4, 16, &pixels);
        // One bit, somewhere in the middle of the pixels.
        raw[40] ^= 0x01;

        let refused = read_frame(&mut raw.as_slice()).expect_err("it must not read");
        assert!(matches!(refused, NotAFrame::Corrupt { .. }), "{refused}");
    }

    /// Something else answering on the port reads as that, not as a damaged frame.
    #[test]
    fn something_that_is_not_a_frame_says_so() {
        let mut raw = b"HTTP/1.1 404 Not Found\r\n\r\npadding to length".to_vec();
        raw.resize(64, 0);
        assert_eq!(
            read_frame(&mut raw.as_slice()).expect_err("it must not read"),
            NotAFrame::NotAHeader
        );
    }

    /// A newer payload is a different problem from a stranger on the port.
    #[test]
    fn a_version_this_does_not_read_is_its_own_complaint() {
        let mut raw = wire(0, 4, 4, 16, &[0_u8; 64]);
        raw[4..6].copy_from_slice(&9_u16.to_le_bytes());
        assert_eq!(
            read_frame(&mut raw.as_slice()).expect_err("it must not read"),
            NotAFrame::Version(9)
        );
    }

    /// **The instrument gets measured before it is used.**
    ///
    /// A frame diffed against itself is zero, and against a one-byte change is exactly one.
    /// If that is not true, nothing measured with it means anything.
    #[test]
    fn a_frame_against_itself_is_zero_and_one_change_is_one() {
        let pixels: Vec<u8> = (0..64_u8).collect();
        let raw = wire(0, 4, 4, 16, &pixels);
        let frame = read_frame(&mut raw.as_slice()).expect("reads");

        assert_eq!(differences(&frame, &frame).expect("same shape"), 0);

        let mut changed = frame.clone();
        changed.pixels[30] ^= 0xff;
        assert_eq!(differences(&frame, &changed).expect("same shape"), 1);
    }

    /// **Two shapes do not have a difference**, and saying they differ by a number would be
    /// handing somebody a figure to act on.
    #[test]
    fn frames_of_different_shapes_are_not_compared() {
        let header = |width: u32| Header {
            version: 1,
            status: 0,
            width,
            height: 4,
            format: 1,
            stride: width * 4,
            bytes: u64::from(width) * 16,
        };
        let left = Frame {
            header: header(4),
            pixels: vec![0; 64],
        };
        let right = Frame {
            header: header(8),
            pixels: vec![0; 128],
        };
        let refused = differences(&left, &right).expect_err("shapes differ");
        assert_eq!(
            refused,
            super::Mismatch::Shape {
                left: (4, 4),
                right: (8, 4)
            }
        );
        // The words still say it, for whoever reads the message rather than matching on it.
        assert!(
            refused.to_string().contains("not a difference"),
            "{refused}"
        );
    }

    /// **A format that differs is its own answer**, not a shape difference by another route.
    ///
    /// This is what having a type buys: a harness branches on the two, because a shape changing
    /// because a title changed mode and a format changing because the grabber was rebuilt call
    /// for different things.
    #[test]
    fn frames_of_different_formats_are_refused_and_say_which() {
        let header = |format: u32| Header {
            version: 1,
            status: 0,
            width: 4,
            height: 4,
            format,
            stride: 16,
            bytes: 64,
        };
        let left = Frame {
            header: header(1),
            pixels: vec![0; 64],
        };
        let right = Frame {
            header: header(2),
            pixels: vec![0; 64],
        };
        assert_eq!(
            differences(&left, &right).expect_err("formats differ"),
            super::Mismatch::Format { left: 1, right: 2 }
        );
    }

    /// The port the design chose is the one the code names, rather than one in prose.
    #[test]
    fn the_grab_port_is_the_one_the_design_chose() {
        assert_eq!(super::PORT, 9022);
    }
}
