//! What is on the target's storage: titles, saves and packages.
//!
//! It reads directory listings through the file service and says what the entries look like;
//! it installs nothing and knows nothing of how the system registers a title. Paths are
//! parameters supplied by the caller, since the name shapes here are conventions. (D007)

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
    /// Kept apart from the name because a reader wants both, and only this field is safe to
    /// match on.
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
/// Lines the listing could not parse are dropped here; the transport has already kept and
/// marked them for callers that want everything.
#[must_use]
pub fn scan(entries: &[Entry]) -> Vec<Item> {
    entries
        .iter()
        .filter(|entry| entry.is_usable())
        .filter(|entry| entry.name != "." && entry.name != "..")
        .map(|entry| {
            let id = title_id(&entry.name);
            // A folder is a title only when the identifier is its whole name: a save folder
            // named after a title is not one, though its identifier is still reported.
            let is_title = id == Some(entry.name.as_str());
            let kind = match entry.kind {
                EntryKind::Directory | EntryKind::Link if is_title => Kind::Title,
                EntryKind::Directory | EntryKind::Link => Kind::Folder,
                _ if is_package(&entry.name) => Kind::Package,
                _ => Kind::File,
            };
            Item {
                name: entry.name.clone(),
                // A package names its title anywhere in its name; a folder at the front or not
                // at all.
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

/// Reads a directory on this machine as a library, in the same shape as a target's so the
/// two sides can be compared.
///
/// # Errors
///
/// When the directory cannot be read. A directory that is not there is an empty list: this
/// program's folders do not exist until something is put in them.
pub fn here(path: &Path) -> crate::Result<Vec<Item>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut items = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        // `file_type` does not follow links, so a link is described rather than its target.
        let kind = entry.file_type()?;
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

/// The title identifier at the front of a name, if it has that shape.
///
/// Four letters and five digits. A shape only: it does not say that any title exists.
#[must_use]
pub fn title_id(name: &str) -> Option<&str> {
    let candidate = name.split(['-', '_', ' ']).next().unwrap_or(name);
    is_identifier(candidate).then_some(candidate)
}

/// A title identifier anywhere in a name, rather than only at the front.
///
/// For packages, which wrap the identifier in a platform and a version
/// (`PS5_LAPY20011_v1.05.pkg`). Separate from [`title_id`] so a folder such as
/// `backup_PPSA01650_old` is not read as that title.
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

/// Whether a file name is a package, by its extension in any case.
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
/// Returns how many were counted beside the total, since a total over partly sized entries
/// looks complete and is not.
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

    /// A folder named like an identifier is a title; any other folder is not.
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

    /// A package is recognised by its extension.
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

    /// A package's title identifier is found in the middle of its name, when it has one.
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

    /// A folder that mentions a title mid-name is not that title.
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
    #[test]
    fn an_unreadable_listing_line_is_not_a_title() {
        let items = scan(&listing());
        assert!(
            items.iter().all(|item| item.name != "total 48"),
            "a header line became an item"
        );
        assert_eq!(items.len(), 5);
    }

    /// A local directory is read with the same rules as a target's.
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

    /// A local directory that does not exist is an empty list, not an error.
    #[test]
    fn a_local_directory_that_is_not_there_is_empty_rather_than_broken() {
        let nowhere = std::env::temp_dir().join("pros-there-is-no-such-directory-here");
        assert_eq!(super::here(&nowhere).expect("not an error").len(), 0);
    }

    /// A save folder named after a title is a folder that still reports the identifier.
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

    /// The identifier is matched by shape only: four letters and five digits.
    #[test]
    fn the_identifier_is_a_shape_and_not_a_lookup() {
        assert_eq!(title_id("PPSA02664"), Some("PPSA02664"));
        assert_eq!(title_id("CUSA00001"), Some("CUSA00001"));
        // Save folders append a slot or a user after the identifier.
        assert_eq!(title_id("PPSA02664-SAVE00"), Some("PPSA02664"));

        assert_eq!(title_id("sce_sys"), None);
        assert_eq!(title_id("PPSA0266"), None, "eight is not nine");
        assert_eq!(title_id("PPSAX2664"), None, "a letter among the digits");
    }

    /// A total comes back with the count of entries that stated a size.
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
