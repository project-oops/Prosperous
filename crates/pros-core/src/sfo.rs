//! Reading the parameter files a target keeps beside its saves.
//!
//! # What this is for
//!
//! A save is encrypted and signed for the account that wrote it, so sending one to a target
//! it did not come from needs it decrypting and re-signing first. The two cases - a plain
//! copy and one that needs work - look identical while they are happening, and differ only
//! later, when a target refuses a save somebody was relying on.
//!
//! **The account is written down in the save's own metadata.** `ACCOUNT_ID` sits in the
//! `.sfo` file the target keeps beside the save, and reading it is better than anything this
//! project could record for itself: it is true for a save that arrived from anywhere, not
//! only for one this tool copied.
//!
//! # And it is not always there
//!
//! Measured on a target with three saves: one carried `.sfo` files, two carried only icons.
//! So this answers *sometimes*, and a design that treated it as the whole answer would work
//! for one save in three and fail quietly for the rest.
//!
//! That is why it is one source among three, in order of authority - the save's own metadata,
//! then what was recorded when the copy was made, then nothing at all. See
//! [`crate::origin::needed`].
//!
//! # The format
//!
//! `\0PSF`, a version, offsets to a key table and a data table, and a count. Then one
//! sixteen-byte index entry per parameter: where its key is, what kind it is, how long the
//! value is, how much room it has, and where the value is.
//!
//! Only what is needed is read. This is not a general parser and does not try to be one - it
//! answers *which account*, and refuses rather than guessing when it cannot.

use std::collections::BTreeMap;

/// What the file starts with.
const MAGIC: &[u8; 4] = b"\0PSF";

/// Where the header ends and the index entries start.
const INDEX: usize = 20;

/// How long one index entry is.
const ENTRY: usize = 16;

/// A parameter's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// Text, with the trailing zero removed.
    Text(String),
    /// A number.
    Number(u32),
    /// Bytes this makes no attempt to interpret.
    ///
    /// **`ACCOUNT_ID` is one of these.** It is an eight-byte identifier, not a number anybody
    /// should be doing arithmetic on, and rendering it as one would produce a different-looking
    /// value on a different-endian machine for no benefit.
    Binary(Vec<u8>),
}

impl Value {
    /// The text, when it is text.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The bytes as lowercase hex, for anything meant to be compared rather than read.
    #[must_use]
    pub fn hex(&self) -> Option<String> {
        match self {
            Self::Binary(bytes) => Some(bytes.iter().fold(String::new(), |mut out, byte| {
                use std::fmt::Write;
                let _ = write!(out, "{byte:02x}");
                out
            })),
            _ => None,
        }
    }
}

/// Why a file was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotParameters {
    /// It does not begin the way one of these does.
    NotAnSfo,
    /// It does, and then stops in the middle.
    ///
    /// **Distinct from not being one at all**, because a truncated parameter file is a
    /// transfer that went wrong and a file of some other kind is somebody looking in the
    /// wrong place. Different problems, different next steps.
    Truncated,
}

impl std::fmt::Display for NotParameters {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnSfo => write!(out, "not a parameter file - it does not start with \\0PSF"),
            Self::Truncated => write!(out, "a parameter file that stops part way through"),
        }
    }
}

impl std::error::Error for NotParameters {}

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

/// Reads the parameters out of a file.
///
/// # Errors
///
/// [`NotParameters::NotAnSfo`] when it is some other kind of file, and
/// [`NotParameters::Truncated`] when the header is right and the rest does not follow.
///
/// **An entry that cannot be read is skipped rather than failing the file.** One unreadable
/// parameter among fifteen should not lose the fourteen that were fine, and the one that
/// matters here is either present or it is not - which the caller can see.
pub fn read(bytes: &[u8]) -> Result<BTreeMap<String, Value>, NotParameters> {
    if bytes.len() < INDEX || !bytes.starts_with(MAGIC) {
        return Err(NotParameters::NotAnSfo);
    }
    let keys = u32_at(bytes, 8).ok_or(NotParameters::Truncated)? as usize;
    let data = u32_at(bytes, 12).ok_or(NotParameters::Truncated)? as usize;
    let count = u32_at(bytes, 16).ok_or(NotParameters::Truncated)? as usize;

    let mut found = BTreeMap::new();
    for entry in 0..count {
        let at = INDEX + entry * ENTRY;
        let Some(key_at) = u16_at(bytes, at) else {
            break;
        };
        let Some(kind) = u16_at(bytes, at + 2) else {
            break;
        };
        let Some(length) = u32_at(bytes, at + 4) else {
            break;
        };
        let Some(data_at) = u32_at(bytes, at + 12) else {
            break;
        };

        let Some(key) = key_at_offset(bytes, keys + key_at as usize) else {
            continue;
        };
        let from = data + data_at as usize;
        let Some(raw) = bytes.get(from..from + length as usize) else {
            continue;
        };
        // 0x0204 is text, 0x0404 is a number, and everything else is bytes this does not
        // pretend to understand.
        let value = match kind {
            0x0204 => Value::Text(
                String::from_utf8_lossy(raw)
                    .trim_end_matches('\0')
                    .to_owned(),
            ),
            0x0404 => match u32_at(bytes, from) {
                Some(number) => Value::Number(number),
                None => continue,
            },
            _ => Value::Binary(raw.to_vec()),
        };
        found.insert(key, value);
    }
    Ok(found)
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
/// way round: refusing is visible and corrupting is not.
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

/// The account a save belongs to, as hex.
///
/// `None` when the file does not carry one - which is a real state and not an error. Two of
/// three saves measured on a target had no parameter file at all.
#[must_use]
pub fn account_id(parameters: &BTreeMap<String, Value>) -> Option<String> {
    parameters.get("ACCOUNT_ID")?.hex()
}

/// What the save is called, for showing beside it.
#[must_use]
pub fn title(parameters: &BTreeMap<String, Value>) -> Option<&str> {
    parameters.get("MAINTITLE")?.text()
}

#[cfg(test)]
mod tests {
    use super::{NotParameters, Value, account_id, read, title};

    /// Builds a parameter file, so the parser is tested against the format rather than
    /// against one target's file - which could not be committed here anyway, carrying
    /// somebody's account identifier as it does.
    fn sfo(entries: &[(&str, Value)]) -> Vec<u8> {
        let mut keys: Vec<u8> = Vec::new();
        let mut data: Vec<u8> = Vec::new();
        let mut index: Vec<u8> = Vec::new();

        for (key, value) in entries {
            let key_at = u16::try_from(keys.len()).expect("small");
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);

            let data_at = u32::try_from(data.len()).expect("small");
            let (kind, raw) = match value {
                Value::Text(text) => {
                    let mut bytes = text.clone().into_bytes();
                    bytes.push(0);
                    (0x0204_u16, bytes)
                }
                Value::Number(number) => (0x0404, number.to_le_bytes().to_vec()),
                Value::Binary(bytes) => (0x0004, bytes.clone()),
            };
            let length = u32::try_from(raw.len()).expect("small");
            data.extend_from_slice(&raw);

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

    /// **The account comes out as hex, byte for byte.**
    ///
    /// Not as a number: it is an identifier to compare, and reading it as an integer would
    /// give a different answer on a machine of the other endianness for no gain at all.
    #[test]
    fn the_account_is_read_as_the_bytes_it_is() {
        let bytes = sfo(&[
            (
                "ACCOUNT_ID",
                Value::Binary(vec![0x76, 0x9f, 0x77, 0x71, 0x69, 0x58, 0xd3, 0x7e]),
            ),
            ("TITLE_ID", Value::Text("PPSA01650".to_owned())),
            ("MAINTITLE", Value::Text("Saved Data".to_owned())),
            ("SAVEDATA_BLOCKS", Value::Number(96)),
        ]);

        let parameters = read(&bytes).expect("it reads");
        assert_eq!(account_id(&parameters).as_deref(), Some("769f77716958d37e"));
        assert_eq!(title(&parameters), Some("Saved Data"));
        assert_eq!(
            parameters.get("TITLE_ID").and_then(Value::text),
            Some("PPSA01650")
        );
        assert_eq!(parameters.get("SAVEDATA_BLOCKS"), Some(&Value::Number(96)));
    }

    /// **A save with no account in its parameters says so**, rather than producing something
    /// that would compare equal to another save with none.
    #[test]
    fn a_file_without_an_account_offers_none() {
        let bytes = sfo(&[("TITLE_ID", Value::Text("PPSA10528".to_owned()))]);
        let parameters = read(&bytes).expect("it reads");
        assert_eq!(account_id(&parameters), None);
    }

    /// Some other file is refused, and refused differently from a damaged one.
    #[test]
    fn something_that_is_not_a_parameter_file_is_refused() {
        assert_eq!(
            read(b"not this at all").unwrap_err(),
            NotParameters::NotAnSfo
        );
        assert_eq!(read(&[]).unwrap_err(), NotParameters::NotAnSfo);
    }

    /// A file that starts right and then stops is a transfer that went wrong, which is a
    /// different problem from a file of the wrong kind.
    #[test]
    fn a_file_that_stops_part_way_is_a_different_complaint() {
        assert_eq!(
            read(b"\0PSF\x01\x01\0\0\x14").unwrap_err(),
            NotParameters::NotAnSfo,
            "too short even for a header"
        );
        let mut bytes = sfo(&[("TITLE_ID", Value::Text("PPSA01650".to_owned()))]);
        bytes.truncate(21);
        // The header survives, the entries do not: what can be read is returned rather than
        // losing everything, and the missing key is simply absent.
        let parameters = read(&bytes).expect("the header is intact");
        assert!(parameters.is_empty());
    }

    /// **A value is written over the old one and nothing moves.**
    ///
    /// Everything the file carries that this code does not understand comes through
    /// untouched, because it was never taken apart.
    #[test]
    fn a_parameter_is_replaced_without_disturbing_the_others() {
        let mut bytes = sfo(&[
            ("ACCOUNT_ID", Value::Binary(vec![0; 8])),
            ("TITLE_ID", Value::Text("PPSA03420".to_owned())),
            ("MAINTITLE", Value::Text("Grand Theft Auto V".to_owned())),
        ]);
        let before = bytes.len();

        super::set_text(&mut bytes, "TITLE_ID", "PPSA01721").expect("same length, fits");
        assert_eq!(bytes.len(), before, "the file should not have grown");

        let read_back = read(&bytes).expect("still reads");
        assert_eq!(
            read_back.get("TITLE_ID").and_then(Value::text),
            Some("PPSA01721")
        );
        assert_eq!(
            read_back.get("MAINTITLE").and_then(Value::text),
            Some("Grand Theft Auto V"),
            "its neighbour is untouched"
        );
    }

    /// **A shorter value does not leave the tail of the old one behind.**
    ///
    /// The slot is cleared first. Without that the bytes still read correctly through this
    /// parser, which takes the recorded length - and wrongly through anything that reads to
    /// the terminator instead.
    #[test]
    fn a_shorter_value_does_not_leave_the_old_one_showing() {
        let mut bytes = sfo(&[("SUBTITLE", Value::Text("Franklin and Lamar".to_owned()))]);
        super::set_text(&mut bytes, "SUBTITLE", "Prologue").expect("shorter, fits");

        let read_back = read(&bytes).expect("still reads");
        assert_eq!(
            read_back.get("SUBTITLE").and_then(Value::text),
            Some("Prologue")
        );
        // The old tail would be here if the slot had not been cleared.
        assert!(
            !String::from_utf8_lossy(&bytes).contains("Lamar"),
            "the old value is still in the file"
        );
    }

    /// **A value too long for its room is refused**, rather than written over its neighbour.
    #[test]
    fn a_value_that_does_not_fit_is_refused() {
        let mut bytes = sfo(&[("TITLE_ID", Value::Text("PPSA03420".to_owned()))]);
        let refused = super::set_text(&mut bytes, "TITLE_ID", "PPSA03420-far-too-long")
            .expect_err("it does not fit");
        assert!(
            matches!(refused, super::NotChanged::TooLong { .. }),
            "{refused:?}"
        );
        assert_eq!(
            read(&bytes)
                .expect("unchanged")
                .get("TITLE_ID")
                .and_then(Value::text),
            Some("PPSA03420"),
            "and nothing was written"
        );
    }

    /// **A parameter the file does not have is refused, not added.**
    ///
    /// Adding one moves every offset after it, and a rebuilt file whose offsets are wrong
    /// still parses - so the mistake would surface as a target rejecting the save.
    #[test]
    fn a_parameter_that_is_not_there_is_not_invented() {
        let mut bytes = sfo(&[("TITLE_ID", Value::Text("PPSA03420".to_owned()))]);
        let refused = super::set_text(&mut bytes, "SUBTITLE", "anything").expect_err("absent");
        assert!(
            matches!(refused, super::NotChanged::Absent(_)),
            "{refused:?}"
        );
    }

    /// The account is bytes, not text, and writing it as text is refused.
    #[test]
    fn the_kind_has_to_match() {
        let mut bytes = sfo(&[("ACCOUNT_ID", Value::Binary(vec![0; 8]))]);
        let refused = super::set_text(&mut bytes, "ACCOUNT_ID", "769f7771").expect_err("binary");
        assert!(
            matches!(refused, super::NotChanged::WrongKind(_)),
            "{refused:?}"
        );

        let account = [0x76, 0x9f, 0x77, 0x71, 0x69, 0x58, 0xd3, 0x7e];
        super::set(&mut bytes, "ACCOUNT_ID", &account, false).expect("as bytes it fits");
        assert_eq!(
            account_id(&read(&bytes).expect("reads")).as_deref(),
            Some("769f77716958d37e")
        );
    }
}
