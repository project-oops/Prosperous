//! Which build this is.
//!
//! # Why this module still exists when the implementation is shared
//!
//! So that **every front end says the same thing**. `oops_build::stamp!` expands at its call
//! site and reads *that* crate's version and commit, so calling it in `pros` and again in
//! `pros-gui` would produce two answers that only agree by coincidence. It is called here
//! instead - once, in the crate both front ends already depend on - and a window footer and a
//! `--version` cannot disagree about which binary they are.
//!
//! The commit is absent rather than wrong when there is nothing to name, and a build from a
//! modified tree says `-dirty`: see [`oops_build::Stamp::is_exact`], which is the question to
//! ask rather than whether a commit is present at all.

/// This build.
#[must_use]
pub fn stamp() -> oops_build::Stamp {
    oops_build::stamp!()
}

/// This build, in one line, for a footer or `--version`.
#[must_use]
pub fn line() -> String {
    stamp().line()
}

/// The same line, borrowed for the life of the process.
///
/// For `clap`, which builds its `--version` from a `&'static str`. Expanded *here* rather than
/// in each front end, for the reason in the module note: only this crate's `build.rs` stamps a
/// commit, so the macro has to run in this crate to see one.
#[must_use]
pub fn line_static() -> &'static str {
    oops_build::line!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stamp_names_this_crate_rather_than_the_shared_one() {
        // The trap the module note describes: expanded in the wrong crate, this reports
        // `oops-build`'s version instead of this project's, compiles, and is confidently wrong.
        assert_eq!(stamp().version, env!("CARGO_PKG_VERSION"));
        assert!(line().starts_with('v'));
    }
}
