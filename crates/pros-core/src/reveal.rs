//! Showing a folder in whatever this machine browses files with.
//!
//! # Why this exists rather than a line of code at each button
//!
//! There were two buttons saying *open folder*. Neither opened anything: they made the
//! directory if it was missing and put its path in the status line. **A control whose label
//! promises one thing and does another is worse than no control**, because the path appearing
//! somewhere reads as it having worked.
//!
//! # Why the exit code is ignored
//!
//! Windows Explorer returns a non-zero code on success. Treating that as failure would report
//! an error over a window that had just opened, which is the same lie pointing the other way.
//!
//! So what is checked is whether the program could be **started** - a missing file browser is
//! a real and reportable condition - and what it did afterwards is between it and the person
//! looking at it.

use std::path::Path;

/// What to run, and the platforms it is for.
///
/// Measured only on the one this was written on; the other two are the conventional commands
/// and are named here rather than assumed silently.
const OPENER: &str = if cfg!(target_os = "windows") {
    "explorer"
} else if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// Makes the folder if it is not there.
///
/// **Made rather than refused**, because every one of these folders is somewhere this program
/// keeps its own files, and one that does not exist yet is one nothing has been put in.
///
/// Separate from [`crate::reveal::folder`] so that it can be tested. **A test that called the
/// whole thing would open a window on whoever ran it** - which is exactly what happened, on
/// every `cargo test`, for as long as this was one function. Worse, the test removed the
/// folder immediately afterwards, so by the time the file browser opened the path was gone and
/// it fell back to showing somebody's Documents instead.
///
/// # Errors
///
/// When the folder cannot be made.
pub fn ensure(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|why| format!("{}: {why}", path.display()))
}

/// Opens a folder in the system's file browser, making it first if it is not there.
///
/// **Not unit tested, on purpose.** There is no way to check that a file browser opened
/// without opening one, and a test suite that opens windows is one nobody can run while
/// working. What can be checked is [`crate::reveal::ensure`], and that is what is checked.
///
/// # Errors
///
/// When the folder cannot be made, or the file browser cannot be started - the second usually
/// meaning there is not one, which is worth saying rather than looking like nothing happened.
pub fn folder(path: &Path) -> Result<(), String> {
    ensure(path)?;
    std::process::Command::new(OPENER)
        .arg(path)
        .spawn()
        .map(|_| ())
        // Not waited on: a file browser stays open, and waiting would freeze the window until
        // somebody closed it.
        .map_err(|why| format!("could not start {OPENER}: {why}"))
}

#[cfg(test)]
mod tests {
    use super::ensure;

    /// **The folder is made when it is not there**, which is the ordinary case for a section
    /// nobody has put anything in yet.
    ///
    /// This tests `ensure` and not `folder`, and that distinction is the whole point: a test
    /// of `folder` **opens a file browser on the machine running it**. It did, on every test
    /// run, until somebody noticed their Documents folder kept appearing.
    #[test]
    fn a_folder_that_does_not_exist_is_made() {
        let path = std::env::temp_dir().join("prosperous-reveal-test");
        let _ = std::fs::remove_dir_all(&path);
        assert!(!path.exists());

        ensure(&path).expect("it is made");
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(&path);
    }

    /// A path that cannot be a folder is reported, rather than reported as opened.
    #[test]
    fn something_that_cannot_be_a_folder_says_so() {
        let file = std::env::temp_dir().join("prosperous-reveal-file");
        std::fs::write(&file, b"not a folder").expect("writes");

        let refused = ensure(&file.join("below"));
        assert!(refused.is_err(), "a folder under a file is not possible");

        let _ = std::fs::remove_file(&file);
    }
}
