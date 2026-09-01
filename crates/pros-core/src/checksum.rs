//! Proving a payload is the one that was described, before it is run.
//!
//! # This is the risk surface, in one paragraph
//!
//! A payload is fetched from a mirror somebody else controls and then handed to a loader
//! that runs it with kernel-adjacent privileges. Everything else in this project is
//! convenience; this is the part where being wrong matters. **Verification happens before
//! sending, always, and the ordinary path offers no way past it.**
//!
//! # An algorithm this cannot check is an error, not a shrug
//!
//! The obvious way to write this is to verify what you recognise and pass over what you do
//! not. That produces a tool which reports success for an entry it never checked - the same
//! defect as a probe that cannot fail, applied to the one place where it would matter most.
//!
//! So an unreadable or unsupported checksum fails at the point the manifest is read, naming
//! what it found. A person can then fix the manifest, which is a small job, rather than
//! discover months later that a category of entry was never verified.
//!
//! # Why only one algorithm
//!
//! SHA-256 is what release assets are published with. The payload manager's own repository
//! has a `checksum` field whose format **has not been measured**, so rather than guess at a
//! second algorithm and write code that has never seen a real input, anything else is
//! reported by name. When a real repository is in front of us, the error message will say
//! exactly what to add.

use std::fmt;

use sha2::{Digest as _, Sha256};

/// How a digest was computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// SHA-256, and currently the only one that can be checked.
    Sha256,
}

impl Algorithm {
    /// How many hexadecimal digits its digest has.
    const fn digits(self) -> usize {
        match self {
            Self::Sha256 => 64,
        }
    }

    /// Its name, as a manifest would spell it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sha256 => "sha256",
        }
    }
}

/// A digest a payload is expected to have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    algorithm: Algorithm,
    /// Lower case hexadecimal, normalised on the way in so two spellings of one digest
    /// cannot disagree.
    digest: String,
}

impl Checksum {
    /// Computes the checksum of some bytes.
    ///
    /// For writing a manifest entry, and for saying what was found when one does not match.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        Self {
            algorithm: Algorithm::Sha256,
            digest: hex(&hasher.finalize()),
        }
    }

    /// Reads a checksum as a manifest states it.
    ///
    /// Accepts `sha256:abc...` and a bare digest whose length names the algorithm. Case is
    /// normalised.
    ///
    /// # Errors
    ///
    /// [`Unreadable`] for anything this cannot check, **including digests it recognises but
    /// cannot verify**. See the module note: passing over those would produce a tool that
    /// reports success for entries it never looked at.
    pub fn parse(text: &str) -> Result<Self, Unreadable> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Unreadable::Absent);
        }
        let (named, digest) = match text.split_once(':') {
            Some((named, digest)) => (Some(named.trim().to_ascii_lowercase()), digest.trim()),
            None => (None, text),
        };
        let digest = digest.to_ascii_lowercase();
        if !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Unreadable::NotADigest {
                text: text.to_owned(),
            });
        }

        // A name, when there is one, wins over the length. A file that says `md5:` followed
        // by sixty-four digits is a manifest somebody has already got wrong, and guessing
        // which half to believe would be inventing an answer.
        if let Some(named) = named {
            if named != Algorithm::Sha256.name() || digest.len() != Algorithm::Sha256.digits() {
                return Err(Unreadable::Unsupported {
                    named,
                    digits: digest.len(),
                });
            }
        } else if digest.len() != Algorithm::Sha256.digits() {
            return Err(Unreadable::Unsupported {
                named: guess(digest.len()),
                digits: digest.len(),
            });
        }

        Ok(Self {
            algorithm: Algorithm::Sha256,
            digest,
        })
    }

    /// Which algorithm this is.
    #[must_use]
    pub const fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The digest, lower case hexadecimal.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Checks bytes against this.
    ///
    /// # Errors
    ///
    /// [`Mismatch`], carrying both digests. **Both, not just a verdict**: the one thing
    /// somebody wants when a download fails to verify is whether it is the file that is
    /// wrong or the manifest, and that is a question about two numbers.
    pub fn verify(&self, bytes: &[u8]) -> Result<(), Mismatch> {
        let found = Self::of(bytes);
        if found.digest == self.digest {
            tracing::debug!(
                algorithm = self.algorithm.name(),
                bytes = bytes.len(),
                "digest matched"
            );
            return Ok(());
        }
        // `warn` rather than `error`, and the distinction is not pedantry: this returns `Err`
        // and the caller decides what a mismatch means to it. The command that gives up says
        // `error`. Logging both here would report one problem twice, at the wrong severity,
        // from the layer that knows least about why it was asked. Conventions section 9.
        //
        // Never silent, though. This is the check standing between a download and something
        // run with kernel-adjacent privileges, and a failure here is never routine.
        tracing::warn!(
            algorithm = self.algorithm.name(),
            expected = %self.digest,
            found = %found.digest,
            "digest did not match"
        );
        Err(Mismatch {
            algorithm: self.algorithm,
            expected: self.digest.clone(),
            found: found.digest,
        })
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm.name(), self.digest)
    }
}

/// A checksum that cannot be used, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// The manifest entry has no checksum at all.
    ///
    /// Its own error rather than a parse failure, because the remedy is different: somebody
    /// has to find out what the digest should be, not correct a typo.
    Absent,
    /// The text is not a digest.
    NotADigest {
        /// What was there instead.
        text: String,
    },
    /// A digest this cannot check.
    Unsupported {
        /// What it appears to be, from its name or its length.
        named: String,
        /// How many digits it had.
        digits: usize,
    },
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Absent => write!(
                f,
                "no checksum, so nothing about this payload can be established"
            ),
            Self::NotADigest { text } => write!(f, "{text:?} is not a digest"),
            Self::Unsupported { named, digits } => write!(
                f,
                "{named} ({digits} digits) cannot be checked here - \
                 only {} is supported, and adding another means writing it against a real one",
                Algorithm::Sha256.name()
            ),
        }
    }
}

impl std::error::Error for Unreadable {}

/// A payload that is not the payload that was described.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// Which algorithm disagreed.
    pub algorithm: Algorithm,
    /// What the manifest said.
    pub expected: String,
    /// What the bytes actually are.
    pub found: String,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} mismatch: expected {}, got {} - do not send this",
            self.algorithm.name(),
            self.expected,
            self.found
        )
    }
}

impl std::error::Error for Mismatch {}

/// What a digest of this length is most likely to be, for the error message.
fn guess(digits: usize) -> String {
    match digits {
        32 => "md5".to_owned(),
        40 => "sha1".to_owned(),
        128 => "sha512".to_owned(),
        _ => "unrecognised".to_owned(),
    }
}

/// Lower case hexadecimal.
fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::{Algorithm, Checksum, Unreadable};

    /// Two published vectors, so the wiring is checked against something outside this crate.
    #[test]
    fn it_agrees_with_the_published_vectors() {
        assert_eq!(
            Checksum::of(b"").digest(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            Checksum::of(b"abc").digest(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// A manifest may write it either way, and case is not meaning.
    #[test]
    fn a_prefix_and_a_bare_digest_read_the_same() {
        let bare =
            Checksum::parse("BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD")
                .expect("a bare digest");
        let named = Checksum::parse(
            " sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad ",
        )
        .expect("a named digest");
        assert_eq!(bare, named);
        assert_eq!(bare.algorithm(), Algorithm::Sha256);
    }

    /// A digest this cannot check is an error naming it, **not** a check that is skipped.
    ///
    /// The failure this pins is the worst one available: a tool that reports a payload as
    /// verified when it never looked at it.
    #[test]
    fn a_digest_that_cannot_be_checked_is_refused_by_name() {
        let error = Checksum::parse("d41d8cd98f00b204e9800998ecf8427e").expect_err("md5 is 32");
        assert_eq!(
            error,
            Unreadable::Unsupported {
                named: "md5".to_owned(),
                digits: 32
            }
        );
        assert!(error.to_string().contains("md5"));

        assert!(matches!(
            Checksum::parse("md5:d41d8cd98f00b204e9800998ecf8427e"),
            Err(Unreadable::Unsupported { .. })
        ));
    }

    /// A missing checksum is its own answer, because the remedy differs from a typo.
    #[test]
    fn no_checksum_at_all_is_told_apart_from_a_bad_one() {
        assert_eq!(Checksum::parse("   "), Err(Unreadable::Absent));
        assert!(matches!(
            Checksum::parse("not a digest at all"),
            Err(Unreadable::NotADigest { .. })
        ));
    }

    /// A name that disagrees with the length is a manifest somebody has already got wrong,
    /// and picking a half to believe would be inventing an answer.
    #[test]
    fn a_name_that_contradicts_the_length_is_refused() {
        assert!(matches!(
            Checksum::parse("sha256:abcdef"),
            Err(Unreadable::Unsupported { .. })
        ));
    }

    /// A mismatch carries both digests, because *which* is wrong is the actual question.
    #[test]
    fn a_mismatch_says_what_was_expected_and_what_arrived() {
        let expected = Checksum::of(b"the payload that was described");
        let mismatch = expected
            .verify(b"something else entirely")
            .expect_err("different bytes");
        assert_eq!(mismatch.expected, expected.digest());
        assert_eq!(
            mismatch.found,
            Checksum::of(b"something else entirely").digest()
        );
        assert!(mismatch.to_string().contains("do not send this"));
    }

    /// The bytes that were described verify.
    #[test]
    fn the_right_bytes_pass() {
        let payload = b"\x7fELF and then some";
        assert!(Checksum::of(payload).verify(payload).is_ok());
    }
}
