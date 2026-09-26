//! Which build this is.
//!
//! `oops_build::stamp!` reads the version and commit of the crate it expands in, so it is
//! called once here, in the crate every front end depends on, and they all report the same
//! build. A build from a modified tree says `-dirty`; see [`oops_build::Stamp::is_exact`].

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
/// For `clap`, which builds its `--version` from a `&'static str`. Expanded here because only
/// this crate's `build.rs` stamps a commit.
#[must_use]
pub fn line_static() -> &'static str {
    oops_build::line!()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stamp reports this crate's version, not `oops-build`'s.
    #[test]
    fn the_stamp_names_this_crate_rather_than_the_shared_one() {
        assert_eq!(stamp().version, env!("CARGO_PKG_VERSION"));
        assert!(line().starts_with('v'));
    }
}
