//! What was last verified to have landed on a target, so an unchanged file is not sent twice.
//!
//! # Why a restore needs to remember anything at all
//!
//! Restoring a title re-sent every file, every time. A store is the only way this has to put
//! bytes on a target, and it had no way to tell that a file already there was the one it was about
//! to send - so a large title was minutes of transfer to leave most of it exactly as it was.
//!
//! # Why the memory is here and not asked of the target
//!
//! The obvious answer - ask the target what it holds and skip what matches - does not work, and
//! the reason is the one [`crate::transfer`] already records at its size check. The kernel VFS
//! hook unwraps a fake-signed SELF on access, so what the file service reports for `eboot.bin`, a
//! `.prx` or a `.sprx` is the decrypted ELF, not the container that was sent. A digest the server
//! computed would be over bytes this side never holds, and would disagree for every SELF - which
//! is exactly the large file a title is mostly made of. Size fails the same way, which is D032.
//!
//! So the only record this can keep truthfully is the one it makes itself: **what this program
//! verified landing.** After a store passes the presence-and-size check, the digest of the bytes
//! that were sent is recorded against the path they went to. A later restore skips a file whose
//! local bytes still hash to that record **and** which the target still reports present - the
//! second half so a file a crash or a wipe removed is sent again even when the local source has
//! not changed.
//!
//! # It is a cache, and being wrong only ever costs a re-send
//!
//! Nothing is skipped that was not both recorded from a verified landing and confirmed present.
//! The one thing this cannot see is something *other than this program* rewriting a file on the
//! target to different bytes at the same path while the local source stays put - and in the deploy
//! loop this feeds, the only writer to `/data/homebrew/<id>` is a restore, so that is rare. It is
//! still a cache: `restore --all` ignores it and sends everything, a fresh target has none, and a
//! store that will not verify drops back out of it. When in doubt it re-sends, never the other
//! way, because a skipped change is the one failure a restore must not have.
//!
//! # On disk beside the registry
//!
//! Kept where the registry, the payload manifest and the source cache are, and keyed by target
//! name over remote path so two targets holding different bytes at one path do not share a record.
//! Like [`crate::sources`], a file that cannot be read is an empty one: losing the cache costs a
//! full re-send, never correctness.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What this program verified landing on one target, by content digest, keyed by remote path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    /// Remote path -> the digest of the bytes last verified to have landed there.
    ///
    /// The digest is the self-describing form [`crate::checksum::Checksum`] renders (`sha256:...`),
    /// so a record is never mistaken for one written by another algorithm if a second is ever
    /// added - the same reason the checksum module keeps the name beside the digits.
    #[serde(default)]
    landed: BTreeMap<String, String>,
}

impl Ledger {
    /// Whether bytes with this digest are already recorded landed at this path.
    #[must_use]
    pub fn records(&self, path: &str, digest: &str) -> bool {
        self.landed.get(path).is_some_and(|known| known == digest)
    }

    /// Records that bytes with this digest were verified landed at this path.
    pub fn record(&mut self, path: &str, digest: &str) {
        self.landed.insert(path.to_owned(), digest.to_owned());
    }

    /// Forgets any record for a path, so it is sent next time.
    ///
    /// Called when a store did not verify: a stale record must never let a file that failed to
    /// land be skipped on the next run - that would be the quiet miss inverted.
    pub fn forget(&mut self, path: &str) {
        self.landed.remove(path);
    }

    /// How many landings are remembered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.landed.len()
    }

    /// Whether nothing is remembered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.landed.is_empty()
    }
}

/// What has been verified landing on every target this machine has restored to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Deployed {
    /// Keyed by the target's registered name.
    #[serde(default)]
    targets: BTreeMap<String, Ledger>,
}

impl Deployed {
    /// The ledger for one target, created empty if this is the first restore to it.
    pub fn for_target(&mut self, name: &str) -> &mut Ledger {
        self.targets.entry(name.to_owned()).or_default()
    }

    /// The ledger for one target, or `None` if nothing has been restored to it.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Ledger> {
        self.targets.get(name)
    }
}

/// Where the record is kept.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("deployed.json");
    Some(path)
}

/// Reads what has landed before.
///
/// **A file that cannot be read is an empty record**, not a failure: the cost of losing it is a
/// full re-send, and refusing to restore because a cache would not parse would be absurd.
#[must_use]
pub fn load() -> Deployed {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes what has landed.
///
/// # Errors
///
/// When there is nowhere to write, or the write fails.
pub fn save(deployed: &Deployed) -> Result<PathBuf, String> {
    let path = path().ok_or_else(|| "no home directory, so there is nowhere for it".to_owned())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| why.to_string())?;
    }
    let text = serde_json::to_string_pretty(deployed).map_err(|why| why.to_string())?;
    std::fs::write(&path, text).map_err(|why| why.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Deployed, Ledger};

    /// A record is a path and a digest together: the same path with a different digest is not a
    /// match, which is what makes a changed file a re-send.
    #[test]
    fn a_record_matches_only_the_digest_it_was_written_with() {
        let mut ledger = Ledger::default();
        ledger.record("/data/homebrew/MESA00001/eboot.bin", "sha256:aaaa");
        assert!(ledger.records("/data/homebrew/MESA00001/eboot.bin", "sha256:aaaa"));
        assert!(
            !ledger.records("/data/homebrew/MESA00001/eboot.bin", "sha256:bbbb"),
            "a different digest at the same path is a changed file"
        );
        assert!(
            !ledger.records("/data/homebrew/MESA00001/other.bin", "sha256:aaaa"),
            "a record is for one path only"
        );
    }

    /// **Forgetting a record means the file is sent next time.** A store that did not verify calls
    /// this so a stale record cannot wave a failed landing through.
    #[test]
    fn a_forgotten_record_no_longer_matches() {
        let mut ledger = Ledger::default();
        ledger.record("/p", "sha256:aaaa");
        ledger.forget("/p");
        assert!(!ledger.records("/p", "sha256:aaaa"));
        assert!(ledger.is_empty());
    }

    /// Two targets do not share a record, so bytes on one are never taken as proof about another.
    #[test]
    fn each_target_keeps_its_own_ledger() {
        let mut deployed = Deployed::default();
        deployed
            .for_target("living-room")
            .record("/p", "sha256:aaaa");
        assert!(
            deployed.get("desk").is_none(),
            "a target never restored to has no ledger"
        );
        assert!(
            !deployed.for_target("desk").records("/p", "sha256:aaaa"),
            "a fresh target's ledger is empty, not a copy of another's"
        );
    }

    /// What is written reads back the same, so the record survives between runs.
    #[test]
    fn a_record_survives_being_written_and_read() {
        let mut deployed = Deployed::default();
        deployed
            .for_target("living-room")
            .record("/p", "sha256:aaaa");
        let text = serde_json::to_string_pretty(&deployed).expect("it serialises");
        let again: Deployed = serde_json::from_str(&text).expect("it reads back");
        assert!(
            again
                .get("living-room")
                .is_some_and(|ledger| ledger.records("/p", "sha256:aaaa"))
        );
    }
}
