//! Removing a directory from a target, and everything under it.
//!
//! # Why this is not in the transport
//!
//! [`pros_link::files::Session::remove_directory`] issues `RMD` and passes the server's refusal
//! straight back, because every server refuses a directory that is not empty. That is the right
//! shape for a transport: one command, one answer, no walking about on somebody's console.
//!
//! Emptying one first is a walk, and a walk is exactly the thing that has gone badly wrong here
//! before. The backup in [`crate::transfer`] once followed `.` out of the directory it was given
//! and copied the system; the same mistake pointed the other way deletes it. So the walk lives
//! at this level, where it can be tested against a pretend target, and it carries the same
//! guards the backup grew after that.
//!
//! # What it guarantees
//!
//! - **Nothing outside the directory it was given.** A listing entry that is a path step rather
//!   than a name - empty, `.`, `..`, or anything with a separator in it - is refused, and every
//!   path built here is checked to still be under the root before a command is sent.
//! - **Depth bounded.** A listing that describes a loop stops at [`DEEPEST`] rather than
//!   recursing until the stack ends.
//! - **Children before parents.** A directory is removed after its contents, because `RMD` on a
//!   full one is refused - which is the whole reason this exists.
//! - **What it could not do comes back.** A refusal on one entry does not abandon the rest, and
//!   nothing is reported as gone that was not.

use pros_link::files::{Entry, Kind, Session};

/// How deep the walk will go before it stops and says so.
///
/// The same bound the backup uses, for the same reason: a listing that describes a loop is a
/// thing a target can produce, and a walk with no bound answers it by running out of stack.
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
/// **Counted rather than assumed.** A removal that refused half way through has done something,
/// and reporting either *done* or *failed* would describe neither.
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
        // **Named, not counted.** A refusal is the thing somebody has to act on, and a number
        // does not say which one to look at.
        let named: Vec<String> = self
            .kept
            .iter()
            .map(|one| format!("{}: {}", one.path, one.why))
            .collect();
        format!("{went}; {} left: {}", self.kept.len(), named.join("; "))
    }
}

/// Whether a listing entry names a way through the tree rather than a thing in it.
///
/// The same rule the backup walk uses. All four of these make a joined path point outside the
/// directory it was joined to, which is the whole of the danger.
fn is_a_step_rather_than_a_name(name: &str) -> bool {
    name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\')
}

/// Removes one thing, and everything under it if it is a directory.
///
/// `folder` is what the listing said it was. A file is one command; a directory is a walk.
///
/// **Nothing is removed above `path`.** Every path this builds is checked to still be under it,
/// so a listing that lies cannot make this delete something nobody named.
pub fn one(remover: &mut dyn Removes, path: &str, folder: bool) -> Gone {
    let mut gone = Gone::default();
    let root = path.trim_end_matches('/').to_owned();
    if folder {
        // **Only when it is actually empty.** A server refuses `RMD` on a directory with
        // anything left in it, so sending it anyway would usually be harmless - and *usually*
        // is the word that makes this the wrong way round. A directory whose contents could not
        // even be listed would be asked to go, and whatever the server said would be reported
        // as the answer. Nothing is asked to go here that this has not seen emptied.
        if empty_it(remover, &root, &root, 0, &mut gone) {
            match remover.remove_directory(&root) {
                Ok(()) => gone.folders += 1,
                Err(why) => gone.kept.push(Kept { path: root, why }),
            }
        } else if !gone.kept.iter().any(|one| one.path == root) {
            // **Unless the walk already said why.** A directory that could not be listed is
            // recorded by the walk itself, and saying *something inside could not be removed*
            // beside it would be a second, vaguer sentence about the same thing.
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
///
/// **One refusal does not abandon the rest.** Somebody who selected thirty files and has one
/// read-only among them wants the other twenty-nine gone and to be told about the one.
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

/// Empties a directory, depth first, without removing the directory itself.
///
/// Returns whether it is now empty - which is the only thing that licenses removing it. **Not
/// the same as "no errors"**: a listing that could not be read leaves something in there this
/// program cannot name, and a directory holding something unnameable is not an empty one.
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
                why: format!("could not be listed, so nothing in it was touched: {why}"),
            });
            return false;
        }
    };
    let mut emptied = true;

    for entry in entries {
        // **A name from the target never steers a path here.** The transport drops `.` and
        // `..`, and this does not trust it to - the backup's own comment says why, and the
        // consequence on this side is deleting something nobody listed.
        if is_a_step_rather_than_a_name(&entry.name) {
            gone.kept.push(Kept {
                path: format!("{at}/{}", entry.name),
                why: "a listing entry that is a path step rather than a name".to_owned(),
            });
            // **Not a reason to keep the directory.** `.` and `..` are in every listing and are
            // not contents; refusing to remove a directory because it contains itself would
            // refuse every directory. They are recorded and stepped over.
            continue;
        }
        let below = format!("{at}/{}", entry.name);
        // Belt and braces, like the backup. A joined path that is somehow not under the root
        // is not a path this was asked about.
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
            // **An unreadable line is something in the directory this cannot name.** Left, and
            // said - the parent's `RMD` will then refuse, which is the correct outcome: a
            // directory this could not empty is one it must not report as gone.
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
    use super::{Gone, Removes, one, these};
    use pros_link::files::{Entry, Kind};
    use std::collections::BTreeMap;

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

    /// **A folder goes, and so does everything in it.**
    ///
    /// This is the whole feature: `RMD` on a directory with anything in it is refused by every
    /// server, so a delete that only issued `RMD` could never remove a folder somebody had used.
    #[test]
    fn a_folder_takes_everything_under_it() {
        let mut target = nested();
        let gone = one(&mut target, "/data/x", true);
        assert_eq!(gone.files, 2, "{gone:?}");
        assert_eq!(gone.folders, 2, "{gone:?}");
        assert!(gone.kept.is_empty(), "{gone:?}");
    }

    /// **Children before parents**, because a full directory cannot be removed.
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

    /// **A listing entry that is a path step is refused**, whatever the server calls it.
    ///
    /// The backup once followed `.` out of its own directory and copied the system. The same
    /// walk, deleting, is the version of that mistake nobody recovers from.
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

    /// **One refusal does not abandon the rest**, and the refusal is named.
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

    /// **Nothing is reported as gone that was not.**
    #[test]
    fn a_directory_that_could_not_be_listed_is_not_reported_as_removed() {
        let mut target = Pretend::default();
        let gone = one(&mut target, "/data/nowhere", true);
        assert_eq!(gone.folders, 0, "{gone:?}");
        assert_eq!(gone.files, 0, "{gone:?}");
        assert_eq!(gone.kept.len(), 1, "the listing failure is said: {gone:?}");
    }

    /// The wording says what happened, including that nothing did.
    #[test]
    fn nothing_deleted_says_so() {
        assert_eq!(Gone::default().describe(), "nothing was deleted");
    }
}
