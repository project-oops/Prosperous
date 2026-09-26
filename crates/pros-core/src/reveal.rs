//! Showing a folder in the machine's file browser.
//!
//! The opener's exit code is ignored: Windows Explorer returns non-zero on success. What is
//! checked is whether the program could be started, since a missing file browser is a real,
//! reportable condition.

use std::path::Path;

/// The file browser command for this platform.
///
/// Measured on Windows only; `open` and `xdg-open` are the conventional commands elsewhere.
const OPENER: &str = if cfg!(target_os = "windows") {
    "explorer"
} else if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// Makes the folder if it is not there.
///
/// Every folder shown is one this program keeps its own files in, so a missing one is made
/// rather than refused. Separate from [`crate::reveal::folder`] so it can be tested without
/// opening a window.
///
/// # Errors
///
/// When the folder cannot be made.
pub fn ensure(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|why| format!("{}: {why}", path.display()))
}

/// Opens a folder in the system's file browser, making it first if it is not there.
///
/// Not unit tested: a test could not check this without opening a window on the machine
/// running it. [`crate::reveal::ensure`] is tested instead.
///
/// # Errors
///
/// When the folder cannot be made, or the file browser cannot be started - usually meaning
/// there is none.
pub fn folder(path: &Path) -> Result<(), String> {
    ensure(path)?;
    std::process::Command::new(OPENER)
        .arg(path)
        .spawn()
        .map(|_| ())
        // Not waited on: a file browser stays open, and waiting would freeze the window.
        .map_err(|why| format!("could not start {OPENER}: {why}"))
}

#[cfg(test)]
mod tests {
    use super::ensure;

    /// A folder that does not exist yet is made.
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
