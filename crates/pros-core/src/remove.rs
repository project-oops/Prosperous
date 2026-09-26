//! Removing a directory from a target, and everything under it.
//!
//! [`pros_link::files::Session::remove_directory`] issues one `RMD` and returns the server's
//! refusal of a non-empty directory; emptying it first is a walk, and the walk lives here where
//! a pretend target can test it. It keeps the same guards as the backup walk in
//! [`crate::transfer`]:
//!
//! - nothing outside the named directory: a listing entry that is a path step (empty, `.`,
//!   `..`, or containing a separator) is refused, and every built path is checked to be under
//!   the root before a command is sent;
//! - depth is bounded by [`DEEPEST`], so a listing that describes a loop stops;
//! - children go before parents, because `RMD` on a full directory is refused;
//! - a refusal does not abandon the rest, and nothing is reported gone that was not.

use std::time::Duration;

use pros_link::Link;
use pros_link::files::{Entry, Kind, Session};

/// How deep the walk will go before it stops and says so.
///
/// The same bound the backup uses: a target can produce a listing that describes a loop.
pub const DEEPEST: usize = 16;

/// The commands a removal needs, so the walk can be tested without a console.
pub trait Removes {
    /// Lists a directory.
    ///
    /// # Errors
    ///
    /// Whatever the transport reports, as text.
    fn list(&mut self, path: &str) -> Result<Vec<Entry>, String>;

    /// Deletes one file.
    ///
    /// # Errors
    ///
    /// As [`Removes::list`].
    fn delete_file(&mut self, path: &str) -> Result<(), String>;

    /// Removes one directory, which the server will refuse unless it is empty.
    ///
    /// # Errors
    ///
    /// As [`Removes::list`].
    fn remove_directory(&mut self, path: &str) -> Result<(), String>;
}

impl Removes for Session {
    fn list(&mut self, path: &str) -> Result<Vec<Entry>, String> {
        Self::list(self, path).map_err(|why| why.to_string())
    }

    fn delete_file(&mut self, path: &str) -> Result<(), String> {
        Self::delete_file(self, path).map_err(|why| why.to_string())
    }

    fn remove_directory(&mut self, path: &str) -> Result<(), String> {
        Self::remove_directory(self, path).map_err(|why| why.to_string())
    }
}

/// Something that was left where it was, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The path, as the target sees it.
    pub path: String,
    /// What stopped it.
    pub why: String,
}

/// What a removal actually did.
///
/// Counted, because a removal refused part way through is neither done nor failed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gone {
    /// How many files were deleted.
    pub files: usize,
    /// How many directories were removed.
    pub folders: usize,
    /// Everything that is still there, with the reason.
    pub kept: Vec<Kept>,
}

impl Gone {
    /// How many things went, of either kind.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.files + self.folders
    }

    /// A sentence for somebody who pressed delete.
    #[must_use]
    pub fn describe(&self) -> String {
        let went = match (self.files, self.folders) {
            (0, 0) => "nothing was deleted".to_owned(),
            (files, 0) => format!("{files} deleted"),
            (0, folders) => format!("{folders} folders removed"),
            (files, folders) => format!("{files} deleted, {folders} folders removed"),
        };
        if self.kept.is_empty() {
            return went;
        }
        // Named, not counted: the user acts on a specific refusal.
        let named: Vec<String> = self
            .kept
            .iter()
            .map(|one| format!("{}: {}", one.path, one.why))
            .collect();
        format!("{went}; {} left: {}", self.kept.len(), named.join("; "))
    }
}

/// The reason recorded when a path could not be listed as a directory. Shared so the top-level
/// removal can recognise its own walk's message and act on it.
const UNLISTABLE: &str = "could not be listed, so nothing in it was touched";

/// Whether the walk failed because `root` itself could not be listed - meaning it was called a
/// folder but is not a directory (a symlink, or a mislabelled file), rather than a directory whose
/// contents refused.
fn failed_to_list(gone: &Gone, root: &str) -> bool {
    gone.kept
        .iter()
        .any(|one| one.path == root && one.why.starts_with(UNLISTABLE))
}

/// Whether a listing entry names a way through the tree rather than a thing in it.
///
/// The same rule the backup walk uses: each of these makes a joined path point outside the
/// directory it was joined to.
fn is_a_step_rather_than_a_name(name: &str) -> bool {
    name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\')
}

/// Removes one thing, and everything under it if it is a directory.
///
/// `folder` is what the listing said it was. A file is one command; a directory is a walk.
/// Every path this builds is checked to be under `path`, so a lying listing cannot make it
/// delete something nobody named.
pub fn one(remover: &mut dyn Removes, path: &str, folder: bool) -> Gone {
    let mut gone = Gone::default();
    let root = path.trim_end_matches('/').to_owned();
    if folder {
        // `RMD` is sent only for a directory this has seen emptied, so a server's answer is
        // never reported for a directory whose contents could not be listed.
        if empty_it(remover, &root, &root, 0, &mut gone) {
            match remover.remove_directory(&root) {
                Ok(()) => gone.folders += 1,
                Err(why) => gone.kept.push(Kept { path: root, why }),
            }
        } else if failed_to_list(&gone, &root) {
            // Called a folder but not listable as one: usually a symlink, which the library
            // groups with folders, and a dangling one fails to list. It is removed as a single
            // thing, never followed: `DELE` first (unlinks a symlink or file), then `RMD`. A
            // real directory that could not be listed refuses both.
            let by_file = remover.delete_file(&root);
            let by_dir = if by_file.is_err() {
                Some(remover.remove_directory(&root))
            } else {
                None
            };
            if by_file.is_ok() || matches!(by_dir, Some(Ok(()))) {
                // It was the link (or a mislabelled file): drop the listing failure, count it gone.
                gone.kept.retain(|one| one.path != root);
                gone.files += 1;
            } else if let Some(kept) = gone.kept.iter_mut().find(|one| one.path == root) {
                // Report both refusals, which are what the user acts on.
                let dele = by_file.err().unwrap_or_default();
                let rmd = by_dir.and_then(Result::err).unwrap_or_default();
                kept.why = format!(
                    "not a listable directory, and could not be removed - DELE: {dele}; RMD: {rmd}"
                );
            }
        } else if !gone.kept.iter().any(|one| one.path == root) {
            // Unless the walk already recorded why, which would make this a vaguer duplicate.
            gone.kept.push(Kept {
                path: root,
                why: "left, because something inside it could not be removed".to_owned(),
            });
        }
    } else {
        match remover.delete_file(&root) {
            Ok(()) => gone.files += 1,
            Err(why) => gone.kept.push(Kept { path: root, why }),
        }
    }
    gone
}

/// Removes several things, carrying on past one that refuses.
pub fn these(remover: &mut dyn Removes, what: &[(String, bool)]) -> Gone {
    let mut all = Gone::default();
    for (path, folder) in what {
        let gone = one(remover, path, *folder);
        all.files += gone.files;
        all.folders += gone.folders;
        all.kept.extend(gone.kept);
    }
    all
}

/// A last resort for what the file service could not remove, over the target's shell.
///
/// The target's `ftpsrv` empties a payload directory but refuses `RMD` on the empty directory,
/// and cannot unlink a broken symlink. The shell can, and this uses the least-powerful command
/// that fits, never a recursive force:
///
/// - a directory: `rmdir`, which removes only an empty directory and refuses a full one;
/// - a file or symlink: `rm -f` with no `-r`, which unlinks one name and refuses a directory.
///
/// So nothing here can delete more than the single thing it names.
pub trait Forces {
    /// Removes exactly one thing over the shell: an empty directory (`folder`) or a single file.
    ///
    /// # Errors
    ///
    /// Whatever the shell reports, as text - a non-empty directory, a missing command, a refusal.
    fn force(&mut self, path: &str, folder: bool) -> Result<(), String>;
}

/// How long the shell is given to answer a removal before its output is taken as complete.
const SHELL_SETTLE: Duration = Duration::from_millis(1500);

/// A [`Forces`] that runs on the target's shell service.
#[derive(Debug)]
pub struct ShellForce<'a> {
    /// The target, for the shell service.
    link: &'a Link,
}

impl<'a> ShellForce<'a> {
    /// A shell fallback for `link`.
    #[must_use]
    pub fn new(link: &'a Link) -> Self {
        Self { link }
    }
}

impl Forces for ShellForce<'_> {
    fn force(&mut self, path: &str, folder: bool) -> Result<(), String> {
        let command = force_command(path, folder)?;
        // `rmdir` and `rm -f` are silent on success, so any output is the reason it failed.
        let said = pros_link::shell::run(self.link, &command, SHELL_SETTLE)
            .map_err(|why| why.to_string())?;
        if said.trim().is_empty() {
            Ok(())
        } else {
            Err(said.trim().to_owned())
        }
    }
}

/// Builds the safe shell command to remove one thing, or refuses a path that has no business being
/// force-removed.
///
/// Never recursive: `rmdir` for a directory, `rm -f` for a file. A path that is empty, the
/// root, or climbs with `..` is refused rather than quoted into a command.
fn force_command(path: &str, folder: bool) -> Result<String, String> {
    let path = path.trim().trim_end_matches('/');
    if path.is_empty() || path == "/" || path == "~" {
        return Err(format!(
            "refusing to force-remove {path:?}: too broad a path"
        ));
    }
    if path.split('/').any(|segment| segment == "..") {
        return Err(format!(
            "refusing to force-remove {path:?}: it climbs with '..'"
        ));
    }
    // Single-quote for the shell, with the one escape single quotes need.
    let quoted = format!("'{}'", path.replace('\'', "'\\''"));
    Ok(if folder {
        format!("rmdir {quoted}")
    } else {
        format!("rm -f {quoted}")
    })
}

/// Removes several things over the file service, then finishes over the shell whatever the file
/// service left behind.
///
/// The shell only touches a selection the file service could not fully remove, and only with
/// `force_command`'s non-recursive commands.
pub fn these_then_force(
    remover: &mut dyn Removes,
    forcer: &mut dyn Forces,
    what: &[(String, bool)],
) -> Gone {
    let mut gone = these(remover, what);
    if gone.kept.is_empty() {
        return gone;
    }
    for (path, folder) in what {
        let root = path.trim_end_matches('/').to_owned();
        let under = format!("{root}/");
        // Is this selection, or anything the walk built under it, still there?
        if !gone
            .kept
            .iter()
            .any(|one| one.path == root || one.path.starts_with(&under))
        {
            continue;
        }
        match forcer.force(&root, *folder) {
            Ok(()) => {
                // Gone now: drop what was kept under this selection, and count it.
                gone.kept
                    .retain(|one| one.path != root && !one.path.starts_with(&under));
                if *folder {
                    gone.folders += 1;
                } else {
                    gone.files += 1;
                }
            }
            Err(why) => {
                // Keep the file service's reason and add the shell's.
                if let Some(kept) = gone.kept.iter_mut().find(|one| one.path == root) {
                    kept.why = format!("{}; the shell could not remove it either: {why}", kept.why);
                }
            }
        }
    }
    gone
}

/// Empties a directory, depth first, without removing the directory itself.
///
/// Returns whether it is now empty, which is the only thing that licenses removing it. Not the
/// same as "no errors": an unreadable listing leaves something in there this cannot name.
fn empty_it(
    remover: &mut dyn Removes,
    root: &str,
    at: &str,
    depth: usize,
    gone: &mut Gone,
) -> bool {
    if depth > DEEPEST {
        gone.kept.push(Kept {
            path: at.to_owned(),
            why: format!("deeper than {DEEPEST} directories, so the walk stopped here"),
        });
        return false;
    }
    let entries = match remover.list(at) {
        Ok(entries) => entries,
        Err(why) => {
            gone.kept.push(Kept {
                path: at.to_owned(),
                why: format!("{UNLISTABLE}: {why}"),
            });
            return false;
        }
    };
    let mut emptied = true;

    for entry in entries {
        // A name from the target never steers a path. The transport drops `.` and `..`; this
        // does not rely on it.
        if is_a_step_rather_than_a_name(&entry.name) {
            gone.kept.push(Kept {
                path: format!("{at}/{}", entry.name),
                why: "a listing entry that is a path step rather than a name".to_owned(),
            });
            // Not contents, so not a reason to keep the directory: every listing has `.`.
            continue;
        }
        let below = format!("{at}/{}", entry.name);
        // A second check, as in the backup: a joined path not under the root is not touched.
        if !below.starts_with(root) {
            gone.kept.push(Kept {
                path: below,
                why: "outside the directory that was named".to_owned(),
            });
            emptied = false;
            continue;
        }
        match entry.kind {
            Kind::Directory => {
                if empty_it(remover, root, &below, depth + 1, gone) {
                    match remover.remove_directory(&below) {
                        Ok(()) => gone.folders += 1,
                        Err(why) => {
                            gone.kept.push(Kept { path: below, why });
                            emptied = false;
                        }
                    }
                } else {
                    gone.kept.push(Kept {
                        path: below,
                        why: "left, because something inside it could not be removed".to_owned(),
                    });
                    emptied = false;
                }
            }
            // An unreadable line is something this cannot name, so the directory is not empty
            // and must not be reported gone.
            Kind::Unrecognised => {
                gone.kept.push(Kept {
                    path: at.to_owned(),
                    why: format!("a listing line that could not be read: {}", entry.raw),
                });
                emptied = false;
            }
            _ => match remover.delete_file(&below) {
                Ok(()) => gone.files += 1,
                Err(why) => {
                    gone.kept.push(Kept { path: below, why });
                    emptied = false;
                }
            },
        }
    }
    emptied
}

#[cfg(test)]
mod tests {
    use super::{Forces, Gone, Removes, force_command, one, these, these_then_force};
    use pros_link::files::{Entry, Kind};
    use std::collections::BTreeMap;

    /// A pretend shell, so the fallback can be tested without a target.
    #[derive(Default)]
    struct PretendShell {
        /// Every command it was asked to run.
        did: Vec<String>,
        /// Paths it refuses to remove.
        refuses: Vec<String>,
    }

    impl Forces for PretendShell {
        fn force(&mut self, path: &str, folder: bool) -> Result<(), String> {
            self.did
                .push(format!("{} {path}", if folder { "rmdir" } else { "rm" }));
            if self.refuses.iter().any(|one| one == path) {
                return Err("still there".to_owned());
            }
            Ok(())
        }
    }

    /// A target made of a map, so a walk can be checked without a console.
    #[derive(Default)]
    struct Pretend {
        /// Directory path to what is in it.
        tree: BTreeMap<String, Vec<Entry>>,
        /// Every command issued, in order.
        did: Vec<String>,
        /// Paths the pretend server refuses to delete.
        refuses: Vec<String>,
    }

    fn entry(name: &str, kind: Kind) -> Entry {
        Entry {
            name: name.to_owned(),
            kind,
            size: Some(1),
            raw: name.to_owned(),
        }
    }

    impl Removes for Pretend {
        fn list(&mut self, path: &str) -> Result<Vec<Entry>, String> {
            self.did.push(format!("list {path}"));
            self.tree
                .get(path)
                .cloned()
                .ok_or_else(|| format!("no such directory: {path}"))
        }

        fn delete_file(&mut self, path: &str) -> Result<(), String> {
            self.did.push(format!("dele {path}"));
            if self.refuses.iter().any(|one| one == path) {
                return Err("permission denied".to_owned());
            }
            Ok(())
        }

        fn remove_directory(&mut self, path: &str) -> Result<(), String> {
            self.did.push(format!("rmd {path}"));
            if self.refuses.iter().any(|one| one == path) {
                return Err("directory not empty".to_owned());
            }
            Ok(())
        }
    }

    /// A tree two deep, with a file at each level.
    fn nested() -> Pretend {
        let mut tree = BTreeMap::new();
        tree.insert(
            "/data/x".to_owned(),
            vec![entry("a.txt", Kind::File), entry("inner", Kind::Directory)],
        );
        tree.insert("/data/x/inner".to_owned(), vec![entry("b.txt", Kind::File)]);
        Pretend {
            tree,
            ..Pretend::default()
        }
    }

    /// A folder goes, and so does everything in it.
    #[test]
    fn a_folder_takes_everything_under_it() {
        let mut target = nested();
        let gone = one(&mut target, "/data/x", true);
        assert_eq!(gone.files, 2, "{gone:?}");
        assert_eq!(gone.folders, 2, "{gone:?}");
        assert!(gone.kept.is_empty(), "{gone:?}");
    }

    /// Children go before parents, because a full directory cannot be removed.
    #[test]
    fn the_inside_goes_before_the_directory_holding_it() {
        let mut target = nested();
        let _ = one(&mut target, "/data/x", true);
        let inner = target
            .did
            .iter()
            .position(|one| one == "rmd /data/x/inner")
            .expect("the inner directory was removed");
        let file = target
            .did
            .iter()
            .position(|one| one == "dele /data/x/inner/b.txt")
            .expect("the file inside it was deleted");
        let outer = target
            .did
            .iter()
            .position(|one| one == "rmd /data/x")
            .expect("the outer directory was removed");
        assert!(file < inner, "{:?}", target.did);
        assert!(inner < outer, "{:?}", target.did);
    }

    /// A listing entry that is a path step is refused, and no command is sent for it.
    #[test]
    fn a_listing_that_points_out_of_the_directory_is_refused() {
        let mut tree = BTreeMap::new();
        tree.insert(
            "/data/x".to_owned(),
            vec![
                entry("..", Kind::Directory),
                entry("../../etc", Kind::Directory),
                entry("ok.txt", Kind::File),
            ],
        );
        let mut target = Pretend {
            tree,
            ..Pretend::default()
        };
        let gone = one(&mut target, "/data/x", true);
        assert_eq!(gone.files, 1, "only the real file: {gone:?}");
        assert_eq!(gone.kept.len(), 2, "{gone:?}");
        assert!(
            !target.did.iter().any(|one| one.contains("..")),
            "a command was sent for a path step: {:?}",
            target.did
        );
    }

    /// One refusal does not abandon the rest, and the refusal is named.
    #[test]
    fn a_refusal_leaves_the_rest_of_the_selection_alone() {
        let mut target = nested();
        target.refuses.push("/data/x/a.txt".to_owned());
        let gone = one(&mut target, "/data/x", true);
        assert_eq!(gone.files, 1, "the other file still went: {gone:?}");
        assert_eq!(gone.kept.len(), 2, "the file, and the directory it is in");
        assert!(gone.describe().contains("a.txt"), "{}", gone.describe());
    }

    /// A selection of several is one walk each, and the counts add up.
    #[test]
    fn several_things_are_removed_in_one_go() {
        let mut target = nested();
        let gone = these(
            &mut target,
            &[
                ("/data/x".to_owned(), true),
                ("/data/loose.bin".to_owned(), false),
            ],
        );
        assert_eq!(gone.files, 3, "{gone:?}");
        assert_eq!(gone.folders, 2, "{gone:?}");
        assert_eq!(gone.total(), 5);
    }

    /// An unlistable directory that refuses both `DELE` and `RMD` is not reported as removed.
    #[test]
    fn a_directory_that_could_not_be_listed_is_not_reported_as_removed() {
        let mut target = Pretend::default();
        // The server refuses it as a file and as a directory, as it does a non-empty directory.
        target.refuses.push("/data/nowhere".to_owned());
        let gone = one(&mut target, "/data/nowhere", true);
        assert_eq!(gone.folders, 0, "{gone:?}");
        assert_eq!(gone.files, 0, "{gone:?}");
        assert_eq!(gone.kept.len(), 1, "nothing was claimed gone: {gone:?}");
        assert!(
            gone.kept[0].why.contains("could not be removed"),
            "{gone:?}"
        );
    }

    /// A symlink labelled a folder is unlinked as a single thing, not walked.
    #[test]
    fn a_symlink_labelled_a_folder_is_deleted_as_a_single_thing() {
        // No such directory in the tree, so `list` fails, as it does for a symlink on the target.
        let mut target = Pretend::default();
        let gone = one(&mut target, "/data/pldmgr/payloads/pltauth-patch.elf", true);
        assert_eq!(gone.files, 1, "the link itself was unlinked: {gone:?}");
        assert_eq!(gone.folders, 0, "{gone:?}");
        assert!(gone.kept.is_empty(), "nothing was left behind: {gone:?}");
        assert!(
            target
                .did
                .iter()
                .any(|one| one == "dele /data/pldmgr/payloads/pltauth-patch.elf"),
            "it was deleted, not walked: {:?}",
            target.did
        );
    }

    /// The shell's `rmdir` removes a directory the file service emptied but would not remove.
    #[test]
    fn the_shell_removes_an_emptied_directory_the_file_service_would_not() {
        let mut tree = BTreeMap::new();
        tree.insert(
            "/data/pldmgr/payloads/pltauth-patch".to_owned(),
            vec![
                entry("pltauth-patch.elf", Kind::File),
                entry("pltauth-patch.json", Kind::File),
            ],
        );
        // The file service empties it but refuses to remove the directory itself.
        let mut ftp = Pretend {
            tree,
            refuses: vec!["/data/pldmgr/payloads/pltauth-patch".to_owned()],
            ..Pretend::default()
        };
        let mut shell = PretendShell::default();
        let gone = these_then_force(
            &mut ftp,
            &mut shell,
            &[("/data/pldmgr/payloads/pltauth-patch".to_owned(), true)],
        );
        assert_eq!(
            gone.files, 2,
            "the two payload files went over the file service: {gone:?}"
        );
        assert_eq!(
            gone.folders, 1,
            "the empty directory went over the shell: {gone:?}"
        );
        assert!(gone.kept.is_empty(), "nothing was left: {gone:?}");
        assert!(
            shell
                .did
                .iter()
                .any(|c| c == "rmdir /data/pldmgr/payloads/pltauth-patch"),
            "it used rmdir, not a recursive force: {:?}",
            shell.did
        );
    }

    /// What the file service handled cleanly never reaches the shell.
    #[test]
    fn a_clean_removal_does_not_touch_the_shell() {
        let mut ftp = nested();
        let mut shell = PretendShell::default();
        let gone = these_then_force(&mut ftp, &mut shell, &[("/data/x".to_owned(), true)]);
        assert!(gone.kept.is_empty(), "{gone:?}");
        assert!(
            shell.did.is_empty(),
            "the shell was not needed: {:?}",
            shell.did
        );
    }

    /// A thing neither service can remove reports both refusals and is not claimed gone.
    #[test]
    fn what_neither_service_can_remove_says_both_refused() {
        // Not in the tree, so it cannot be listed; and both services refuse it.
        let mut ftp = Pretend {
            refuses: vec!["/data/x".to_owned()],
            ..Pretend::default()
        };
        let mut shell = PretendShell {
            refuses: vec!["/data/x".to_owned()],
            ..PretendShell::default()
        };
        let gone = these_then_force(&mut ftp, &mut shell, &[("/data/x".to_owned(), true)]);
        assert_eq!(gone.folders, 0, "{gone:?}");
        assert_eq!(gone.kept.len(), 1, "{gone:?}");
        assert!(
            gone.kept[0].why.contains("the shell could not remove it"),
            "both refusals are said: {gone:?}"
        );
    }

    /// The fallback command is never recursive, and a broad or climbing path is refused.
    #[test]
    fn the_force_command_is_never_recursive_and_guards_the_path() {
        assert_eq!(
            force_command("/data/x/dir", true).unwrap(),
            "rmdir '/data/x/dir'"
        );
        assert_eq!(
            force_command("/data/x/file.elf", false).unwrap(),
            "rm -f '/data/x/file.elf'"
        );
        assert!(!force_command("/data/x", true).unwrap().contains("-r"));
        assert!(!force_command("/data/x", false).unwrap().contains("-r"));
        assert!(force_command("/", true).is_err());
        assert!(force_command("", false).is_err());
        assert!(force_command("/data/../etc", false).is_err());
        // A single quote in a name is escaped so it cannot end the quoting.
        assert_eq!(
            force_command("/data/it's", false).unwrap(),
            "rm -f '/data/it'\\''s'"
        );
    }

    /// The wording says what happened, including that nothing did.
    #[test]
    fn nothing_deleted_says_so() {
        assert_eq!(Gone::default().describe(), "nothing was deleted");
    }
}
