//! Walking a local folder before any of it is sent.
//!
//! # Why this is worth its own file
//!
//! `transfer::contents` carries a doc comment saying it is *"separated from the sending so
//! it can be tested, and so a caller can show what is about to go before any of it does"* -
//! and then nothing tested it. A seam introduced for testability and left untested is the
//! cost of the seam without the benefit.
//!
//! It is also the half of a restore that decides what a restore *is*. `upload` takes a live
//! session and cannot be exercised without a target; this decides the file list that session
//! is then handed, so a folder missed here is a file that never goes back - and a restore
//! that quietly missed one is worse than none, because it will be trusted at the moment it
//! matters.

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

/// Every file is listed, by a path relative to the folder rather than an absolute one.
///
/// Relative because the caller is about to join each onto a path on the target. An absolute
/// one would carry this machine's own directory across, and the restore would land somewhere
/// nobody asked for.
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

/// The list is sorted, so two runs of a restore describe themselves the same way.
///
/// A directory read is in whatever order the filesystem gives, which differs between
/// machines and between runs on one. A caller showing "what is about to go" would otherwise
/// shuffle its own list every time it was opened.
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

/// Directories are walked, not listed.
///
/// A restore makes directories on the way down as it needs them, so a directory in the file
/// list would be sent as though it were a file - and an empty one carries nothing worth
/// sending anyway.
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

/// A folder with nothing in it is an empty list rather than a failure.
///
/// The ordinary case for a title that has been installed and not yet played.
#[test]
fn an_empty_folder_is_an_empty_list() {
    let root = scratch("empty");
    assert_eq!(
        contents(&root).expect("an empty folder can be read"),
        Vec::<PathBuf>::new()
    );
}

/// A folder that is not there is reported rather than treated as empty.
///
/// **The distinction a restore rests on.** "Nothing to send" and "I could not look" produce
/// the same empty list, and one of them means the restore did nothing while reporting
/// success.
#[test]
fn a_folder_that_cannot_be_read_is_an_error_and_not_an_empty_list() {
    let missing = scratch("missing").join("not-here");
    let error = contents(&missing).expect_err("there is nothing to walk");
    assert!(
        !error.is_empty(),
        "the failure has to say something a person can act on"
    );
}

/// A folder nested past the bound stops rather than running.
///
/// **A bound rather than a belief.** Save folders are shallow; something that is not one
/// should stop. The files above the limit are still listed, so what is returned stays a true
/// statement about part of the tree rather than becoming a failure about all of it.
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

/// A file at exactly the deepest level allowed is still found.
///
/// The boundary the bound is written at. Off by one here either loses a legitimate file or
/// walks one level further than the rule says, and neither shows up on a shallow folder.
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
