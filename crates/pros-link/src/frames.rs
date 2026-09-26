//! Reading frames from a grabber on the target, for diffing an emulator's output against the
//! hardware's (`docs/VIDEO.md`, Diffing). Watching lives in `pros-core::watch`.
//!
//! A payload holds the display open and answers `GRAB\n` with a header, the pixels and an
//! FNV-1a checksum. The format and stride are reported and passed through, never assumed; a
//! non-zero status means no pixels follow; `bytes` is authoritative and a short read is an
//! error, never a smaller frame.
//!
//! The checksum guards against truncation or corruption on a local network, not substitution,
//! so it is not cryptographic and stays writable in freestanding C. The payload manifest uses a
//! digest because it guards code about to run.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// What the header starts with.
pub const MAGIC: [u8; 4] = *b"PFRM";

/// The version this reads.
pub const VERSION: u16 = 1;

/// How long the header is.
pub const HEADER: usize = 32;

/// The whole request, typeable by hand for diagnosis over a raw socket.
pub const REQUEST: &str = "GRAB\n";

/// The port a target's frame grabber listens on.
///
/// Chosen in `docs/VIDEO.md` (Diffing), not measured: next to the loader, and clear of the
/// ports the chain uses (9021, 2121, 3232, 2323, 8084, and 6967 for scripted input). [`grab`]
/// still takes the port as a parameter so a registration can override it.
pub const PORT: u16 = 9022;

/// What a frame says about itself.
///
/// Nothing here is interpreted: `format` and `stride` are exactly what the platform reported.
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
    /// Bytes per row, which is not necessarily width times four.
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
    /// is a version this does not read. They are distinct because something else on the port
    /// and a newer payload need different next steps.
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

    /// Whether the header agrees with itself: stride times height equals `bytes`.
    ///
    /// A header that disagrees with its own size cannot be trusted about anything else.
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
    /// Not a transfer failure: the target answered no.
    Refused(u16),
    /// Fewer bytes arrived than the header promised.
    ///
    /// A truncated transfer must never arrive as a smaller frame, which would diff cleanly and
    /// mean nothing.
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
/// Matches what a freestanding payload writes; not cryptographic (see the module header).
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
/// Separate from connecting so it can be tested against bytes in memory.
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
/// Unlike `read_exact`, the error carries how much arrived, which separates a payload that
/// stopped mid-frame from a network that never started.
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
    // The format has no greeting, so nothing is skipped; a greeting would read as a bad header.
    let _ = buffered.fill_buf();
    read_frame(&mut buffered)
}

/// Why two frames cannot be compared.
///
/// A type so a diffing harness can branch on it: a shape change (a title changed mode) and a
/// format change (the grabber was rebuilt) need different handling.
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

/// How many bytes differ between two frames of the same shape and format.
///
/// # Errors
///
/// [`Mismatch`] when the two differ in shape or format, since a byte count between them would
/// be a number with no meaning.
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

    /// A whole frame reads back as what was written.
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

    /// A short transfer is an error that says how far it got, not a smaller frame.
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
        if let NotAFrame::Short { wanted, got } = refused {
            assert!(got < wanted, "{got} of {wanted}");
        }
    }

    /// A non-zero status is a refusal, not an empty or black frame.
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

    /// A frame against itself differs by zero, and against a one-byte change by exactly one.
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

    /// Frames of different shapes are refused with `Mismatch::Shape`, not given a count.
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
        assert!(
            refused.to_string().contains("not a difference"),
            "{refused}"
        );
    }

    /// Frames of different formats are refused with `Mismatch::Format`, distinct from shape.
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

    /// The grab port constant matches the design document.
    #[test]
    fn the_grab_port_is_the_one_the_design_chose() {
        assert_eq!(super::PORT, 9022);
    }
}
