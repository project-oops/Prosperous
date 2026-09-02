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

/// Puts a local folder back onto the target.
///
/// Directories are made on the way down, and one that already exists is not a failure - see
/// [`Session::make_directory`].
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
        // Every directory on the way, in order, because a server will not make a parent for
        // you and the second file in a folder should not pay for the first one's work.
        if let Some(parent) = relative.parent() {
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

        let source = from.join(&relative);
        match std::fs::read(&source) {
            Ok(bytes) => match session.store(&there, &bytes) {
                Ok(()) => {
                    summary.files += 1;
                    summary.bytes += bytes.len() as u64;
                    watch(&Progress {
                        files: summary.files,
                        bytes: summary.bytes,
                        current: there.clone(),
                    });
                }
                Err(why) => summary.skipped.push(Skipped {
                    path: there,
                    why: why.to_string(),
                }),
            },
            Err(why) => summary.skipped.push(Skipped {
                path: source.display().to_string(),
                why: why.to_string(),
            }),
        }
    }
    Ok(summary)
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

    use super::{DEEPEST, Source, Summary, download};

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
}
