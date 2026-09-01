//! What is on the target's storage: titles, saves and packages.
//!
//! # What this does and does not know
//!
//! It reads directory listings and says what the entries **look like**. It does not install
//! anything, does not know how the system registers a title, and does not pretend to: those
//! need a protocol nobody here has measured.
//!
//! What it does need is nothing but the file service, which already works. Listing a folder
//! and recognising the shape of what is in it is the whole of it, and that is enough to
//! browse a library, find a save and copy one off.
//!
//! # Why the paths are parameters
//!
//! Where a target keeps titles and saves has **not been measured** by this project. The
//! shapes below are conventions, and a convention is a good guess; the caller supplies the
//! place. Same rule as the payload repository, and for the same reason. (D007)

use std::path::Path;

use pros_link::files::{Entry, Kind as EntryKind};

/// What an entry in a library directory appears to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A folder whose name has the shape of a title identifier.
    Title,
    /// A package file, waiting to be installed by something else.
    Package,
    /// A folder that is not shaped like a title - a save slot, a data folder, anything.
    Folder,
    /// A file that is not a package.
    File,
}

/// One thing found in a library directory.
#[derive(Debug, Clone)]
pub struct Item {
    /// The name as the target spells it.
    pub name: String,
    /// The title identifier, when the name is one.
    ///
    /// Kept apart from the name because a title folder is named by identifier and a person
    /// reading a list wants both, and because **only this field is safe to match on**.
    pub id: Option<String>,
    /// What it appears to be.
    pub kind: Kind,
    /// Size in bytes, when the listing carried one.
    pub size: Option<u64>,
}

impl Item {
    /// Whether this is somewhere that can be listed in turn.
    #[must_use]
    pub const fn is_enterable(&self) -> bool {
        matches!(self.kind, Kind::Title | Kind::Folder)
    }
}

/// Reads a directory listing as a library.
///
/// Lines the listing could not parse are **dropped here and only here**: they were already
/// kept and marked by the transport, and a library view is a place for things that are
/// things. A caller that wants everything asks the file service instead.
#[must_use]
pub fn scan(entries: &[Entry]) -> Vec<Item> {
    entries
        .iter()
        .filter(|entry| entry.is_usable())
        .filter(|entry| entry.name != "." && entry.name != "..")
        .map(|entry| {
            let id = title_id(&entry.name);
            // **A folder is a title only when the identifier is the whole of its name.**
            // A save folder is named after a title and is not one, and calling it a title
            // would put saves in the same column as installed software. The identifier is
            // still reported, because that is the useful half.
            let is_title = id == Some(entry.name.as_str());
            let kind = match entry.kind {
                EntryKind::Directory | EntryKind::Link if is_title => Kind::Title,
                EntryKind::Directory | EntryKind::Link => Kind::Folder,
                _ if is_package(&entry.name) => Kind::Package,
                _ => Kind::File,
            };
            Item {
                name: entry.name.clone(),
                // A package says which title it is for, somewhere in its name. A folder
                // says it at the front or not at all.
                id: if kind == Kind::Package {
                    title_id_within(&entry.name).map(str::to_owned)
                } else {
                    id.map(str::to_owned)
                },
                kind,
                size: entry.size,
            }
        })
        .collect()
}

/// Reads a directory on **this** machine as a library.
///
/// # Why the same shape as a target's
///
/// Because the useful view is both at once. A person wants to see what is here beside what
/// is there and move things between them, and that comparison is only possible if the two
/// sides are described the same way.
///
/// # Errors
///
/// When the directory cannot be read. **A directory that is not there is an empty list**,
/// not a failure: this project's own folders do not exist until something is put in them,
/// and reporting that as an error would make an ordinary state look like a fault.
pub fn here(path: &Path) -> Result<Vec<Item>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|why| why.to_string())? {
        let entry = entry.map_err(|why| why.to_string())?;
        let name = entry.file_name().to_string_lossy().to_string();
        // `file_type` does not follow links and `is_dir` does, which is the difference
        // between describing a link and describing whatever it points at.
        let kind = entry.file_type().map_err(|why| why.to_string())?;
        let id = title_id(&name).map(str::to_owned);
        let is_title = id.as_deref() == Some(name.as_str());
        items.push(Item {
            kind: if kind.is_dir() && is_title {
                Kind::Title
            } else if kind.is_dir() {
                Kind::Folder
            } else if is_package(&name) {
                Kind::Package
            } else {
                Kind::File
            },
            size: entry.metadata().ok().map(|about| about.len()),
            id,
            name,
        });
    }
    items.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(items)
}

/// The title identifier in a name, if the name has that shape.
///
/// Four letters and five digits, which is the published form of these identifiers. Matched
/// as a **shape only**: this says the name looks like an identifier, not that any title
/// exists, and certainly not which one.
#[must_use]
pub fn title_id(name: &str) -> Option<&str> {
    let candidate = name.split(['-', '_', ' ']).next().unwrap_or(name);
    is_identifier(candidate).then_some(candidate)
}

/// A title identifier anywhere in a name, rather than only at the front.
///
/// # Why packages need this and folders do not
///
/// A title's folder is named `PPSA01650` and nothing else, so the strict rule is right there.
/// A package is named `PS5_LAPY20011_v1.05.pkg` - the identifier is in the middle, wrapped in
/// a platform and a version by whoever built it.
///
/// Kept separate from [`title_id`] rather than loosening it, because loosening the strict one
/// would let a folder called `backup_PPSA01650_old` read as that title, and a folder is a
/// thing this project copies into and out of.
#[must_use]
pub fn title_id_within(name: &str) -> Option<&str> {
    name.split(['-', '_', ' ', '.'])
        .find(|part| is_identifier(part))
}

/// Whether a word has the shape of a title identifier: four letters, five digits.
fn is_identifier(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.len() == 9
        && bytes
            .get(..4)
            .is_some_and(|letters| letters.iter().all(u8::is_ascii_alphabetic))
        && bytes
            .get(4..)
            .is_some_and(|digits| digits.iter().all(u8::is_ascii_digit))
}

/// Whether a file name is a package.
///
/// Case-insensitively, because the extension is a convention and a target's filesystem is
/// not the arbiter of how somebody typed it.
fn is_package(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pkg"))
}

/// Everything that looks like a title, in the order the listing gave them.
#[must_use]
pub fn titles(items: &[Item]) -> Vec<&Item> {
    items
        .iter()
        .filter(|item| item.kind == Kind::Title)
        .collect()
}

/// How much the listed items add up to, counting only what stated a size.
///
/// **Returns what it counted as well as the total**, because a total over a listing where
/// half the entries carried no size is a number that looks complete and is not.
#[must_use]
pub fn total_size(items: &[Item]) -> (u64, usize) {
    let counted: Vec<u64> = items.iter().filter_map(|item| item.size).collect();
    (counted.iter().sum(), counted.len())
}

#[cfg(test)]
mod tests {

    use pros_link::files::{Entry, Kind as EntryKind};

    use super::{Kind, scan, title_id, titles, total_size};

    fn entry(name: &str, kind: EntryKind, size: Option<u64>) -> Entry {
        Entry {
            name: name.to_owned(),
            kind,
            size,
            raw: name.to_owned(),
        }
    }

    fn listing() -> Vec<Entry> {
        vec![
            entry("PPSA02664", EntryKind::Directory, Some(0)),
            entry("CUSA12345", EntryKind::Directory, Some(0)),
            entry("sce_sys", EntryKind::Directory, Some(0)),
            entry("something.pkg", EntryKind::File, Some(4096)),
            entry("readme.txt", EntryKind::File, Some(120)),
            Entry {
                name: "total 48".to_owned(),
                kind: EntryKind::Unrecognised,
                size: None,
                raw: "total 48".to_owned(),
            },
        ]
    }

    /// A folder named like an identifier is a title; one that is not, is not.
    #[test]
    fn a_title_is_told_from_an_ordinary_folder_by_its_name() {
        let items = scan(&listing());
        let found = titles(&items);
        assert_eq!(found.len(), 2);
        assert_eq!(
            found.first().map(|item| item.name.as_str()),
            Some("PPSA02664")
        );

        let other = items.iter().find(|item| item.name == "sce_sys").unwrap();
        assert_eq!(other.kind, Kind::Folder, "a data folder is not a title");
    }

    /// A package is a file this cannot install, and saying which files are packages is still
    /// worth doing - it is what somebody is looking for.
    #[test]
    fn a_package_is_recognised_by_its_extension() {
        let items = scan(&listing());
        let package = items
            .iter()
            .find(|item| item.name == "something.pkg")
            .unwrap();
        assert_eq!(package.kind, Kind::Package);

        let plain = items.iter().find(|item| item.name == "readme.txt").unwrap();
        assert_eq!(plain.kind, Kind::File);
    }

    /// **A package names the title it installs, in the middle of its own name.**
    ///
    /// Real ones from a target: `PS5_LAPY20011_v1.05.pkg` is for `LAPY20011`, and
    /// `Store-R2-PS5.pkg` is for nothing this can name - which is a package that installs
    /// something without a title identifier in its file name, not a failure.
    #[test]
    fn a_package_says_which_title_it_is_for() {
        let items = scan(&[
            entry("PS5_LAPY20011_v1.05.pkg", EntryKind::File, Some(1)),
            entry("Store-R2-PS5.pkg", EntryKind::File, Some(1)),
        ]);
        assert_eq!(items[0].id.as_deref(), Some("LAPY20011"));
        assert_eq!(
            items[1].id, None,
            "it named a title that is not in the name"
        );
    }

    /// Looking anywhere is for packages only. A folder that mentions a title is not that
    /// title, and folders are what this project copies into and out of.
    #[test]
    fn a_folder_that_merely_mentions_a_title_is_not_that_title() {
        let items = scan(&[entry("backup_PPSA01650_old", EntryKind::Directory, Some(0))]);
        assert_eq!(items[0].kind, Kind::Folder);
        assert_eq!(
            items[0].id, None,
            "a folder named after a title in the middle was read as that title"
        );
    }

    /// A line the transport could not read is not an item.
    ///
    /// It was already kept and marked where that mattered. A library view is a place for
    /// things that are things.
    #[test]
    fn an_unreadable_listing_line_is_not_a_title() {
        let items = scan(&listing());
        assert!(
            items.iter().all(|item| item.name != "total 48"),
            "a header line became an item"
        );
        assert_eq!(items.len(), 5);
    }

    /// This machine is read with the same rules, so the two sides can be compared.
    #[test]
    fn a_local_directory_reads_the_same_way() {
        let scratch = std::env::temp_dir().join(format!("pros-here-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        std::fs::create_dir_all(scratch.join("PPSA02664")).expect("a title folder");
        std::fs::create_dir_all(scratch.join("PPSA02664-SAVE00")).expect("a save folder");
        std::fs::write(scratch.join("update.pkg"), b"x").expect("a package");

        let items = super::here(&scratch).expect("it reads");
        let title = items.iter().find(|item| item.name == "PPSA02664").unwrap();
        let save = items
            .iter()
            .find(|item| item.name == "PPSA02664-SAVE00")
            .unwrap();
        let package = items.iter().find(|item| item.name == "update.pkg").unwrap();

        assert_eq!(title.kind, Kind::Title);
        assert_eq!(
            save.kind,
            Kind::Folder,
            "a save folder is not a title here either"
        );
        assert_eq!(save.id.as_deref(), Some("PPSA02664"));
        assert_eq!(package.kind, Kind::Package);
    }

    /// **A folder that is not there is an empty list, not a failure.**
    ///
    /// This project's own directories do not exist until something is put in them, and
    /// reporting that as an error makes an ordinary state look like a fault.
    #[test]
    fn a_local_directory_that_is_not_there_is_empty_rather_than_broken() {
        let nowhere = std::env::temp_dir().join("pros-there-is-no-such-directory-here");
        assert_eq!(super::here(&nowhere).expect("not an error").len(), 0);
    }

    /// A folder named after a title is not a title.
    ///
    /// Save folders are named that way, and putting them in the same column as installed
    /// software would be a claim nobody made. The identifier is still reported, because a
    /// person looking at a save wants to know whose it is.
    #[test]
    fn a_folder_named_after_a_title_is_not_a_title() {
        let items = scan(&[entry("PPSA02664-SAVE00", EntryKind::Directory, Some(0))]);
        let save = items.first().unwrap();
        assert_eq!(save.kind, Kind::Folder, "a save folder became a title");
        assert_eq!(
            save.id.as_deref(),
            Some("PPSA02664"),
            "and it should still say whose save it is"
        );
    }

    /// The shape is four letters and five digits, and nothing else is claimed.
    #[test]
    fn the_identifier_is_a_shape_and_not_a_lookup() {
        assert_eq!(title_id("PPSA02664"), Some("PPSA02664"));
        assert_eq!(title_id("CUSA00001"), Some("CUSA00001"));
        // Enough to be an identifier and part of a longer name, which happens in save
        // folders that append a slot or a user.
        assert_eq!(title_id("PPSA02664-SAVE00"), Some("PPSA02664"));

        assert_eq!(title_id("sce_sys"), None);
        assert_eq!(title_id("PPSA0266"), None, "eight is not nine");
        assert_eq!(title_id("PPSAX2664"), None, "a letter among the digits");
    }

    /// **A total over a listing where half the sizes are missing looks complete and is not.**
    ///
    /// So the count comes back with it, and a caller that wants to say "3.2 GB" can first
    /// check whether it is over everything or over some of it.
    #[test]
    fn a_total_says_how_many_it_could_count() {
        let items = scan(&listing());
        let (total, counted) = total_size(&items);
        assert_eq!(total, 4096 + 120);
        assert_eq!(counted, 5, "the folders stated zero, which is a size");

        let partial = scan(&[
            entry("a.pkg", EntryKind::File, Some(10)),
            entry("b.pkg", EntryKind::File, None),
        ]);
        assert_eq!(total_size(&partial), (10, 1));
    }
}
