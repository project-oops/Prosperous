//! Reading what is in an encoded video stream, without decoding it.
//!
//! # Reading is not decoding, and the split is the point
//!
//! `docs/VIDEO.md` part three hands the socket to a media player, because decoding would mean
//! a substantial C or C++ dependency reached through FFI in a workspace that **forbids**
//! unsafe code - to show a picture `mpv` shows for free.
//!
//! But handing a socket to a player answers nothing when the picture does not appear. Is the
//! payload emitting? Is it emitting anything a decoder could use? Has a keyframe ever gone
//! past? A player answers *no picture*; this answers **which of the several reasons**.
//!
//! So: find the units, name their types, count them. Never interpret one.
//!
//! # The framing, and why this one
//!
//! Annex B - each unit preceded by `00 00 01` or `00 00 00 01`. Chosen because **it is what
//! players already accept**, so the same bytes that feed this feed `mpv` with no container
//! and no header of ours in the way. A length-prefixed format would be marginally easier to
//! parse here and would need our client to be running for anything to be watchable, which is
//! the wrong trade.
//!
//! # What a partial read must not become
//!
//! A stream arrives in pieces, and a unit split across two reads is the normal case. **A unit
//! is only complete when the next start code is found**, so this holds the tail rather than
//! emitting a short unit - because a truncated access unit is one a decoder rejects, and one
//! reported as complete would send somebody looking at the encoder instead of the network.

/// What a unit is for, as far as this needs to know.
///
/// **Only the distinctions that answer a question somebody is asking.** The full type table is
/// a decoder's business; what matters here is whether a picture could be produced, and whether
/// the parameters a decoder needs have ever gone past.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A picture that stands alone. Until one of these, a decoder has nothing to start from.
    Keyframe,
    /// A picture that depends on earlier ones.
    Picture,
    /// Sequence parameters - a decoder needs these before any picture.
    Sequence,
    /// Picture parameters - likewise.
    Picture_,
    /// Supplemental information a decoder may ignore.
    Extra,
    /// Something this does not name.
    ///
    /// **Kept rather than dropped.** A stream full of units nothing recognises is a finding,
    /// and one silently discarded looks like a stream that carried nothing at all.
    Other(u8),
}

impl Kind {
    /// What the low five bits of the first byte mean.
    ///
    /// From the published codec specification: 1 is a non-keyframe slice, 5 a keyframe slice,
    /// 6 supplemental, 7 sequence parameters, 8 picture parameters.
    #[must_use]
    pub const fn of(header: u8) -> Self {
        match header & 0x1f {
            1 => Self::Picture,
            5 => Self::Keyframe,
            6 => Self::Extra,
            7 => Self::Sequence,
            8 => Self::Picture_,
            other => Self::Other(other),
        }
    }

    /// Whether a decoder could begin from this.
    #[must_use]
    pub const fn starts_a_decode(self) -> bool {
        matches!(self, Self::Keyframe)
    }

    /// What to call it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Keyframe => "keyframe",
            Self::Picture => "picture",
            Self::Sequence => "sequence parameters",
            Self::Picture_ => "picture parameters",
            Self::Extra => "supplemental",
            Self::Other(_) => "unrecognised",
        }
    }
}

/// One unit, as it sat in the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// What it is for.
    pub kind: Kind,
    /// How long it was, excluding the start code.
    pub bytes: usize,
}

/// Accumulates a stream and yields whole units.
///
/// **Fed in whatever pieces arrive.** A socket read has no relationship to unit boundaries,
/// so the only correct design is one that holds a partial unit until the next start code
/// proves it complete.
#[derive(Debug, Default)]
pub struct Reader {
    held: Vec<u8>,
    /// How many units have come out.
    pub units: u64,
    /// How many of them a decoder could have started from.
    pub keyframes: u64,
}

impl Reader {
    /// A reader that has seen nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            held: Vec::new(),
            units: 0,
            keyframes: 0,
        }
    }

    /// Takes more of the stream and returns whatever became complete.
    ///
    /// The last unit is **never** returned by this: nothing after it has proved it whole. See
    /// [`Reader::finish`], which is the caller saying the stream ended.
    pub fn feed(&mut self, more: &[u8]) -> Vec<Unit> {
        self.held.extend_from_slice(more);
        let mut out = Vec::new();
        let mut starts = start_codes(&self.held);
        // Nothing can be complete until there are two start codes: one opening a unit and one
        // proving where it ended.
        while starts.len() >= 2 {
            let (at, skip) = starts[0];
            let (next, _) = starts[1];
            let body = &self.held[at + skip..next];
            if let Some(unit) = unit_of(body) {
                self.units = self.units.saturating_add(1);
                if unit.kind.starts_a_decode() {
                    self.keyframes = self.keyframes.saturating_add(1);
                }
                out.push(unit);
            }
            self.held.drain(..next);
            starts = start_codes(&self.held);
        }
        out
    }

    /// Says the stream ended, so the last held unit is complete after all.
    ///
    /// **Separate from [`Reader::feed`] on purpose.** While a stream is open, a held unit
    /// might still be growing, and emitting it early would report a truncated access unit as a
    /// whole one - which sends somebody to the encoder for a network's fault.
    pub fn finish(&mut self) -> Option<Unit> {
        let starts = start_codes(&self.held);
        let (at, skip) = *starts.first()?;
        let unit = unit_of(&self.held[at + skip..])?;
        self.held.clear();
        self.units = self.units.saturating_add(1);
        if unit.kind.starts_a_decode() {
            self.keyframes = self.keyframes.saturating_add(1);
        }
        Some(unit)
    }

    /// How many bytes are held, waiting for a boundary.
    ///
    /// **Worth exposing.** A number that climbs and never falls is a stream that has stopped
    /// producing start codes, which looks exactly like a stream that has stopped - and the two
    /// need different work.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.held.len()
    }

    /// Whether anything a decoder could begin from has gone past.
    ///
    /// **The question a black window actually asks.** A stream of nothing but dependent
    /// pictures decodes to nothing at all, and looks identical to no stream.
    #[must_use]
    pub const fn could_have_shown_anything(&self) -> bool {
        self.keyframes > 0
    }
}

/// Every start code in the buffer, as an offset and its length.
fn start_codes(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut at = 0;
    while at + 3 <= bytes.len() {
        if bytes[at] == 0 && bytes[at + 1] == 0 {
            if bytes[at + 2] == 1 {
                found.push((at, 3));
                at += 3;
                continue;
            }
            if at + 4 <= bytes.len() && bytes[at + 2] == 0 && bytes[at + 3] == 1 {
                found.push((at, 4));
                at += 4;
                continue;
            }
        }
        at += 1;
    }
    found
}

/// Reads a unit out of its body, if there is one.
fn unit_of(body: &[u8]) -> Option<Unit> {
    let header = *body.first()?;
    Some(Unit {
        kind: Kind::of(header),
        bytes: body.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::{Kind, Reader};

    /// Builds a unit the way an encoder emits one: a start code, a header byte, a body.
    fn unit(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 1, kind];
        out.extend_from_slice(body);
        out
    }

    /// The units come out, in order, with their kinds.
    #[test]
    fn a_stream_yields_its_units() {
        let mut stream = Vec::new();
        stream.extend(unit(7, b"sequence"));
        stream.extend(unit(8, b"picture"));
        stream.extend(unit(5, b"key"));
        stream.extend(unit(1, b"delta"));

        let mut reader = Reader::new();
        let mut got = reader.feed(&stream);
        got.extend(reader.finish());

        let kinds: Vec<Kind> = got.iter().map(|unit| unit.kind).collect();
        assert_eq!(
            kinds,
            [
                Kind::Sequence,
                Kind::Picture_,
                Kind::Keyframe,
                Kind::Picture
            ]
        );
        assert_eq!(reader.units, 4);
        assert_eq!(reader.keyframes, 1);
    }

    /// **A unit split across reads is held, not emitted short.**
    ///
    /// The normal case for a socket, and the one where getting it wrong reports a truncated
    /// access unit as a whole one - sending somebody to the encoder for a network's fault.
    #[test]
    fn a_unit_split_across_reads_arrives_whole() {
        let whole = {
            let mut out = unit(5, b"aaaabbbbcccc");
            out.extend(unit(1, b"d"));
            out
        };
        let (first, second) = whole.split_at(9);

        let mut reader = Reader::new();
        let early = reader.feed(first);
        assert!(early.is_empty(), "nothing is complete yet: {early:?}");
        assert!(reader.pending() > 0, "it is holding the partial unit");

        let then = reader.feed(second);
        assert_eq!(then.len(), 1, "the keyframe completes when the next begins");
        assert_eq!(then[0].kind, Kind::Keyframe);
        assert_eq!(then[0].bytes, 13, "header plus twelve, whole");
    }

    /// Both start-code lengths are recognised, because encoders emit both.
    #[test]
    fn three_byte_and_four_byte_start_codes_both_work() {
        let mut stream = vec![0, 0, 1, 5];
        stream.extend_from_slice(b"key");
        stream.extend_from_slice(&[0, 0, 0, 1, 1]);
        stream.extend_from_slice(b"delta");

        let mut reader = Reader::new();
        let mut got = reader.feed(&stream);
        got.extend(reader.finish());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].kind, Kind::Keyframe);
        assert_eq!(got[1].kind, Kind::Picture);
    }

    /// **The last unit needs the stream to end before it is whole.**
    ///
    /// Held while the stream is open, because a unit that is still growing and one that has
    /// finished look identical from inside a read.
    #[test]
    fn the_final_unit_waits_for_the_stream_to_end() {
        let mut reader = Reader::new();
        assert!(reader.feed(&unit(5, b"only one")).is_empty());
        assert_eq!(reader.units, 0, "nothing has been proved complete");

        let last = reader.finish().expect("the stream ended, so it is whole");
        assert_eq!(last.kind, Kind::Keyframe);
        assert_eq!(reader.units, 1);
    }

    /// **A stream with no keyframe decodes to nothing and looks like no stream.**
    ///
    /// The question a black window is actually asking, and the reason this counts them.
    #[test]
    fn a_stream_of_dependent_pictures_says_it_could_show_nothing() {
        let mut stream = Vec::new();
        for _ in 0..5 {
            stream.extend(unit(1, b"delta"));
        }
        let mut reader = Reader::new();
        reader.feed(&stream);
        reader.finish();

        assert_eq!(reader.units, 5, "it is carrying data");
        assert!(
            !reader.could_have_shown_anything(),
            "and none of it could start a decode"
        );
    }

    /// A unit type this does not name is carried rather than dropped - a stream full of them
    /// is a finding, and silently discarding them looks like a stream that carried nothing.
    #[test]
    fn an_unrecognised_unit_is_kept() {
        let mut reader = Reader::new();
        let mut stream = unit(24, b"whatever");
        stream.extend(unit(1, b"x"));
        let got = reader.feed(&stream);
        assert_eq!(got[0].kind, Kind::Other(24));
        assert_eq!(reader.units, 1);
    }

    /// Bytes that are not a stream produce nothing and hold everything, which is what a
    /// climbing `pending` is for.
    #[test]
    fn something_that_is_not_a_stream_produces_no_units() {
        let mut reader = Reader::new();
        assert!(reader.feed(b"HTTP/1.1 404 Not Found\r\n\r\n").is_empty());
        assert_eq!(reader.units, 0);
        assert!(
            reader.pending() > 0,
            "it is all held, which is how a caller notices"
        );
    }
}
