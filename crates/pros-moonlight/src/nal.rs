//! Grouping an Annex-B H.264 stream into frames, so the packetiser has whole pictures to send.
//!
//! The bridge reads the target's encoded stream off 9805 as a byte stream and has to hand the
//! Moonlight packetiser one **access unit** (one coded picture, with its parameter sets) at a
//! time. This finds NAL boundaries by their start codes and groups them: parameter sets and other
//! non-picture units accumulate and attach to the next picture, and each picture NAL closes a
//! frame. It classifies each unit with [`pros_link::stream::Kind`] - the same reader Porthole's
//! `watch` counts with - so "is this a keyframe" is decided in one place, not two.
//!
//! This is single-slice-per-picture grouping: a stream that splits one picture across several VCL
//! NALs would see each treated as its own frame. The target's encoder emits one slice per picture,
//! which is the case this serves; a multi-slice source is a later refinement, noted rather than
//! pretended away.
//!
//! **Reading is not decoding** still holds - this finds unit boundaries and reads one header byte
//! per unit, exactly what `pros_link::stream` already does, and never interprets the picture.

use pros_link::stream::Kind;

/// One access unit: the Annex-B bytes of a coded picture, with any parameter sets before it.
#[derive(Debug, Clone)]
pub(crate) struct Frame {
    /// The frame's bytes, start codes included, ready to packetise.
    pub(crate) bytes: Vec<u8>,
    /// Whether the picture is a keyframe (an IDR the decoder can start from).
    pub(crate) keyframe: bool,
}

/// Assembles frames from a byte stream fed in arbitrary pieces.
#[derive(Debug, Default)]
pub(crate) struct Frames {
    /// Bytes not yet split into a complete NAL.
    pending: Vec<u8>,
    /// Bytes of the frame being assembled - accumulated NALs up to and including a picture.
    frame: Vec<u8>,
    /// Whether the frame being assembled contains a keyframe picture.
    keyframe: bool,
    /// Whether the frame being assembled already holds a picture NAL.
    has_picture: bool,
}

impl Frames {
    /// A fresh assembler.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Feed more stream bytes, returning any frames that completed.
    ///
    /// A NAL is only complete once the **next** start code appears, so the bytes from the last
    /// start code onward stay in `pending` for next time; [`Frames::finish`] flushes them.
    pub(crate) fn feed(&mut self, more: &[u8]) -> Vec<Frame> {
        self.pending.extend_from_slice(more);
        let mut frames = Vec::new();
        let codes = start_codes(&self.pending);
        if codes.len() < 2 {
            return frames;
        }
        let mut consumed = 0;
        for pair in codes.windows(2) {
            let nal = self.pending[pair[0]..pair[1]].to_vec();
            self.take_nal(&nal, &mut frames);
            consumed = pair[1];
        }
        self.pending.drain(..consumed);
        frames
    }

    /// Flush whatever is still held - the stream has ended.
    pub(crate) fn finish(&mut self) -> Option<Frame> {
        if !self.pending.is_empty() {
            let tail = std::mem::take(&mut self.pending);
            let mut frames = Vec::new();
            self.take_nal(&tail, &mut frames);
            if let Some(frame) = frames.into_iter().next() {
                return Some(frame);
            }
        }
        self.flush()
    }

    /// Emit the frame being assembled, if any, and reset the assembler.
    fn flush(&mut self) -> Option<Frame> {
        self.has_picture = false;
        if self.frame.is_empty() {
            None
        } else {
            Some(Frame {
                bytes: std::mem::take(&mut self.frame),
                keyframe: std::mem::replace(&mut self.keyframe, false),
            })
        }
    }

    /// Add one complete NAL (start code included) to the frame being assembled, closing the frame
    /// when the NAL is a picture.
    fn take_nal(&mut self, nal: &[u8], frames: &mut Vec<Frame>) {
        let Some(kind) = header_of(nal).map(Kind::of) else {
            self.frame.extend_from_slice(nal);
            return;
        };
        let is_picture = matches!(kind, Kind::Keyframe | Kind::Picture);
        // A picture arriving while the frame already holds one starts a new access unit.
        if is_picture && self.has_picture && let Some(frame) = self.flush() {
            frames.push(frame);
        }
        self.frame.extend_from_slice(nal);
        if kind == Kind::Keyframe {
            self.keyframe = true;
        }
        if is_picture {
            self.has_picture = true;
            if let Some(frame) = self.flush() {
                frames.push(frame);
            }
        }
    }
}

/// The NAL header byte (the first byte after the start code), if the unit has one.
fn header_of(nal: &[u8]) -> Option<u8> {
    let after = if nal.starts_with(&[0, 0, 0, 1]) {
        4
    } else if nal.starts_with(&[0, 0, 1]) {
        3
    } else {
        0
    };
    nal.get(after).copied()
}

/// The byte offset of every Annex-B start code (`00 00 01`) in `bytes`.
///
/// Positions only, with no end boundary: a unit runs from one start code to the next, and the
/// bytes after the last start code are an incomplete unit the caller holds until more arrives.
fn start_codes(bytes: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if bytes[i] == 0 && bytes[i + 1] == 0 && bytes[i + 2] == 1 {
            offsets.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }
    offsets
}

#[cfg(test)]
mod tests {
    use super::Frames;

    // Annex-B units: 4-byte start code then a header byte whose low 5 bits are the type.
    fn nal(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut unit = vec![0, 0, 0, 1, kind];
        unit.extend_from_slice(body);
        unit
    }

    #[test]
    fn a_keyframe_groups_its_parameter_sets_with_the_idr() {
        let mut frames = Frames::new();
        let mut stream = Vec::new();
        stream.extend(nal(7, b"sps")); // sequence parameters
        stream.extend(nal(8, b"pps")); // picture parameters
        stream.extend(nal(5, b"idr")); // keyframe slice
        stream.extend(nal(1, b"p1")); // a following inter picture
        // Feed a trailing start code so the p-frame is seen as complete.
        stream.extend([0, 0, 0, 1, 1]);
        let out = frames.feed(&stream);
        assert_eq!(out.len(), 2, "one keyframe access unit, then one inter picture");
        assert!(out[0].keyframe, "the first frame is the keyframe");
        assert!(out[0].bytes.windows(3).any(|w| w == b"sps"));
        assert!(out[0].bytes.windows(3).any(|w| w == b"idr"));
        assert!(!out[1].keyframe);
    }

    #[test]
    fn a_unit_split_across_feeds_is_held_until_complete() {
        let mut frames = Frames::new();
        let whole = nal(5, b"keyframe-body");
        let (head, tail) = whole.split_at(6);
        assert!(frames.feed(head).is_empty(), "an incomplete unit yields nothing");
        frames.feed(tail);
        // Nothing is emitted until a following start code closes the picture; finish() flushes it.
        let last = frames.finish().expect("the held picture flushes at end of stream");
        assert!(last.keyframe);
        assert!(last.bytes.windows(8).any(|w| w == b"keyframe"));
    }
}
