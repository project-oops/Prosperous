//! What a file is, decided before it is sent anywhere.
//!
//! # The mistake this module exists to prevent
//!
//! A vendor-format module and a plain payload **share their first four bytes**. Both begin
//! `7f 45 4c 46`, because both are ELF. The target loader checks exactly that much,
//! accepts either, maps the one it cannot run, and dies without printing anything - the
//! entry point it jumps to expects tens of thousands of resolved imports that nobody
//! resolved.
//!
//! From the outside that is indistinguishable from a payload that ran and did nothing,
//! which is the worst shape a failure can have. So the check happens on this side, before
//! a byte goes out.
//!
//! # Where the numbers come from
//!
//! `e_type` sits at offset `0x10` of an ELF header and is two bytes, little-endian.
//!
//! **The two vendor values are easy to swap, and swapping them is not hypothetical.** The
//! sibling project had them named the wrong way round for months, so its module builder
//! wrote the library type while claiming the executable one, and its documentation
//! repeated the name back as fact. Every loader that checked accepted the file and then
//! declined to run it, which looks exactly like loading a module and not entering it.
//!
//! They are written here with what each one *is*, not with what it is called.

/// Offset of `e_type` in an ELF header.
const E_TYPE: usize = 0x10;

/// The four bytes every ELF begins with, and the reason this module is needed.
const MAGIC: [u8; 4] = [0x7f, 0x45, 0x4c, 0x46];

/// An ordinary shared object - what a linker produces, and what a payload loader runs.
const ET_DYN: u16 = 0x0003;

/// The vendor position-independent **executable**: what a title binary is, and what an
/// emulator fetches an entry point from.
const ET_SCE_DYNEXEC: u16 = 0xFE10;

/// The vendor shared **library**, the `.prx` shape.
///
/// A loader that distinguishes the two runs a library initialisers and then looks
/// elsewhere for something to start. Accepted everywhere, entered only by loaders that do
/// not check.
const ET_SCE_DYNAMIC: u16 = 0xFE18;

/// What a candidate file turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A plain shared object. This is what the target loader runs.
    Payload,
    /// A vendor executable - an emulator input, not a target one.
    VendorExecutable,
    /// A vendor shared library. Entered by nothing, here or there.
    VendorLibrary,
    /// An ELF with an `e_type` this crate has no name for.
    ///
    /// Refused rather than attempted. An unrecognised type is not evidence that it is
    /// harmless, and the loader failure mode for a wrong one is silence.
    UnknownElf(u16),
    /// Not an ELF at all.
    NotElf,
    /// Shorter than an ELF header, so nothing can be read from it.
    TooShort,
}

impl Shape {
    /// What the bytes are, in a sentence.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::Payload => "a payload",
            Self::VendorExecutable => "a vendor executable, not a payload",
            Self::VendorLibrary => "a vendor shared library, not a payload",
            Self::UnknownElf(_) => "an ELF of a type this tool does not recognise",
            Self::NotElf => "not an ELF file",
            Self::TooShort => "too short to be an ELF file",
        }
    }

    /// What to do about it.
    ///
    /// Included because *what is this* and *what do I do now* are different questions, and
    /// somebody holding a vendor module has almost always reached for the wrong file in a
    /// directory containing both.
    #[must_use]
    pub fn remedy(self) -> &'static str {
        match self {
            Self::Payload => "send it",
            Self::VendorExecutable | Self::VendorLibrary => {
                "send it to an emulator; the target loader accepts it and then dies quietly"
            }
            Self::UnknownElf(_) => "check what produced it before sending anything",
            Self::NotElf | Self::TooShort => "check the path",
        }
    }

    /// Whether the target loader can run it.
    #[must_use]
    pub const fn is_payload(self) -> bool {
        matches!(self, Self::Payload)
    }
}

/// Reads what a file is from its own header.
///
/// Reads two fields and nothing else. This is a guard, not a parser: anything more would
/// be a second ELF reader in a project that does not need one, and the sibling projects
/// already have theirs.
#[must_use]
pub fn identify(bytes: &[u8]) -> Shape {
    let Some(head) = bytes.get(..E_TYPE + 2) else {
        return Shape::TooShort;
    };
    if head.get(..4) != Some(&MAGIC[..]) {
        return Shape::NotElf;
    }
    // Two bytes, little-endian, at a fixed offset. Both are present because the slice
    // above is exactly long enough, which is what makes the reads below total.
    let low = head.get(E_TYPE).copied().unwrap_or_default();
    let high = head.get(E_TYPE + 1).copied().unwrap_or_default();
    match u16::from_le_bytes([low, high]) {
        ET_DYN => Shape::Payload,
        ET_SCE_DYNEXEC => Shape::VendorExecutable,
        ET_SCE_DYNAMIC => Shape::VendorLibrary,
        other => Shape::UnknownElf(other),
    }
}

#[cfg(test)]
mod tests {
    use super::{ET_DYN, ET_SCE_DYNAMIC, ET_SCE_DYNEXEC, MAGIC, Shape, identify};

    /// Builds a header with the given `e_type` and nothing else that matters.
    fn header(e_type: u16) -> Vec<u8> {
        let mut bytes = vec![0_u8; 64];
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[0x10..0x12].copy_from_slice(&e_type.to_le_bytes());
        bytes
    }

    /// The whole reason this module exists: these three are indistinguishable to the check
    /// the target loader performs, and only one of them may be sent.
    #[test]
    fn the_three_elf_shapes_are_told_apart() {
        assert_eq!(identify(&header(ET_DYN)), Shape::Payload);
        assert_eq!(identify(&header(ET_SCE_DYNEXEC)), Shape::VendorExecutable);
        assert_eq!(identify(&header(ET_SCE_DYNAMIC)), Shape::VendorLibrary);

        // And every one of them passes a check that reads only the magic, which is what
        // the loader does. This is the assertion that makes the rest necessary.
        for shape in [ET_DYN, ET_SCE_DYNEXEC, ET_SCE_DYNAMIC] {
            assert_eq!(header(shape).get(..4), Some(&MAGIC[..]));
        }
    }

    /// Only a plain shared object is sendable.
    #[test]
    fn only_a_plain_shared_object_is_a_payload() {
        assert!(identify(&header(ET_DYN)).is_payload());
        assert!(!identify(&header(ET_SCE_DYNEXEC)).is_payload());
        assert!(!identify(&header(ET_SCE_DYNAMIC)).is_payload());
        assert!(!identify(&header(0x1234)).is_payload());
    }

    /// An unrecognised type is refused rather than tried, and carries what it was.
    ///
    /// Silence is the loader failure mode, so "this might be fine" is not a bet worth
    /// taking on somebody else target.
    #[test]
    fn an_unknown_type_is_refused_and_says_what_it_saw() {
        assert_eq!(identify(&header(0xBEEF)), Shape::UnknownElf(0xBEEF));
    }

    /// Short and non-ELF inputs are different answers, because they have different fixes.
    #[test]
    fn a_short_file_and_a_wrong_one_are_told_apart() {
        assert_eq!(identify(&[]), Shape::TooShort);
        assert_eq!(identify(&[0x7f, 0x45]), Shape::TooShort);
        assert_eq!(identify(&[0_u8; 64]), Shape::NotElf);
    }

    /// Every shape says what it is and what to do about it.
    ///
    /// A refusal that only says no leaves the reader to work out which of two tools wanted
    /// the file, which is the guess this whole module exists to remove.
    #[test]
    fn every_shape_explains_itself_and_says_what_to_do() {
        for shape in [
            Shape::Payload,
            Shape::VendorExecutable,
            Shape::VendorLibrary,
            Shape::UnknownElf(1),
            Shape::NotElf,
            Shape::TooShort,
        ] {
            assert!(
                !shape.describe().is_empty(),
                "{shape:?} does not describe itself"
            );
            assert!(
                !shape.remedy().is_empty(),
                "{shape:?} does not say what to do"
            );
        }
    }
}
