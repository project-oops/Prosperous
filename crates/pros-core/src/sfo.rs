//! Editing and comparing a save's parameter file, over SELFish's reader for the format.
//!
//! # The format is SELFish's; this is only what Prosperous adds
//!
//! A save is encrypted and signed for the account that wrote it, so sending one to a target it
//! did not come from needs it decrypting and re-signing first. The account is written down in
//! the save's own metadata: `ACCOUNT_ID` sits in the `.sfo` parameter file the target keeps
//! beside the save, and reading it is better than anything this project could record for
//! itself - it is true for a save that arrived from anywhere, not only one this tool copied.
//!
//! And it is not always there. Measured on a target with three saves: one carried `.sfo` files,
//! two carried only icons. So it answers *sometimes*, which is why it is one source among three
//! in [`crate::origin::needed`], in order of authority.
//!
//! # Why the parser is gone
//!
//! `param.sfo` is a platform format, and principle 6 says Prosperous reads the platform's
//! formats from SELFish and invents none of its own. This module used to carry its own `\0PSF`
//! parser - the exact duplication [`selfish_title`] exists to prevent - and it is gone: the
//! reading is [`selfish_title::sfo::Sfo`] now, and what stays here is only what Prosperous adds
//! on top of it. (D028)
//!
//! - `account_in` and `account_id` render `ACCOUNT_ID` as hex, the shape an identifier is
//!   *compared* in rather than displayed - never as a number, which would read differently on a
//!   machine of the other endianness for no gain.
//! - `set` rewrites one parameter **in place**, which SELFish's whole-file writer deliberately
//!   is not, and which `graft` depends on. See its own note for why nothing is rebuilt.

/// Where the header ends and the index entries start.
const INDEX: usize = 20;

/// How long one index entry is.
const ENTRY: usize = 16;

/// An identifier rendered for comparison rather than display: lowercase hex, two characters a
/// byte.
///
/// The one rendering every account reader here shares, so two ids compare equal exactly when
/// their bytes do. Not a number: an eight-byte user id read as an integer would come back
/// different on a machine of the other endianness, for a value nobody does arithmetic on.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// The account a parsed parameter file names, as hex, or `None` when it carries none.
///
/// Goes through [`selfish_title::sfo::Sfo::bytes`], which answers with the value's raw bytes
/// whichever variant the parse chose - so an id that happens to decode as text is still read as
/// the eight bytes it is, not the string it looked like.
#[must_use]
pub fn account_id(sfo: &selfish_title::sfo::Sfo) -> Option<String> {
    sfo.bytes("ACCOUNT_ID").map(hex)
}

/// The account a parameter file's bytes name, as hex - parse and read in one step.
///
/// The operation `origin` and `saves` share: `None` when the bytes are not a parameter file, or
/// are one that carries no account. Both are real states and neither is an error - two of three
/// saves measured on a target had no parameter file at all.
#[must_use]
pub fn account_in(bytes: &[u8]) -> Option<String> {
    account_id(&selfish_title::sfo::Sfo::parse(bytes).ok()?)
}

/// Reads four bytes as a number, if they are there.
fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|four| four.try_into().ok())
        .map(u32::from_le_bytes)
}

/// Reads two bytes as a number, if they are there.
fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at + 2)
        .and_then(|two| two.try_into().ok())
        .map(u16::from_le_bytes)
}

/// The zero-terminated key at an offset.
fn key_at_offset(bytes: &[u8], at: usize) -> Option<String> {
    let rest = bytes.get(at..)?;
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(rest.len());
    let key = String::from_utf8_lossy(&rest[..end]).into_owned();
    (!key.is_empty()).then_some(key)
}

/// Why a parameter could not be changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotChanged {
    /// The file does not carry that parameter.
    ///
    /// **Not added.** Adding one means moving every offset after it, and a parameter file
    /// this project rebuilt rather than edited is one where a mistake is invisible: it would
    /// still parse, and the target would reject the save with no clue which byte was wrong.
    Absent(String),
    /// The new value is longer than the room the file left for it.
    ///
    /// Each entry records how much space it has. Writing past it would overwrite whatever
    /// comes next, which is another parameter.
    TooLong {
        /// What was being written.
        key: String,
        /// How many bytes it needed.
        needed: usize,
        /// How many it had.
        room: usize,
    },
    /// It is there and it is not the kind of thing being written.
    WrongKind(String),
}

impl std::fmt::Display for NotChanged {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Absent(key) => write!(out, "no {key} in this parameter file to change"),
            Self::TooLong { key, needed, room } => write!(
                out,
                "{key} needs {needed} bytes and the file left room for {room}"
            ),
            Self::WrongKind(key) => write!(out, "{key} is not the kind of value being written"),
        }
    }
}

impl std::error::Error for NotChanged {}

/// Replaces one parameter's bytes, in place.
///
/// # Why in place, and never by rebuilding
///
/// A parameter file is a table of offsets. Rewriting it means recomputing every one of them,
/// and a file this project assembled rather than edited would still parse - so a mistake in
/// it is invisible until a target refuses the save, with nothing pointing at which byte.
///
/// **So nothing moves.** A value is written over the old one, within the room the file already
/// left for it, and everything this code does not understand is untouched by construction.
/// The cost is that a longer value is refused rather than accommodated, which is the right
/// way round: refusing is visible and corrupting is not. It is also why this stays here rather
/// than moving to SELFish with the reader: [`selfish_title::sfo::Sfo::to_bytes`] rebuilds the
/// file, which is the opposite call, and `graft` wants this one.
///
/// # Errors
///
/// [`NotChanged`] when the parameter is absent, too long for its room, or a different kind.
pub fn set(bytes: &mut [u8], key: &str, value: &[u8], text: bool) -> Result<(), NotChanged> {
    let (data_table, count) = match (u32_at(bytes, 12), u32_at(bytes, 16)) {
        (Some(data), Some(count)) => (data as usize, count as usize),
        _ => return Err(NotChanged::Absent(key.to_owned())),
    };
    let keys = u32_at(bytes, 8).ok_or_else(|| NotChanged::Absent(key.to_owned()))? as usize;

    for entry in 0..count {
        let at = INDEX + entry * ENTRY;
        let (Some(key_at), Some(kind), Some(room), Some(data_at)) = (
            u16_at(bytes, at),
            u16_at(bytes, at + 2),
            u32_at(bytes, at + 8),
            u32_at(bytes, at + 12),
        ) else {
            break;
        };
        if key_at_offset(bytes, keys + key_at as usize).as_deref() != Some(key) {
            continue;
        }
        // Text is 0x0204; anything else here is bytes. A number is not editable this way and
        // says so rather than being written as four arbitrary bytes.
        if text != (kind == 0x0204) {
            return Err(NotChanged::WrongKind(key.to_owned()));
        }
        // Text carries its terminator inside its length, so the room has to hold it too.
        let needed = value.len() + usize::from(text);
        let room = room as usize;
        if needed > room {
            return Err(NotChanged::TooLong {
                key: key.to_owned(),
                needed,
                room,
            });
        }

        let from = data_table + data_at as usize;
        let Some(slot) = bytes.get_mut(from..from + room) else {
            return Err(NotChanged::Absent(key.to_owned()));
        };
        // The whole slot is cleared first: a shorter value would otherwise leave the tail of
        // the old one behind it, which for text is a string that reads correctly here and
        // wrongly wherever the length is taken from the file instead.
        slot.fill(0);
        slot[..value.len()].copy_from_slice(value);

        // The recorded length follows the value. The room does not change - it is what the
        // file was built with and is not this code's to alter.
        let length = u32::try_from(needed).unwrap_or(u32::MAX);
        bytes[at + 4..at + 8].copy_from_slice(&length.to_le_bytes());
        return Ok(());
    }
    Err(NotChanged::Absent(key.to_owned()))
}

/// Replaces a text parameter.
///
/// # Errors
///
/// As [`crate::sfo::set`].
pub fn set_text(bytes: &mut [u8], key: &str, value: &str) -> Result<(), NotChanged> {
    set(bytes, key, value.as_bytes(), true)
}

#[cfg(test)]
mod tests {
    use super::{NotChanged, account_id, account_in, hex, set, set_text};
    use selfish_title::sfo::Sfo;

    /// The two index kinds these tests use: text carries a terminator inside its length,
    /// bytes do not.
    const TEXT: u16 = 0x0204;
    const BYTES: u16 = 0x0004;

    /// Builds a parameter file from `(key, kind, value-bytes)`, so the writer is tested against
    /// the format rather than one target's file - which could not be committed here anyway,
    /// carrying somebody's account identifier as it does. The recorded length and the room are
    /// both the value's length, which is the tight case [`set`] refuses to grow past.
    fn sfo(entries: &[(&str, u16, Vec<u8>)]) -> Vec<u8> {
        let mut keys: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();
        let mut index: Vec<u8> = Vec::new();

        for (key, kind, raw) in entries {
            let key_at = u16::try_from(keys.len()).expect("small");
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);

            let data_at = u32::try_from(data.len()).expect("small");
            let length = u32::try_from(raw.len()).expect("small");
            data.extend_from_slice(raw);

            index.extend_from_slice(&key_at.to_le_bytes());
            index.extend_from_slice(&kind.to_le_bytes());
            index.extend_from_slice(&length.to_le_bytes());
            index.extend_from_slice(&length.to_le_bytes());
            index.extend_from_slice(&data_at.to_le_bytes());
        }

        let key_table = u32::try_from(20 + index.len()).expect("small");
        let data_table = key_table + u32::try_from(keys.len()).expect("small");
        let mut out = Vec::new();
        out.extend_from_slice(b"\0PSF");
        out.extend_from_slice(&0x0101_u32.to_le_bytes());
        out.extend_from_slice(&key_table.to_le_bytes());
        out.extend_from_slice(&data_table.to_le_bytes());
        out.extend_from_slice(&u32::try_from(entries.len()).expect("small").to_le_bytes());
        out.extend_from_slice(&index);
        out.extend_from_slice(&keys);
        out.extend_from_slice(&data);
        out
    }

    /// A NUL-terminated text value, the shape the `0x0204` format states.
    fn terminated(text: &str) -> Vec<u8> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        bytes
    }

    /// **The account comes out as hex, byte for byte** - read through SELFish, which is where
    /// the parsing lives now.
    #[test]
    fn the_account_is_read_as_the_bytes_it_is() {
        let bytes = sfo(&[
            (
                "ACCOUNT_ID",
                BYTES,
                vec![0x76, 0x9f, 0x77, 0x71, 0x69, 0x58, 0xd3, 0x7e],
            ),
            ("TITLE_ID", TEXT, terminated("PPSA01650")),
        ]);

        assert_eq!(account_in(&bytes).as_deref(), Some("769f77716958d37e"));

        let sfo = Sfo::parse(&bytes).expect("selfish-title reads it");
        assert_eq!(account_id(&sfo).as_deref(), Some("769f77716958d37e"));
        assert_eq!(sfo.text("TITLE_ID"), Some("PPSA01650"));
    }

    /// **A save with no account says so**, rather than producing something that would compare
    /// equal to another save with none.
    #[test]
    fn a_file_without_an_account_offers_none() {
        let bytes = sfo(&[("TITLE_ID", TEXT, terminated("PPSA10528"))]);
        assert_eq!(account_in(&bytes), None);
    }

    /// Bytes that are not a parameter file are no account, not a panic.
    #[test]
    fn what_is_not_a_parameter_file_is_no_account() {
        assert_eq!(account_in(b"not this at all"), None);
        assert_eq!(account_in(&[]), None);
    }

    /// `hex` renders lowercase and zero-padded, two characters a byte - the shape ids compare in.
    #[test]
    fn hex_is_lowercase_and_padded() {
        assert_eq!(hex(&[0x00, 0x9f, 0x0a]), "009f0a");
    }

    /// **A value is written over the old one and nothing moves**, verified by reading the
    /// result back through SELFish.
    #[test]
    fn a_parameter_is_replaced_without_disturbing_the_others() {
        let mut bytes = sfo(&[
            ("ACCOUNT_ID", BYTES, vec![0; 8]),
            ("TITLE_ID", TEXT, terminated("PPSA03420")),
            ("MAINTITLE", TEXT, terminated("Grand Theft Auto V")),
        ]);
        let before = bytes.len();

        set_text(&mut bytes, "TITLE_ID", "PPSA01721").expect("same length, fits");
        assert_eq!(bytes.len(), before, "the file should not have grown");

        let sfo = Sfo::parse(&bytes).expect("still reads");
        assert_eq!(sfo.text("TITLE_ID"), Some("PPSA01721"));
        assert_eq!(
            sfo.text("MAINTITLE"),
            Some("Grand Theft Auto V"),
            "its neighbour is untouched"
        );
    }

    /// **A shorter value does not leave the tail of the old one behind.**
    ///
    /// The slot is cleared first. Without that the bytes still read correctly through a parser
    /// that takes the recorded length, and wrongly through anything that reads to the
    /// terminator instead.
    #[test]
    fn a_shorter_value_does_not_leave_the_old_one_showing() {
        let mut bytes = sfo(&[("SUBTITLE", TEXT, terminated("Franklin and Lamar"))]);
        set_text(&mut bytes, "SUBTITLE", "Prologue").expect("shorter, fits");

        let sfo = Sfo::parse(&bytes).expect("still reads");
        assert_eq!(sfo.text("SUBTITLE"), Some("Prologue"));
        // The old tail would be here if the slot had not been cleared.
        assert!(
            !String::from_utf8_lossy(&bytes).contains("Lamar"),
            "the old value is still in the file"
        );
    }

    /// **A value too long for its room is refused**, rather than written over its neighbour.
    #[test]
    fn a_value_that_does_not_fit_is_refused() {
        let mut bytes = sfo(&[("TITLE_ID", TEXT, terminated("PPSA03420"))]);
        let refused = set_text(&mut bytes, "TITLE_ID", "PPSA03420-far-too-long")
            .expect_err("it does not fit");
        assert!(matches!(refused, NotChanged::TooLong { .. }), "{refused:?}");

        let sfo = Sfo::parse(&bytes).expect("unchanged");
        assert_eq!(
            sfo.text("TITLE_ID"),
            Some("PPSA03420"),
            "and nothing was written"
        );
    }

    /// **A parameter the file does not have is refused, not added.**
    #[test]
    fn a_parameter_that_is_not_there_is_not_invented() {
        let mut bytes = sfo(&[("TITLE_ID", TEXT, terminated("PPSA03420"))]);
        let refused = set_text(&mut bytes, "SUBTITLE", "anything").expect_err("absent");
        assert!(matches!(refused, NotChanged::Absent(_)), "{refused:?}");
    }

    /// The account is bytes, not text: writing it as text is refused, and as bytes it fits.
    #[test]
    fn the_kind_has_to_match() {
        let mut bytes = sfo(&[("ACCOUNT_ID", BYTES, vec![0; 8])]);
        let refused = set_text(&mut bytes, "ACCOUNT_ID", "769f7771").expect_err("binary");
        assert!(matches!(refused, NotChanged::WrongKind(_)), "{refused:?}");

        let account = [0x76, 0x9f, 0x77, 0x71, 0x69, 0x58, 0xd3, 0x7e];
        set(&mut bytes, "ACCOUNT_ID", &account, false).expect("as bytes it fits");
        assert_eq!(account_in(&bytes).as_deref(), Some("769f77716958d37e"));
    }
}
