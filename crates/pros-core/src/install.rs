//! Installing a package on the target, through the shell's `pkg_install URL` builtin.
//!
//! The argument must be an http(s) URL served from this machine. Measured on a target with a
//! real package (valid `CNT` magic) in `/data/pkg`: a bare path and a `file://` URL both
//! answer `content_id = [] content_platform = [0]`, the same as a missing file, while an
//! `http://` URL answers with the package's content identifier. An empty `content_id` means
//! the package was never read; a populated one means it was handed to the installer, under
//! the identifier the target will list it by. Whether the install then finishes is visible
//! only on the target's own screen.

/// What the target said about an install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// It read the package and handed it to the installer.
    ///
    /// Carries the content identifier the target reported. Not the same as installed: the
    /// installer finishing is visible only on the target's own screen.
    Accepted(String),
    /// It could not read the package.
    ///
    /// Measured as an empty `content_id`, for a missing file and for any local path form.
    CouldNotRead,
    /// It said nothing before the shell went quiet.
    ///
    /// Not success: a network fetch can outlast the shell's window, so silence means still
    /// going or never started, which this side cannot tell apart.
    Silent,
    /// It said something this does not recognise.
    ///
    /// The words are carried for somebody to read rather than guessed at.
    Unclear(String),
}

impl Said {
    /// Whether this is known to have gone wrong.
    ///
    /// `false` for [`Said::Unclear`], which is not the same as *it worked*.
    #[must_use]
    pub const fn is_a_known_failure(&self) -> bool {
        matches!(self, Self::CouldNotRead | Self::Silent)
    }

    /// Whether the target took it.
    #[must_use]
    pub const fn was_accepted(&self) -> bool {
        matches!(self, Self::Accepted(_))
    }

    /// How to put it to somebody.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Accepted(id) => format!(
                "the target read it and handed it to the installer as {id}. Whether the \
                 install finishes is on the target's own screen"
            ),
            Self::CouldNotRead => {
                "the target could not read the package. It needs an http url it can fetch - a \
                 path on its own disk is not one, measured"
                    .to_owned()
            }
            Self::Silent => "the target said nothing before the shell went quiet. It may still be \
                 installing: look at the target itself rather than trusting this"
                .to_owned(),
            Self::Unclear(said) => format!(
                "the target said this, and nothing here knows what a successful install \
                 looks like - check the target:\n{said}"
            ),
        }
    }
}

/// The command that installs a package from a url the target can fetch.
///
/// Not quoted: the shell splits on spaces and has no quoting, so [`is_a_url`] refuses a url
/// with a space rather than sending half of it.
#[must_use]
pub fn command(url: &str) -> String {
    format!("pkg_install {url}")
}

/// Whether this is something the target could fetch.
///
/// A path on the target's disk is not: `pkg_install` reads only http(s) URLs (measured, see
/// the module note).
#[must_use]
pub fn is_a_url(url: &str) -> bool {
    let url = url.trim();
    !url.contains(char::is_whitespace)
        && (url.starts_with("http://") || url.starts_with("https://"))
}

/// Whether a name is one of the things this can install.
#[must_use]
pub fn is_a_package(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".pkg")
}

/// Reads what the target said back.
#[must_use]
pub fn read(said: &str) -> Said {
    let trimmed = said.trim();
    if trimmed.is_empty() || trimmed.starts_with("no output") {
        return Said::Silent;
    }
    // Measured: empty is a package it never read, and populated is one it took.
    if trimmed.contains("content_id = []") {
        return Said::CouldNotRead;
    }
    if let Some(id) = between(trimmed, "content_id = [", "]") {
        return Said::Accepted(id);
    }
    Said::Unclear(trimmed.to_owned())
}

/// What sits between two markers, when both are there.
fn between(text: &str, open: &str, close: &str) -> Option<String> {
    let from = text.find(open)? + open.len();
    let rest = text.get(from..)?;
    let to = rest.find(close)?;
    let found = rest.get(..to)?.trim();
    (!found.is_empty()).then(|| found.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Said, command, is_a_package, is_a_url, read};

    /// The command is the one the shell prints usage for.
    #[test]
    fn the_command_is_what_the_shell_documents() {
        assert_eq!(
            command("http://192.0.2.1:8099/thing.pkg"),
            "pkg_install http://192.0.2.1:8099/thing.pkg"
        );
    }

    /// Paths and `file://` URLs are not fetchable, and neither is a URL with a space.
    #[test]
    fn a_path_on_the_targets_own_disk_is_not_something_it_can_fetch() {
        assert!(is_a_url("http://192.0.2.1:8099/thing.pkg"));
        assert!(is_a_url("https://example.com/thing.pkg"));
        assert!(!is_a_url("/data/pkg/thing.pkg"));
        assert!(!is_a_url("file:///data/pkg/thing.pkg"));
        assert!(
            !is_a_url("http://192.0.2.1/my thing.pkg"),
            "the shell splits on spaces"
        );
    }

    /// A populated content identifier is accepted, not reported as a finished install.
    #[test]
    fn a_package_the_target_took_is_reported_with_its_identifier() {
        let said = read(concat!(
            "IpcFacade::appInstallByPackage pkg_info ",
            "content_id = [IV0002-ITEM00001_00-STOREUPD00000000] ",
            "content_type = [0] content_platform = [1]"
        ));
        assert_eq!(
            said,
            Said::Accepted("IV0002-ITEM00001_00-STOREUPD00000000".to_owned())
        );
        assert!(said.was_accepted());
        assert!(!said.is_a_known_failure());
        assert!(
            said.describe().contains("target's own screen"),
            "it should not claim the install finished: {}",
            said.describe()
        );
    }

    /// Only `.pkg` files, in any case, are offered for installing.
    #[test]
    fn only_a_package_is_offered_for_installing() {
        assert!(is_a_package("thing.PKG"), "case does not matter");
        assert!(!is_a_package("elfldr.elf"));
        assert!(!is_a_package("thing"));
    }

    /// An empty content identifier is a known failure.
    #[test]
    fn a_package_the_target_cannot_read_is_a_known_failure() {
        let said = read(
            "IpcFacade::appInstallByPackage pkg_info content_id = [] content_type = [0] \
             content_platform = [0]",
        );
        assert_eq!(said, Said::CouldNotRead);
        assert!(said.is_a_known_failure());
    }

    /// Silence is not taken for success.
    #[test]
    fn saying_nothing_is_not_taken_for_success() {
        assert_eq!(read(""), Said::Silent);
        assert_eq!(read("no output - is the shell loaded?"), Said::Silent);
        assert!(read("").is_a_known_failure());
    }

    /// An unrecognised answer is unclear, neither success nor a known failure.
    #[test]
    fn an_unrecognised_answer_is_not_promoted_to_success() {
        let said = read("something the shell printed that mentions no content at all");
        assert!(matches!(said, Said::Unclear(_)));
        assert!(
            !said.is_a_known_failure(),
            "it is not a known failure either - it is unknown"
        );
        assert!(
            said.describe().contains("check the target"),
            "the wording should send somebody to look: {}",
            said.describe()
        );
    }
}
