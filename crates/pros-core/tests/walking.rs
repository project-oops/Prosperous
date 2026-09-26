//! Walking a local folder before any of it is sent.
//!
//! `transfer::contents` decides the file list a restore hands to its session, so a file it
//! misses is a file that never goes back. These tests cover it without a target.

use pros_core::transfer::contents;
use std::path::{Path, PathBuf};

/// A directory this test owns, cleared first so a previous run cannot decide the answer.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pros-core-walking-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// Writes a file, making the directories above it.
fn file_at(root: &Path, relative: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("a parent directory");
    }
    std::fs::write(&path, b"contents").expect("a file");
}

/// Renders the result with forward slashes, so an assertion reads the same on either
/// platform.
fn listed(root: &Path) -> Vec<String> {
    contents(root)
        .expect("the folder can be read")
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect()
}

/// Every file is listed by a path relative to the folder, never an absolute one.
#[test]
fn every_file_is_listed_relative_to_the_folder_it_was_found_in() {
    let root = scratch("flat");
    file_at(&root, "save.dat");
    file_at(&root, "icon.png");

    let found = listed(&root);
    assert_eq!(found, ["icon.png", "save.dat"]);
    assert!(
        found.iter().all(|p| !Path::new(p).is_absolute()),
        "an absolute path would carry this machine's directory onto the target"
    );
}

/// A nested folder is walked, and its files keep the path that puts them back.
#[test]
fn a_nested_folder_is_walked_and_keeps_its_shape() {
    let root = scratch("nested");
    file_at(&root, "top.dat");
    file_at(&root, "one/inner.dat");
    file_at(&root, "one/two/deeper.dat");

    assert_eq!(
        listed(&root),
        ["one/inner.dat", "one/two/deeper.dat", "top.dat"]
    );
}

/// The list is sorted, independent of the filesystem's order.
#[test]
fn the_list_is_ordered_the_same_way_every_time() {
    let root = scratch("ordering");
    for name in ["zulu.dat", "alpha.dat", "mike.dat", "bravo/charlie.dat"] {
        file_at(&root, name);
    }

    let once = listed(&root);
    let twice = listed(&root);
    assert_eq!(once, twice, "two walks of one folder must agree");

    let mut sorted = once.clone();
    sorted.sort();
    assert_eq!(once, sorted, "and the order is not the filesystem's");
}

/// Directories are walked, not listed, and an empty one contributes nothing.
#[test]
fn directories_are_walked_rather_than_listed() {
    let root = scratch("dirs");
    file_at(&root, "folder/inside.dat");
    std::fs::create_dir_all(root.join("empty")).expect("an empty directory");
    std::fs::create_dir_all(root.join("folder/also-empty")).expect("another");

    let found = listed(&root);
    assert_eq!(
        found,
        ["folder/inside.dat"],
        "only files, and an empty directory contributes nothing"
    );
}

/// An empty folder is an empty list rather than a failure.
#[test]
fn an_empty_folder_is_an_empty_list() {
    let root = scratch("empty");
    assert_eq!(
        contents(&root).expect("an empty folder can be read"),
        Vec::<PathBuf>::new()
    );
}

/// A folder that is not there is an error, not an empty list.
#[test]
fn a_folder_that_cannot_be_read_is_an_error_and_not_an_empty_list() {
    let missing = scratch("missing").join("not-here");
    let error = contents(&missing).expect_err("there is nothing to walk");
    assert!(
        !error.is_empty(),
        "the failure has to say something a person can act on"
    );
}

/// A tree deeper than the bound stops there, still listing what is within it.
#[test]
fn a_tree_deeper_than_the_bound_stops_instead_of_running() {
    let root = scratch("deep");
    file_at(&root, "shallow.dat");

    // Well past the twelve levels the walk allows.
    let mut deep = String::new();
    for level in 0..30 {
        use std::fmt::Write as _;
        write!(deep, "level{level}/").expect("writing to a String cannot fail");
    }
    file_at(&root, &format!("{deep}buried.dat"));

    let found = listed(&root);
    assert!(
        found.contains(&"shallow.dat".to_owned()),
        "what is within the bound is still reported"
    );
    assert!(
        !found.iter().any(|p| p.contains("buried.dat")),
        "and what is past it is not walked to: {found:?}"
    );
}

/// A file at exactly the deepest allowed level is still found.
#[test]
fn a_file_at_the_deepest_allowed_level_is_still_found() {
    let root = scratch("boundary");

    // Twelve directories deep, which is the stated limit.
    let mut path = String::new();
    for level in 0..12 {
        use std::fmt::Write as _;
        write!(path, "d{level}/").expect("writing to a String cannot fail");
    }
    file_at(&root, &format!("{path}edge.dat"));

    let found = listed(&root);
    assert!(
        found.iter().any(|p| p.ends_with("edge.dat")),
        "a file at the limit is inside it: {found:?}"
    );
}
