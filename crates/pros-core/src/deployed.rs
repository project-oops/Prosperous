//! What was last verified to have landed on a target, so an unchanged file is not sent twice.
//!
//! The target cannot be asked: the kernel VFS hook unwraps a fake-signed SELF on access, so
//! the file service reports the decrypted ELF for `eboot.bin`, `.prx` and `.sprx`, and neither
//! its size nor a digest of it matches what was sent (see [`crate::transfer`], D032). So this
//! records what this program verified landing: the digest of the sent bytes, per target name
//! and remote path. A restore skips a file only when its local bytes still hash to the record
//! and the target still reports it present. It is a cache that errs towards re-sending:
//! `restore --all` ignores it, a store that fails verification drops out of it, and a file
//! that cannot be read is an empty record.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What this program verified landing on one target, by content digest, keyed by remote path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ledger {
    /// Remote path -> the digest of the bytes last verified to have landed there.
    ///
    /// The digest is the self-describing form [`crate::checksum::Checksum`] renders
    /// (`sha256:...`), so a record names its algorithm.
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
    /// Called when a store did not verify, so a stale record never lets a failed landing be
    /// skipped.
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

/// Where the record is kept, beside the registry.
#[must_use]
pub fn path() -> Option<PathBuf> {
    let mut path = crate::target::directory()?;
    path.push("deployed.json");
    Some(path)
}

/// Reads what has landed before.
///
/// A file that cannot be read is an empty record, not a failure: losing it costs a full
/// re-send.
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
pub fn save(deployed: &Deployed) -> Result<PathBuf> {
    let path =
        path().ok_or_else(|| Error::failed("no home directory, so there is nowhere for it"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text =
        serde_json::to_string_pretty(deployed).map_err(|why| Error::failed(why.to_string()))?;
    std::fs::write(&path, text)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{Deployed, Ledger};

    /// A record matches only its own path and digest, so a changed file is re-sent.
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

    /// A forgotten record no longer matches, so the file is sent next time.
    #[test]
    fn a_forgotten_record_no_longer_matches() {
        let mut ledger = Ledger::default();
        ledger.record("/p", "sha256:aaaa");
        ledger.forget("/p");
        assert!(!ledger.records("/p", "sha256:aaaa"));
        assert!(ledger.is_empty());
    }

    /// Two targets do not share a record.
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

    /// A record survives being written and read back.
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
