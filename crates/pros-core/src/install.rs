//! Installing a package on the target, through the shell.
//!
//! # How this turned out to work, and how long that took to find
//!
//! A note in this project's own decisions once said package installation was unmeasured:
//! nothing answered on 8080, 9090 or 12800, so it was recorded as an open question rather
//! than guessed at. That was right, and it was also looking in the wrong place.
//!
//! **The shell has a builtin.** `pkg_install URL`, alongside `launch`, `hbldr` and `notify`.
//! No service, no port - a command, on the shell that was already there.
//!
//! # It takes a URL, and it means it
//!
//! `pkg_install` prints `Usage: pkg_install URL`, and **that is not a loose way of saying
//! path**. Measured against a target, with a real package present on its own disk:
//!
//! ```text
//! pkg_install /data/pkg/thing.pkg          content_id = [] content_platform = [0]
//! pkg_install file:///data/pkg/thing.pkg   content_id = [] content_platform = [0]
//! pkg_install http://192.168.1.100/thing.pkg
//!     content_id = [IV0002-ITEM00001_00-STOREUPD00000000] content_platform = [1]
//! ```
//!
//! # The wrong conclusion this replaces, and how it was reached
//!
//! An earlier version of this note said a bare path and a `file://` one *reach the same code
//! inside it* - concluded from giving it a missing file and getting an identical complaint
//! from each.
//!
//! They were identical because **both fail**, not because both work. Two inputs producing
//! indistinguishable output, read as agreement rather than as a pair of failures: this
//! project's own defect, in the reasoning about it rather than in the code.
//!
//! What settled it was giving it a package that was definitely there - valid `CNT` magic,
//! sitting in `/data/pkg` - and watching it produce the same empty answer as a file that did
//! not exist.
//!
//! So the package has to be **served over HTTP from this machine**. That is the listening
//! socket this module previously said it would not open; it is not optional, and a note
//! saying otherwise was worth less than the measurement that disproved it.
//!
//! # What success looks like, now that one has been watched
//!
//! `content_id` is the whole of it. Empty means the package was never read; populated means it
//! was read and handed to the installer, and the identifier is the one the target will list it
//! under.
//!
//! This still does not claim the install **finished**. Handing a package to the installer and
//! the installer completing are different events, and only the first is visible from here - so
//! what comes back says the target accepted it, with the identifier, and leaves the rest to
//! the target's own screen.

/// What the target said about an install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// It read the package and handed it to the installer.
    ///
    /// Carries the content identifier the target reported. **Not the same as installed**: what
    /// is visible from here is the handover, and the installer finishing is an event on the
    /// target's own screen.
    Accepted(String),
    /// It could not read the package.
    ///
    /// Measured: an empty `content_id`. Produced by a file that is not there, and equally by
    /// one that is - the local path forms fail this way, which is how they were found out.
    CouldNotRead,
    /// It said nothing before the shell went quiet.
    ///
    /// **Not success.** A fetch over the network can outlast the window the shell is given,
    /// so silence here means *still going or never started*, and the two are not
    /// distinguishable from this side.
    Silent,
    /// It said something this does not recognise.
    ///
    /// **The honest default.** No successful install has been observed by this project, so
    /// there is no shape to match against, and claiming one would be inventing a measurement.
    /// The words are carried so somebody can read them.
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
/// **Not quoted or escaped**, because the shell splits its line on spaces and offers no
/// quoting - so a url with a space in it cannot be passed at all, and [`is_a_url`] refuses one
/// rather than sending half of it.
#[must_use]
pub fn command(url: &str) -> String {
    format!("pkg_install {url}")
}

/// Whether this is something the target could fetch.
///
/// **A path is not**, however much it looks like one this program could open. That was
/// measured: a real package sitting in the target's own `/data/pkg` produced the same empty
/// answer as a file that was not there.
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

    /// **A path is not a url, however much it looks like one.**
    ///
    /// The measurement that settled this: a real package in the target's own `/data/pkg`
    /// produced the same empty answer as a file that was not there.
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

    /// **A populated content identifier is the target taking it**, and it is reported as
    /// exactly that rather than as an install that finished.
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

    /// **A path with a space is refused rather than sent.**
    ///
    /// The shell splits on spaces and cannot be told otherwise, so sending one would install
    /// whatever the first word named. Refusing is the only honest option.
    /// Only packages, because that is what the command takes.
    #[test]
    fn only_a_package_is_offered_for_installing() {
        assert!(is_a_package("thing.PKG"), "case does not matter");
        assert!(!is_a_package("elfldr.elf"));
        assert!(!is_a_package("thing"));
    }

    /// **The measured failure is recognised**, having been produced deliberately with a file
    /// that was not there.
    #[test]
    fn a_package_the_target_cannot_read_is_a_known_failure() {
        let said = read(
            "IpcFacade::appInstallByPackage pkg_info content_id = [] content_type = [0] \
             content_platform = [0]",
        );
        assert_eq!(said, Said::CouldNotRead);
        assert!(said.is_a_known_failure());
    }

    /// **Silence is not success.** A fetch can outlast the shell's window, so nothing said
    /// means *still going or never started* - two states this side cannot tell apart.
    #[test]
    fn saying_nothing_is_not_taken_for_success() {
        assert_eq!(read(""), Said::Silent);
        assert_eq!(read("no output - is the shell loaded?"), Said::Silent);
        assert!(read("").is_a_known_failure());
    }

    /// **Anything else is unclear, and is not reported as success.**
    ///
    /// No successful install has been watched by this project - doing so means installing
    /// something on somebody's target - so there is no shape to match, and a version that
    /// called any other output *installed* would say the same thing when it was wrong.
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
