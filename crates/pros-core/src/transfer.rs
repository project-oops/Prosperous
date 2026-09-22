//! Copying a whole folder off the target, and putting one back.
//!
//! # What a backup has to promise
//!
//! That it is complete, or that it says exactly where it is not.
//!
//! A save backup which quietly missed a file is worse than no backup, because it will be
//! trusted at the moment it matters. So every entry this walk does not copy is **collected
//! and returned**, and the summary a caller shows says how many. Nothing is skipped
//! silently, and nothing is skipped for a reason the caller cannot read.
//!
//! # Symbolic links are not followed
//!
//! A link on a target filesystem can point at its own parent, and a walk that follows one
//! runs until it fills a disk. Following them safely means tracking identity across a
//! protocol that does not offer it, so they are **reported as skipped** instead - which is
//! a fact about the backup, and appears in the same list as everything else that was not
//! copied.
//!
//! # Why the walk is written against a trait
//!
//! So that the recursion, the link rule and the skipped list can be tested without a
//! target. The protocol underneath is one implementation of two methods.

use std::path::{Path, PathBuf};

use pros_link::files::{Entry, Kind, Session};

use crate::checksum::Checksum;
use crate::deployed::Ledger;

/// How deep a walk may go before it stops.
///
/// A bound rather than a belief. Save folders are shallow, and something that is not one
/// should stop rather than run.
const DEEPEST: usize = 12;

/// Somewhere directories can be listed and files fetched.
///
/// Two methods, so a test can be a map in memory and the real one can be a logged-in file
/// session.
pub trait Source {
    /// Lists a directory.
    ///
    /// # Errors
    ///
    /// Whatever the underlying transport reports, as text.
    fn list(&mut self, path: &str) -> Result<Vec<Entry>, String>;

    /// Fetches a file whole.
    ///
    /// # Errors
    ///
    /// As [`Source::list`].
    fn retrieve(&mut self, path: &str) -> Result<Vec<u8>, String>;
}

impl Source for Session {
    fn list(&mut self, path: &str) -> Result<Vec<Entry>, String> {
        Self::list(self, path).map_err(|why| why.to_string())
    }

    fn retrieve(&mut self, path: &str) -> Result<Vec<u8>, String> {
        Self::retrieve(self, path).map_err(|why| why.to_string())
    }
}

/// How far a copy has got.
///
/// **Reported as it happens, not at the end.** A folder of any size takes long enough that a
/// window showing nothing is indistinguishable from a window that has stopped, and the
/// person watching cannot tell whether to wait or to kill it. Naming the file currently
/// going across answers both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// How many files have been copied so far.
    pub files: usize,
    /// How many bytes those were.
    pub bytes: u64,
    /// What is going across now.
    pub current: String,
}

/// Something the walk did not copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Where it was.
    pub path: String,
    /// Why it was left.
    pub why: String,
}

/// What a copy did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    /// How many files were copied.
    pub files: usize,
    /// How many bytes those were.
    pub bytes: u64,
    /// How many files were already on the target, unchanged, and so were not sent again.
    ///
    /// **Not a skip and not a copy.** A skip is a file the copy failed to move and the summary is
    /// incomplete without it; an unchanged file is one that did not need moving. Kept apart so a
    /// restore that sent nothing because nothing changed reads as the success it is, not as a
    /// backup that copied nothing. See [`upload`] and [`crate::deployed`].
    pub unchanged: usize,
    /// Everything that was not copied, and why.
    ///
    /// **The field that makes the rest of it mean anything.** A backup is only as good as
    /// its account of what it left behind.
    pub skipped: Vec<Skipped>,
}

impl Summary {
    /// Whether everything the walk saw was copied.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }
}

/// Copies a directory and everything under it to a local folder.
///
/// # Why a caller can stop it
///
/// **A copy that cannot be stopped is one somebody has to kill the process to escape.** This
/// walks a tree whose size is not known until it has been walked, started by one click, over
/// a network - all three mean it can turn out to be far larger than whoever asked expected.
/// `stop` is checked before each entry and before each directory, so asking it to stop takes
/// effect within one file rather than at the end.
///
/// A stopped copy is **recorded as stopped in the summary**. A partial backup that presented
/// itself as complete would be the worst possible outcome of this, and it is exactly what
/// returning early without saying so would produce.
///
/// # Errors
///
/// Only when the top of the walk cannot be listed at all, or a local write fails. Anything
/// further down that cannot be copied is **recorded in the summary** rather than abandoning
/// the rest: a backup that stops at the first unreadable file has saved nothing.
pub fn download(
    source: &mut dyn Source,
    from: &str,
    into: &Path,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> Result<Summary, String> {
    let mut summary = Summary::default();
    walk(source, from, into, 0, &mut summary, watch, stop)?;
    Ok(summary)
}

/// Whether a listing entry names a way through the tree rather than a thing in it.
///
/// Empty, `.`, `..`, or anything carrying a separator. All four make `Path::join` produce a
/// path outside the directory it was joined to, which is the whole of the danger.
fn is_a_step_rather_than_a_name(name: &str) -> bool {
    name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\')
}

/// One directory of the walk.
fn walk(
    source: &mut dyn Source,
    from: &str,
    into: &Path,
    depth: usize,
    summary: &mut Summary,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> Result<(), String> {
    if stop() {
        summary.skipped.push(Skipped {
            path: from.to_owned(),
            why: "stopped before this was copied".to_owned(),
        });
        return Ok(());
    }
    if depth > DEEPEST {
        summary.skipped.push(Skipped {
            path: from.to_owned(),
            why: format!("deeper than {DEEPEST} directories, so the walk stopped here"),
        });
        return Ok(());
    }

    let entries = source.list(from)?;
    std::fs::create_dir_all(into).map_err(|why| why.to_string())?;

    for entry in entries {
        if stop() {
            summary.skipped.push(Skipped {
                path: format!("{}/{}", from.trim_end_matches('/'), entry.name),
                why: "stopped before this was copied".to_owned(),
            });
            continue;
        }
        // **A name from the target never steers a local path.** The transport already drops
        // `.` and `..`, and this does not trust it to: `into.join(name)` with a name holding
        // a separator or a parent step writes outside the folder somebody asked to fill, and
        // recursing on one walks back up the target's filesystem.
        //
        // Belt and braces on purpose. The first version of this had the check in neither
        // place, and the result was a backup of a 64KB directory quietly copying the system.
        if is_a_step_rather_than_a_name(&entry.name) {
            summary.skipped.push(Skipped {
                path: format!("{}/{}", from.trim_end_matches('/'), entry.name),
                why: "a listing entry that is a path step rather than a name".to_owned(),
            });
            continue;
        }
        let there = format!("{}/{}", from.trim_end_matches('/'), entry.name);
        if !entry.is_usable() {
            // The transport kept the line it could not read, and here is where that matters:
            // something is in this directory and the backup does not have it.
            summary.skipped.push(Skipped {
                path: from.to_owned(),
                why: format!("a listing line that could not be read: {}", entry.raw),
            });
            continue;
        }
        match entry.kind {
            Kind::Directory => {
                let below = into.join(&entry.name);
                walk(source, &there, &below, depth + 1, summary, watch, stop)?;
            }
            Kind::Link => summary.skipped.push(Skipped {
                path: there,
                why: "a link, which is not followed - it may point at its own parent".to_owned(),
            }),
            Kind::File => match source.retrieve(&there) {
                Ok(bytes) => {
                    let here = into.join(&entry.name);
                    std::fs::write(&here, &bytes).map_err(|why| why.to_string())?;
                    summary.files += 1;
                    summary.bytes += bytes.len() as u64;
                    watch(&Progress {
                        files: summary.files,
                        bytes: summary.bytes,
                        current: there.clone(),
                    });
                }
                // One unreadable file does not end the backup, and it does not disappear.
                Err(why) => summary.skipped.push(Skipped { path: there, why }),
            },
            Kind::Unrecognised => {}
        }
    }
    Ok(())
}

/// Whether a restore may skip files it has already put on this target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resend {
    /// Send every file, whatever was sent before. What `restore --all` asks for, and the safe
    /// choice when the record cannot be trusted - a target reimaged behind the same name, say.
    Everything,
    /// Skip a file whose bytes are already recorded landed at its path and which is still present.
    /// The default, and on a large title the reason most of a restore becomes a set of cheap size
    /// checks rather than the whole tree sent again. See [`crate::deployed`].
    OnlyChanged,
}

/// Puts a local folder back onto the target.
///
/// Directories are made on the way down, and one that already exists is not a failure - see
/// [`Session::make_directory`].
///
/// **A file already verified landed here is not sent again** unless [`Resend::Everything`] is
/// asked for: its local bytes are hashed, and if that digest is what `known` records at its path
/// and the target still reports the file present, the store is skipped and the file counted as
/// [`Summary::unchanged`]. Every verified store updates `known`, so the next restore can skip it;
/// a store that does not verify forgets it, so a failed landing is never skipped. Why the record
/// is kept this side rather than asked of the target - the SELF unwrap - is in [`crate::deployed`]
/// and at `land`.
///
/// # Errors
///
/// When the local folder cannot be read. A file that will not go across is **recorded in the
/// summary**, for the same reason as a backup: stopping at the first refusal leaves the
/// restore half done and unrecorded, which is the worst of both.
pub fn upload(
    session: &mut Session,
    from: &Path,
    to: &str,
    known: &mut Ledger,
    resend: Resend,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> Result<Summary, String> {
    let mut summary = Summary::default();
    let root = to.trim_end_matches('/');
    let _ = session.make_directory(root);

    for relative in contents(from)? {
        // The same reason as a backup: a restore is a walk of unknown size started by one
        // click, and it stops within one file rather than at the end.
        if stop() {
            summary.skipped.push(Skipped {
                path: relative.to_string_lossy().into_owned(),
                why: "stopped before this was copied".to_owned(),
            });
            continue;
        }
        let there = format!("{root}/{}", relative.to_string_lossy().replace('\\', "/"));
        make_parents(session, root, &relative, &mut summary);

        let source = from.join(&relative);
        let bytes = match std::fs::read(&source) {
            Ok(bytes) => bytes,
            Err(why) => {
                summary.skipped.push(Skipped {
                    path: source.display().to_string(),
                    why: why.to_string(),
                });
                continue;
            }
        };

        // **An unchanged file already on the target is not sent again.** The digest is of the
        // local bytes - the side that can be hashed truthfully, because the target unwraps a SELF
        // on access (`crate::deployed`, and `land` below). The presence check is not optional: a
        // record is not a promise the file is still there, and skipping one a crash had removed
        // would be the quiet miss this module exists to refuse. `--all` is `Resend::Everything`
        // and passes both by.
        let digest = Checksum::of(&bytes).to_string();
        if resend == Resend::OnlyChanged
            && known.records(&there, &digest)
            && session.size(&there).is_ok()
        {
            summary.unchanged += 1;
            continue;
        }

        land(session, there, &bytes, &digest, known, &mut summary, watch);
    }
    Ok(summary)
}

/// Makes every directory on the way to a file, in order.
///
/// A server will not make a parent for you, and the second file in a folder should not pay for the
/// first one's work. A directory that will not be made is recorded and the walk goes on - the
/// store into it will fail and be recorded too, so nothing is lost by not stopping here.
fn make_parents(session: &mut Session, root: &str, relative: &Path, summary: &mut Summary) {
    let Some(parent) = relative.parent() else {
        return;
    };
    let mut here = root.to_owned();
    for part in parent.components() {
        here.push('/');
        here.push_str(&part.as_os_str().to_string_lossy());
        if let Err(why) = session.make_directory(&here) {
            summary.skipped.push(Skipped {
                path: here.clone(),
                why: why.to_string(),
            });
        }
    }
}

/// Stores one file, then records the outcome: counted and remembered if it verifies, skipped and
/// forgotten if it does not.
///
/// **A store the server accepted is not yet a file replaced.** The size is read back and checked:
/// a target that has the title mounted, or an overlay that swallows the write, leaves the old file
/// in place while `STOR` still completes, and a restore that trusted the reply then reported a
/// file written that was not.
///
/// **A SELF container does not keep its sent size, and must not be compared to it.** On a
/// jailbroken console the kernel VFS hook unwraps a fake-signed SELF on access, so `SIZE` reports
/// the decrypted ELF payload - a legitimately different, usually larger number - and comparing it
/// to the bytes sent condemns a deploy that worked (oops-mesa REQ-20260917T1500Z-3e57). That
/// unwrapped size cannot be recovered from the container here: the kernel presents the whole
/// decrypted file, not a sum this side can compute from the segment table, and reimplementing the
/// SELF+ELF layout to guess it is the format-reinvention principle 6 exists to refuse. So for a
/// container the check is *presence* - a size came back, so a file is there - which still catches
/// a store that landed nothing. A plain file is size-checked exactly, and a mismatch is not-copied.
///
/// **What lands is remembered, what does not is forgotten.** A verified store records `digest`
/// against the path in `known`, so the next restore can skip it; every failure forgets any record
/// there, so a file that did not land is sent again next time rather than skipped on a stale one.
fn land(
    session: &mut Session,
    there: String,
    bytes: &[u8],
    digest: &str,
    known: &mut Ledger,
    summary: &mut Summary,
    watch: &mut dyn FnMut(&Progress),
) {
    let sent = bytes.len() as u64;
    if let Err(why) = session.store(&there, bytes) {
        known.forget(&there);
        summary.skipped.push(Skipped {
            path: there,
            why: why.to_string(),
        });
        return;
    }
    // The four bytes at offset zero, asked of SELFish: a SELF container for either generation
    // (which the target unwraps), or not (an ELF or anything else, which it stores as-is).
    let is_container = bytes
        .get(..4)
        .and_then(|head| <[u8; 4]>::try_from(head).ok())
        .and_then(selfish_abi::Generation::from_container_magic)
        .is_some();
    match session.size(&there) {
        Ok(there_bytes) if is_container || there_bytes == sent => {
            known.record(&there, digest);
            summary.files += 1;
            summary.bytes += sent;
            watch(&Progress {
                files: summary.files,
                bytes: summary.bytes,
                current: there,
            });
        }
        Ok(there_bytes) => {
            known.forget(&there);
            summary.skipped.push(Skipped {
                why: format!(
                    "sent {sent} bytes but the target reports {there_bytes} afterwards - it was \
                     not replaced (is the title mounted?)"
                ),
                path: there,
            });
        }
        Err(why) => {
            known.forget(&there);
            summary.skipped.push(Skipped {
                why: format!(
                    "sent {sent} bytes but the target could not confirm the size afterwards, so \
                     it is not known to have landed: {why}"
                ),
                path: there,
            });
        }
    }
}

/// Everything under a local folder, as paths relative to it.
///
/// Separated from the sending so it can be tested, and so a caller can show what is about to
/// go before any of it does.
///
/// # Errors
///
/// When the folder cannot be read.
pub fn contents(of: &Path) -> Result<Vec<PathBuf>, String> {
    let mut found = Vec::new();
    gather(of, of, 0, &mut found)?;
    found.sort();
    Ok(found)
}

/// Walks a local folder.
fn gather(root: &Path, here: &Path, depth: usize, found: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > DEEPEST {
        return Ok(());
    }
    for entry in std::fs::read_dir(here).map_err(|why| why.to_string())? {
        let entry = entry.map_err(|why| why.to_string())?;
        let path = entry.path();
        // `is_dir` follows links and `file_type` does not, which is the difference between
        // walking a loop and noticing one.
        let kind = entry.file_type().map_err(|why| why.to_string())?;
        if kind.is_dir() {
            gather(root, &path, depth + 1, found)?;
        } else if kind.is_file() {
            let relative = path.strip_prefix(root).map_err(|why| why.to_string())?;
            found.push(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pros_link::files::{Entry, Kind};

    use super::{DEEPEST, Resend, Source, Summary, download, upload};
    use crate::checksum::Checksum;
    use crate::deployed::Ledger;

    /// A filesystem in memory, so the walk can be checked without a target.
    struct Pretend {
        directories: BTreeMap<String, Vec<Entry>>,
        files: BTreeMap<String, Vec<u8>>,
    }

    impl Source for Pretend {
        fn list(&mut self, path: &str) -> Result<Vec<Entry>, String> {
            self.directories
                .get(path)
                .cloned()
                .ok_or_else(|| format!("no such directory {path}"))
        }

        fn retrieve(&mut self, path: &str) -> Result<Vec<u8>, String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("no such file {path}"))
        }
    }

    fn entry(name: &str, kind: Kind) -> Entry {
        Entry {
            name: name.to_owned(),
            kind,
            size: None,
            raw: name.to_owned(),
        }
    }

    fn a_save() -> Pretend {
        let mut directories = BTreeMap::new();
        directories.insert(
            "/user/home/PPSA02664".to_owned(),
            vec![
                entry("savedata.bin", Kind::File),
                entry("slot2", Kind::Directory),
                entry("elsewhere", Kind::Link),
                Entry {
                    name: "total 12".to_owned(),
                    kind: Kind::Unrecognised,
                    size: None,
                    raw: "total 12".to_owned(),
                },
            ],
        );
        directories.insert(
            "/user/home/PPSA02664/slot2".to_owned(),
            vec![
                entry("savedata.bin", Kind::File),
                entry("gone.bin", Kind::File),
            ],
        );

        let mut files = BTreeMap::new();
        files.insert(
            "/user/home/PPSA02664/savedata.bin".to_owned(),
            b"first".to_vec(),
        );
        files.insert(
            "/user/home/PPSA02664/slot2/savedata.bin".to_owned(),
            b"second".to_vec(),
        );
        // `gone.bin` is listed and cannot be fetched, which is the case a backup must not
        // paper over.
        Pretend { directories, files }
    }

    fn scratch(what: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("pros-copy-{}-{what}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// The walk goes down, and what it copied is on this machine afterwards.
    #[test]
    fn a_folder_and_everything_under_it_comes_across() {
        let into = scratch("tree");
        let summary = download(
            &mut a_save(),
            "/user/home/PPSA02664",
            &into,
            &mut |_| {},
            &|| false,
        )
        .expect("it walks");

        assert_eq!(summary.files, 2);
        assert_eq!(summary.bytes, 11);
        assert_eq!(
            std::fs::read(into.join("savedata.bin")).expect("the top file"),
            b"first"
        );
        assert_eq!(
            std::fs::read(into.join("slot2").join("savedata.bin")).expect("the nested file"),
            b"second"
        );
    }

    /// **A backup that quietly missed a file is worse than no backup.**
    ///
    /// One file cannot be fetched, one line could not be read, and one entry is a link. All
    /// three are in the summary, and the backup is not called complete.
    #[test]
    fn everything_not_copied_is_named() {
        let into = scratch("skipped");
        let summary = download(
            &mut a_save(),
            "/user/home/PPSA02664",
            &into,
            &mut |_| {},
            &|| false,
        )
        .expect("it walks");

        assert!(!summary.is_complete());
        assert_eq!(summary.skipped.len(), 3, "{:?}", summary.skipped);

        let reasons: Vec<&str> = summary
            .skipped
            .iter()
            .map(|skipped| skipped.why.as_str())
            .collect();
        assert!(reasons.iter().any(|why| why.contains("link")));
        assert!(reasons.iter().any(|why| why.contains("could not be read")));
        assert!(reasons.iter().any(|why| why.contains("no such file")));
    }

    /// One unreadable file does not end the backup.
    ///
    /// A walk that stops at the first failure has saved nothing, and the thing it failed on
    /// is usually the least important file in the folder.
    #[test]
    fn one_unreadable_file_does_not_abandon_the_rest() {
        let into = scratch("continues");
        let summary = download(
            &mut a_save(),
            "/user/home/PPSA02664",
            &into,
            &mut |_| {},
            &|| false,
        )
        .expect("it walks");
        assert_eq!(summary.files, 2, "it stopped early");
    }

    /// A link that points at its own parent would otherwise run until the disk filled.
    #[test]
    fn a_loop_cannot_run_away() {
        let mut directories = BTreeMap::new();
        // Every level contains another level with the same shape, for ever.
        for depth in 0..=(DEEPEST + 4) {
            let here = format!("/loop{}", "/down".repeat(depth));
            directories.insert(here, vec![entry("down", Kind::Directory)]);
        }
        let mut pretend = Pretend {
            directories,
            files: BTreeMap::new(),
        };

        let into = scratch("loop");
        let summary: Summary =
            download(&mut pretend, "/loop", &into, &mut |_| {}, &|| false).expect("it stops");
        assert!(
            summary
                .skipped
                .iter()
                .any(|skipped| skipped.why.contains("deeper than")),
            "the walk did not stop and did not say why"
        );
    }

    /// **Progress arrives as it happens**, so a window can show which file is going across
    /// rather than a clock that says only that time is passing.
    #[test]
    fn progress_is_reported_file_by_file() {
        let into = scratch("progress");
        let mut seen = Vec::new();
        download(
            &mut a_save(),
            "/user/home/PPSA02664",
            &into,
            &mut |progress| seen.push(progress.clone()),
            &|| false,
        )
        .expect("it walks");

        assert_eq!(seen.len(), 2, "one report per file copied");
        assert!(
            seen.first().is_some_and(|first| first.files == 1),
            "the first report should come after the first file"
        );
        assert!(
            seen.last().is_some_and(|last| last.bytes == 11),
            "the last report should carry the running total"
        );
        assert!(
            seen.iter().all(|report| !report.current.is_empty()),
            "every report should name what was going across"
        );
    }

    /// A directory that cannot be listed at all is the one failure worth refusing on: there
    /// is no backup to be partially complete.
    #[test]
    fn a_top_that_cannot_be_listed_is_an_error() {
        let into = scratch("nothing");
        assert!(download(&mut a_save(), "/nowhere", &into, &mut |_| {}, &|| false).is_err());
    }

    /// **A listing full of path steps copies nothing and escapes nowhere.**
    ///
    /// This is the bug that made the rule: asking to back up one small directory walked into
    /// `.` until the depth bound stopped it and climbed out through `..` into the rest of the
    /// target. It never errored. It copied, steadily, with a progress line indistinguishable
    /// from a large folder taking a while.
    ///
    /// The transport now drops `.` and `..` before anything sees them; this checks the second
    /// line, where a name that steers a path is refused even if one arrives.
    #[test]
    fn a_listing_that_points_at_itself_or_upwards_is_not_followed() {
        let mut directories = BTreeMap::new();
        directories.insert(
            "/data/pkg".to_owned(),
            vec![
                entry(".", Kind::Directory),
                entry("..", Kind::Directory),
                entry("../../etc", Kind::Directory),
                entry("real.bin", Kind::File),
            ],
        );
        let mut files = BTreeMap::new();
        files.insert("/data/pkg/real.bin".to_owned(), b"kept".to_vec());
        let mut source = Pretend { directories, files };

        let into = std::env::temp_dir().join("prosperous-walk-steps");
        let _ = std::fs::remove_dir_all(&into);
        let summary = download(&mut source, "/data/pkg", &into, &mut |_| {}, &|| false)
            .expect("the walk finishes");

        assert_eq!(summary.files, 1, "only the one real file should be copied");
        assert_eq!(
            summary.skipped.len(),
            3,
            "each path step should be recorded rather than silently dropped"
        );
        assert!(
            summary
                .skipped
                .iter()
                .all(|one| one.why.contains("path step")),
            "a skip should say why: {:?}",
            summary.skipped
        );
        let _ = std::fs::remove_dir_all(&into);
    }

    /// **Asking it to stop stops it, and the summary says so.**
    ///
    /// The failure this guards against is not that a stop is ignored - that is visible. It is
    /// a stop that works and returns a summary indistinguishable from a completed backup,
    /// which would be trusted later at exactly the moment it matters.
    #[test]
    fn a_copy_that_was_stopped_says_it_was_stopped() {
        let into = scratch("stopped");
        let summary = download(
            &mut a_save(),
            "/user/home/PPSA02664",
            &into,
            &mut |_| {},
            // Stopped from the very first check, which is the strongest form: nothing at all
            // should be copied, and nothing should be quietly reported as complete.
            &|| true,
        )
        .expect("it returns rather than failing");

        assert_eq!(summary.files, 0, "a stopped copy still copied something");
        assert!(
            !summary.is_complete(),
            "a stopped copy reported itself as a complete backup"
        );
        assert!(
            summary
                .skipped
                .iter()
                .any(|one| one.why.contains("stopped")),
            "the summary should say it was stopped: {:?}",
            summary.skipped
        );
    }

    /// **A store the server accepts but does not keep is not a file copied.**
    ///
    /// The reported bug: `pros restore` printed success while the on-console `eboot.bin` kept its
    /// old size. The target acknowledged every `STOR` and replaced nothing - the title was
    /// mounted - and a restore that trusts the reply reports a backup that is not one. Now the
    /// size is read back after each store and a mismatch is recorded as not-copied, so the
    /// summary is incomplete and the caller fails rather than claiming success.
    #[test]
    fn a_store_the_target_did_not_keep_is_not_counted_as_copied() {
        use pros_link::fake::{Behaviour, Fake, Store};
        use pros_link::files::Session;

        // The target already holds an eboot of a different size and swallows every write, so a
        // STOR is acknowledged and the old bytes stay - exactly the mounted-title case.
        let contents = Store::new(&[(
            "/data/homebrew/MESA00001/eboot.bin",
            b"the old, larger eboot that will not be replaced",
        )]);
        let fake = Fake::start(Behaviour::Files {
            contents,
            claims: [127, 0, 0, 1],
            binary: true,
            swallows_stores: true,
        })
        .expect("the fake binds");

        let from = scratch("not-kept");
        std::fs::create_dir_all(&from).expect("a source folder");
        std::fs::write(from.join("eboot.bin"), b"the new eboot").expect("a source file");

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut Ledger::default(),
            Resend::Everything,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs to the end");
        session.close();

        assert_eq!(
            summary.files, 0,
            "nothing actually landed, so nothing is copied"
        );
        assert!(
            !summary.is_complete(),
            "a restore whose files were not kept must not report itself complete"
        );
        assert!(
            summary
                .skipped
                .iter()
                .any(|one| one.path.ends_with("eboot.bin") && one.why.contains("not replaced")),
            "the summary should name the file that was not replaced: {:?}",
            summary.skipped
        );
        let _ = std::fs::remove_dir_all(&from);
    }

    /// **A SELF container the target unwraps is not called incomplete.** The console's VFS hook
    /// unwraps a fake-signed SELF on access, so `SIZE` reports the decrypted payload - a size that
    /// legitimately differs from the container that was sent. A restore of one must still report
    /// complete, because the file is there and its prefix says the target will have changed its
    /// size (oops-mesa REQ-20260917T1500Z-3e57). Modelled with a target that reports a different
    /// size for the path than was sent, and a sent file carrying the SELF magic SELFish knows.
    #[test]
    fn a_self_container_the_target_unwraps_is_not_called_incomplete() {
        use pros_link::fake::{Behaviour, Fake, Store};
        use pros_link::files::Session;

        // The target already holds a different-sized file at the path and swallows the write, so
        // `SIZE` reports that different size afterwards - which is what an unwrap looks like from
        // here: the bytes on the target are not the bytes that were sent.
        let contents = Store::new(&[("/data/homebrew/MESA00001/libc.prx", &[0_u8; 200])]);
        let fake = Fake::start(Behaviour::Files {
            contents,
            claims: [127, 0, 0, 1],
            binary: true,
            swallows_stores: true,
        })
        .expect("the fake binds");

        // A file that begins with the SELF container magic SELFish defines, so the transfer knows
        // the target will unwrap it and must not compare its size.
        let mut wrapped = selfish_abi::Generation::Prospero.container_magic().to_vec();
        wrapped.extend_from_slice(b"a fake-signed SELF, smaller than its unwrapped payload");
        let from = scratch("self-unwrapped");
        std::fs::create_dir_all(&from).expect("a source folder");
        std::fs::write(from.join("libc.prx"), &wrapped).expect("a source file");

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut Ledger::default(),
            Resend::Everything,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs to the end");
        session.close();

        assert_eq!(summary.files, 1, "the container is present and counted");
        assert!(
            summary.is_complete(),
            "a SELF the target unwraps must not be called incomplete: {:?}",
            summary.skipped
        );
        let _ = std::fs::remove_dir_all(&from);
    }

    /// **A SELF container that did not land at all is still caught.** Skipping the size *value*
    /// for a container is not skipping the check: presence is still required. A store the target
    /// acknowledged and kept nothing - no file at the path afterwards - is not-copied, so the SELF
    /// exemption cannot be used to wave through a transfer that vanished.
    #[test]
    fn a_self_container_that_did_not_land_at_all_is_still_caught() {
        use pros_link::fake::{Behaviour, Fake, Store};
        use pros_link::files::Session;

        // Nothing at the path, and the write is swallowed, so `SIZE` finds no file afterwards.
        let fake = Fake::start(Behaviour::Files {
            contents: Store::new(&[]),
            claims: [127, 0, 0, 1],
            binary: true,
            swallows_stores: true,
        })
        .expect("the fake binds");

        let mut wrapped = selfish_abi::Generation::Prospero.container_magic().to_vec();
        wrapped.extend_from_slice(b"a container that will not land");
        let from = scratch("self-vanished");
        std::fs::create_dir_all(&from).expect("a source folder");
        std::fs::write(from.join("eboot.bin"), &wrapped).expect("a source file");

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut Ledger::default(),
            Resend::Everything,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs to the end");
        session.close();

        assert_eq!(summary.files, 0, "nothing landed, so nothing is copied");
        assert!(
            !summary.is_complete(),
            "a container that vanished must still be caught, not waved through as a SELF"
        );
        let _ = std::fs::remove_dir_all(&from);
    }

    /// A file service holding these files, that keeps what it is sent.
    fn a_target(files: &[(&str, &[u8])]) -> (pros_link::fake::Fake, pros_link::fake::Store) {
        use pros_link::fake::{Behaviour, Fake, Store};
        let contents = Store::new(files);
        let fake = Fake::start(Behaviour::Files {
            contents: contents.clone(),
            claims: [127, 0, 0, 1],
            binary: true,
            swallows_stores: false,
        })
        .expect("the fake binds");
        (fake, contents)
    }

    /// A one-file source folder, returning where it is.
    fn a_source(what: &str, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let from = scratch(what);
        std::fs::create_dir_all(&from).expect("a source folder");
        std::fs::write(from.join(name), bytes).expect("a source file");
        from
    }

    /// **An unchanged file already on the target is not sent again.**
    ///
    /// The reported cost: restoring a large title re-sent every file even where nothing had
    /// changed. With a record of what verified landing and the file still present, the store is
    /// skipped - proven here by leaving different bytes on the target and showing they are
    /// untouched, so the skip is the ledger's decision and not a store that happened to match.
    #[test]
    fn an_unchanged_file_is_not_resent() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/thing.bin";
        // Different bytes on the target on purpose: a skip must not overwrite them.
        let (fake, contents) = a_target(&[(path, b"what is already on the target")]);
        let local = b"the local bytes, already verified landed last time";
        let from = a_source("unchanged", "thing.bin", local);

        // The ledger already records exactly these local bytes landed at the path.
        let mut known = Ledger::default();
        known.record(path, &Checksum::of(local).to_string());

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut known,
            Resend::OnlyChanged,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs");
        session.close();

        assert_eq!(summary.files, 0, "nothing was sent");
        assert_eq!(
            summary.unchanged, 1,
            "the file was counted as already there"
        );
        assert!(summary.is_complete(), "an unchanged file is not a skip");
        assert_eq!(
            contents.get(path).as_deref(),
            Some(&b"what is already on the target"[..]),
            "the target's file was not touched, so no store happened"
        );
        let _ = std::fs::remove_dir_all(&from);
    }

    /// **A changed local file is sent even where the ledger knows the path.** The digest is of the
    /// bytes, so a record from a previous build does not match a rebuilt file, and it goes across.
    #[test]
    fn a_changed_local_file_is_resent() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/eboot.bin";
        let (fake, contents) = a_target(&[(path, b"the previous build")]);
        let now = b"the rebuilt eboot, different bytes";
        let from = a_source("changed", "eboot.bin", now);

        // The ledger records an older build's digest at the path.
        let mut known = Ledger::default();
        known.record(path, &Checksum::of(b"the previous build").to_string());

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut known,
            Resend::OnlyChanged,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs");
        session.close();

        assert_eq!(summary.files, 1, "the changed file was sent");
        assert_eq!(summary.unchanged, 0);
        assert_eq!(
            contents.get(path).as_deref(),
            Some(&now[..]),
            "the target holds the new bytes"
        );
        assert!(
            known.records(path, &Checksum::of(now).to_string()),
            "the ledger now records what actually landed"
        );
        let _ = std::fs::remove_dir_all(&from);
    }

    /// **A file the ledger knows but the target no longer has is sent again.**
    ///
    /// The presence half of the check. A record is not a promise the file is still there - a crash
    /// or a wipe can remove it while the local source is unchanged - so a matching digest alone
    /// does not skip; the target must still report it present.
    #[test]
    fn a_recorded_file_the_target_no_longer_has_is_resent() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/eboot.bin";
        // The target holds nothing, so SIZE finds no file.
        let (fake, contents) = a_target(&[]);
        let local = b"back again after a wipe";
        let from = a_source("lost", "eboot.bin", local);

        // The ledger records exactly the current local bytes - the digest matches - but the file
        // is gone from the target.
        let mut known = Ledger::default();
        known.record(path, &Checksum::of(local).to_string());

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut known,
            Resend::OnlyChanged,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs");
        session.close();

        assert_eq!(
            summary.files, 1,
            "a matching digest does not skip a missing file"
        );
        assert_eq!(summary.unchanged, 0);
        assert_eq!(
            contents.get(path).as_deref(),
            Some(&local[..]),
            "the file was put back"
        );
        let _ = std::fs::remove_dir_all(&from);
    }

    /// **`--all` ignores the ledger and sends everything.** The escape hatch for when the record
    /// cannot be trusted: even a file recorded landed and still present goes across again.
    #[test]
    fn everything_mode_sends_even_an_unchanged_file() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/thing.bin";
        let (fake, contents) = a_target(&[(path, b"stale on target")]);
        let local = b"send me regardless";
        let from = a_source("forced", "thing.bin", local);

        // The ledger records exactly these local bytes, and the file is present - OnlyChanged
        // would skip it. Everything must not.
        let mut known = Ledger::default();
        known.record(path, &Checksum::of(local).to_string());

        let mut session = Session::open_at(fake.address(), fake.port()).expect("the fake logs in");
        let summary = upload(
            &mut session,
            &from,
            "/data/homebrew/MESA00001",
            &mut known,
            Resend::Everything,
            &mut |_| {},
            &|| false,
        )
        .expect("the upload runs");
        session.close();

        assert_eq!(summary.files, 1, "--all sends even an unchanged file");
        assert_eq!(summary.unchanged, 0);
        assert_eq!(
            contents.get(path).as_deref(),
            Some(&local[..]),
            "the store actually ran"
        );
        let _ = std::fs::remove_dir_all(&from);
    }
}
