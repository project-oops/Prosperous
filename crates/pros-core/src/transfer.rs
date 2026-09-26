//! Copying a whole folder off the target, and putting one back.
//!
//! A copy is complete, or its [`Summary`] names every entry it did
//! not copy and why.
//!
//! Symbolic links are not followed: one can point at its own parent, and the file protocol
//! offers no identity to detect the loop, so a link is reported as skipped. The walk is
//! written against [`Source`] so it can be tested without a target.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use pros_link::files::{Entry, Kind, Session};

use crate::checksum::Checksum;
use crate::deployed::Ledger;

/// How deep a walk may go before it stops.
///
/// Save folders are shallow; anything deeper is stopped and reported.
const DEEPEST: usize = 12;

/// Somewhere directories can be listed and files fetched.
///
/// A test implements it as a map in memory; the real one is a logged-in file session.
pub trait Source {
    /// Lists a directory.
    ///
    /// # Errors
    ///
    /// Whatever the underlying transport reports.
    fn list(&mut self, path: &str) -> crate::Result<Vec<Entry>>;

    /// Fetches a file whole.
    ///
    /// # Errors
    ///
    /// As [`Source::list`].
    fn retrieve(&mut self, path: &str) -> crate::Result<Vec<u8>>;
}

impl Source for Session {
    fn list(&mut self, path: &str) -> crate::Result<Vec<Entry>> {
        Ok(Self::list(self, path)?)
    }

    fn retrieve(&mut self, path: &str) -> crate::Result<Vec<u8>> {
        Ok(Self::retrieve(self, path)?)
    }
}

/// How far a copy has got.
///
/// Reported after each file, naming it, so a long copy is visibly moving.
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
    /// Neither a skip nor a copy: a restore that sent nothing because nothing changed is
    /// complete. See [`upload`] and [`crate::deployed`].
    pub unchanged: usize,
    /// Everything that was not copied, and why.
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
/// `stop` is checked before each entry and each directory, so a stop takes effect within one
/// file; everything not copied is recorded as stopped, so the summary is not complete.
///
/// # Errors
///
/// Only when the top of the walk cannot be listed, or a local write fails. Anything further
/// down that cannot be copied is recorded in the summary and the walk goes on.
pub fn download(
    source: &mut dyn Source,
    from: &str,
    into: &Path,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> crate::Result<Summary> {
    let mut summary = Summary::default();
    walk(source, from, into, 0, &mut summary, watch, stop)?;
    Ok(summary)
}

/// Whether a listing entry names a way through the tree rather than a thing in it.
///
/// Empty, `.`, `..`, or anything carrying a separator: each makes `Path::join` produce a path
/// outside the directory it was joined to.
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
) -> crate::Result<()> {
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
    std::fs::create_dir_all(into)?;

    for entry in entries {
        if stop() {
            summary.skipped.push(Skipped {
                path: format!("{}/{}", from.trim_end_matches('/'), entry.name),
                why: "stopped before this was copied".to_owned(),
            });
            continue;
        }
        // A name from the target never steers a local path. The transport drops `.` and `..`
        // too; this check does not rely on it, because a separator or parent step here writes
        // outside `into` and recursing on one walks back up the target's filesystem.
        if is_a_step_rather_than_a_name(&entry.name) {
            summary.skipped.push(Skipped {
                path: format!("{}/{}", from.trim_end_matches('/'), entry.name),
                why: "a listing entry that is a path step rather than a name".to_owned(),
            });
            continue;
        }
        let there = format!("{}/{}", from.trim_end_matches('/'), entry.name);
        if !entry.is_usable() {
            // A line the transport could not parse is something the backup does not have.
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
                    std::fs::write(&here, &bytes)?;
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
            Kind::Unrecognised => {}
        }
    }
    Ok(())
}

/// Whether a restore may skip files it has already put on this target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resend {
    /// Send every file, whatever was sent before (`restore --all`), for when the record cannot
    /// be trusted, such as a target reimaged behind the same name.
    Everything,
    /// Skip a file whose bytes are recorded as landed at its path and which is still present.
    /// The default. See [`crate::deployed`].
    OnlyChanged,
}

/// A restore, finished.
#[derive(Debug)]
pub struct Restored {
    /// What moved, what was skipped as unchanged, and what would not go.
    pub summary: Summary,
    /// Why the record of what landed could not be written, when it could not. A note, not a
    /// failure: the record is a cache, and the cost is a full re-send next time.
    pub unrecorded: Option<String>,
}

/// Puts a local folder onto a target: the one restore that `pros restore`, `pros probe` and the
/// window all call.
///
/// Opens the file service, loads the record of what already landed on this target, runs
/// [`upload`] against it, and writes the record back.
///
/// # Errors
///
/// When the file service will not open, or as [`upload`].
pub fn restore(
    target: &crate::target::Target,
    from: &Path,
    to: &str,
    resend: Resend,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> crate::Result<Restored> {
    let mut session = Session::open(&target.link())?;
    let mut deployed = crate::deployed::load();
    let done = upload(
        &mut session,
        from,
        to,
        deployed.for_target(&target.name),
        resend,
        watch,
        stop,
    );
    session.close();
    // Saved whatever the outcome: a restore that failed part way still landed verified files.
    let unrecorded = crate::deployed::save(&deployed)
        .err()
        .map(|why| why.to_string());
    Ok(Restored {
        summary: done?,
        unrecorded,
    })
}

/// Puts a local folder back onto the target, over a session already open.
///
/// Directories are made on the way down, and one that already exists is not a failure - see
/// [`Session::make_directory`].
///
/// Unless [`Resend::Everything`] is asked for, a file whose local digest is what `known`
/// records at its path, and which the target still lists, is counted as
/// [`Summary::unchanged`] and not sent. A verified store updates `known`; a failed one forgets
/// the path. The record is kept on this side because the target unwraps a SELF on access; see
/// [`crate::deployed`].
///
/// # Errors
///
/// When the local folder cannot be read. A file that will not go across is recorded in the
/// summary and the restore goes on.
pub fn upload(
    session: &mut Session,
    from: &Path,
    to: &str,
    known: &mut Ledger,
    resend: Resend,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> crate::Result<Summary> {
    let mut summary = Summary::default();
    let root = to.trim_end_matches('/');
    let _ = session.make_directory(root);
    // Directory listings, cached so presence costs one listing per folder. See `present`.
    let mut listings: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    for relative in contents(from)? {
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

        // The digest is of the local bytes, because the target unwraps a SELF on access. A
        // record does not prove the file is still there, so presence is checked too.
        let digest = Checksum::of(&bytes).to_string();
        if resend == Resend::OnlyChanged
            && known.records(&there, &digest)
            && present(session, &there, &mut listings)
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
/// The server does not make parents. A directory that will not be made is recorded and the
/// walk goes on; the store into it fails and is recorded too.
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

/// Whether the target still holds a file, read from a directory listing.
///
/// A listing, not a `SIZE`: a measured ftpsrv answered a size for a deleted path, and a
/// listed name does not change when the target unwraps a SELF. Each folder is listed once into
/// `listings`; a folder that cannot be listed reads as empty, so everything in it is resent.
fn present(
    session: &mut Session,
    there: &str,
    listings: &mut BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let (dir, name) = there.rsplit_once('/').unwrap_or(("", there));
    if !listings.contains_key(dir) {
        let names = session
            .list(dir)
            .map(|entries| entries.into_iter().map(|entry| entry.name).collect())
            .unwrap_or_default();
        listings.insert(dir.to_owned(), names);
    }
    listings.get(dir).is_some_and(|names| names.contains(name))
}

/// Stores one file, then records the outcome: counted and remembered if it verifies, skipped and
/// forgotten if it does not.
///
/// The size is read back: a mounted title or an overlay that swallows the write leaves the old
/// file in place while `STOR` still completes. A plain file must match the sent size exactly.
///
/// A SELF container is checked for presence only. On a target running the homebrew services
/// the VFS unwraps a fake-signed SELF on access, so `SIZE` reports the unwrapped ELF, and that
/// size cannot be computed from the container here.
///
/// A verified store records `digest` against the path in `known`; every failure forgets it.
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
    // A SELF container for either generation, which the target unwraps; anything else is
    // stored as-is.
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
/// Separate from the sending so a caller can show what is about to go before any of it does.
///
/// # Errors
///
/// When the folder cannot be read.
pub fn contents(of: &Path) -> crate::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    gather(of, of, 0, &mut found)?;
    found.sort();
    Ok(found)
}

/// Walks a local folder.
fn gather(root: &Path, here: &Path, depth: usize, found: &mut Vec<PathBuf>) -> crate::Result<()> {
    if depth > DEEPEST {
        return Ok(());
    }
    for entry in std::fs::read_dir(here)? {
        let entry = entry?;
        let path = entry.path();
        // `file_type` does not follow links, so a link loop is not walked.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            gather(root, &path, depth + 1, found)?;
        } else if kind.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|why| crate::Error::failed(why.to_string()))?;
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
        fn list(&mut self, path: &str) -> crate::Result<Vec<Entry>> {
            self.directories
                .get(path)
                .cloned()
                .ok_or_else(|| crate::Error::failed(format!("no such directory {path}")))
        }

        fn retrieve(&mut self, path: &str) -> crate::Result<Vec<u8>> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| crate::Error::failed(format!("no such file {path}")))
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
        // `gone.bin` is listed and cannot be fetched.
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

    /// An unfetchable file, an unreadable line and a link are all named in the summary.
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

    /// A self-similar tree stops at the depth bound and says why.
    #[test]
    fn a_loop_cannot_run_away() {
        let mut directories = BTreeMap::new();
        // Every level contains another level with the same shape.
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

    /// Progress is reported once per file, naming it.
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

    /// A top directory that cannot be listed is an error.
    #[test]
    fn a_top_that_cannot_be_listed_is_an_error() {
        let into = scratch("nothing");
        assert!(download(&mut a_save(), "/nowhere", &into, &mut |_| {}, &|| false).is_err());
    }

    /// Listing entries that are path steps are skipped and recorded, never followed.
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

    /// A stopped copy says it was stopped and is not complete.
    #[test]
    fn a_copy_that_was_stopped_says_it_was_stopped() {
        let into = scratch("stopped");
        let summary = download(
            &mut a_save(),
            "/user/home/PPSA02664",
            &into,
            &mut |_| {},
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

    /// A store the target acknowledges but does not keep is not counted as copied.
    #[test]
    fn a_store_the_target_did_not_keep_is_not_counted_as_copied() {
        use pros_link::fake::{Behaviour, Fake, Store};
        use pros_link::files::Session;

        // A different-sized eboot and a target that swallows writes: the mounted-title case.
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

    /// A SELF container whose size changes on the target still counts as copied.
    #[test]
    fn a_self_container_the_target_unwraps_is_not_called_incomplete() {
        use pros_link::fake::{Behaviour, Fake, Store};
        use pros_link::files::Session;

        // A different-sized file at the path and a swallowed write model an unwrap: `SIZE`
        // afterwards differs from what was sent.
        let contents = Store::new(&[("/data/homebrew/MESA00001/libc.prx", &[0_u8; 200])]);
        let fake = Fake::start(Behaviour::Files {
            contents,
            claims: [127, 0, 0, 1],
            binary: true,
            swallows_stores: true,
        })
        .expect("the fake binds");

        // Begins with the SELF container magic, so its size is not compared.
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

    /// A SELF container that did not land at all is not counted as copied.
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

    /// A recorded, present, unchanged file is not sent again.
    #[test]
    fn an_unchanged_file_is_not_resent() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/thing.bin";
        // Different bytes on the target, so an untouched file proves no store happened.
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

    /// A changed local file is sent even where the ledger knows the path.
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

    /// A file the ledger knows but the target no longer has is sent again.
    #[test]
    fn a_recorded_file_the_target_no_longer_has_is_resent() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/eboot.bin";
        // The target holds nothing, so the listing does not name the file.
        let (fake, contents) = a_target(&[]);
        let local = b"back again after a wipe";
        let from = a_source("lost", "eboot.bin", local);

        // The digest matches the local bytes, but the file is gone from the target.
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

    /// `Resend::Everything` ignores the ledger and sends even an unchanged file.
    #[test]
    fn everything_mode_sends_even_an_unchanged_file() {
        use pros_link::files::Session;

        let path = "/data/homebrew/MESA00001/thing.bin";
        let (fake, contents) = a_target(&[(path, b"stale on target")]);
        let local = b"send me regardless";
        let from = a_source("forced", "thing.bin", local);

        // Recorded and present, so `OnlyChanged` would skip it.
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
