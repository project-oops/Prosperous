//! The window's sections, how the sidebar groups them, and where each keeps its things.

/// Which section of the window is showing.
///
/// Each is a sidebar entry and a view with its own toolbar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Section {
    /// What the target can do now.
    #[default]
    Check,
    /// Watching it, through a client that already speaks the protocol.
    Stream,
    /// What the target loads at startup, and the manager's settings.
    Autoload,
    /// What the target is: firmware, target, storage, what is running.
    System,
    /// Controllers presented to the target from this machine.
    Controllers,
    /// The payloads this project tracks, and what can be done with them.
    Payloads,
    /// Packages here and on the target.
    Packages,
    /// What is installed.
    Titles,
    /// Save data, and copies of it.
    Saves,
    /// Cheats, when there are any to track.
    Cheats,
    /// Anywhere on the target's storage.
    Filesystem,
    /// The system log.
    Log,
    /// An installed title, launched with its log captured.
    Probe,
    /// A command and what it printed.
    Shell,
}

/// One place a section's things might be kept.
///
/// A place carries a label saying what it is, since a fragment of its path says nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Place {
    /// Where on the target.
    pub(crate) path: &'static str,
    /// What to call it - what the place is for, not a piece of its path.
    pub(crate) label: &'static str,
    /// Why things are there, and where that is known from: documented by another tool, or
    /// seen on a target.
    pub(crate) note: &'static str,
}

impl Section {
    /// The sidebar, in groups.
    ///
    /// Grouped by kind: the target itself, syncing, diagnosing.
    pub(crate) const GROUPS: [(&'static str, &'static [Self]); 3] = [
        (
            "target",
            &[
                Self::Check,
                Self::System,
                Self::Stream,
                Self::Controllers,
                Self::Autoload,
            ],
        ),
        (
            "sync",
            &[
                Self::Payloads,
                Self::Packages,
                Self::Titles,
                Self::Saves,
                Self::Cheats,
                Self::Filesystem,
            ],
        ),
        ("diagnose", &[Self::Log, Self::Probe, Self::Shell]),
    ];

    /// What it is called in the sidebar.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Stream => "stream",
            Self::Autoload => "autoload",
            Self::System => "system",
            Self::Controllers => "controllers",
            Self::Payloads => "payloads",
            Self::Packages => "packages",
            Self::Titles => "titles",
            Self::Saves => "saves",
            Self::Cheats => "cheats",
            Self::Filesystem => "filesystem",
            Self::Log => "log",
            Self::Probe => "probe",
            Self::Shell => "shell",
        }
    }

    /// One line under the heading, saying what this section is for; the name alone does not
    /// say, for example, that payloads run through a loader while titles are booted by the
    /// system. Anything longer belongs in `docs/`.
    pub(crate) const fn explains(self) -> &'static str {
        match self {
            Self::Check => "what is answering on the target, and what is not",
            Self::Stream => "watching the target, through a client that speaks its protocol",
            Self::Autoload => "what the target loads at startup, in order",
            Self::System => "what the target says about itself",
            Self::Controllers => "a pad on this machine, presented to the target",
            Self::Payloads => {
                "ELF files. run loads one into memory now; send writes it to the target's disk"
            }
            Self::Packages => "PKG files. install holds one out for the target to fetch",
            Self::Titles => {
                "what is installed. launch asks the target to start one - it is not a file run \
                 from here"
            }
            Self::Saves => "save data on the target, and copies of it here",
            Self::Cheats => "cheat files, on either side",
            Self::Filesystem => "anywhere on the target's storage",
            Self::Log => "the target's system log, as it arrives",
            Self::Probe => {
                "launch an installed title with the log already attached, and keep what it said"
            }
            Self::Shell => "one command on the target, and what it printed",
        }
    }

    /// Whether this section already scrolls its own content.
    ///
    /// Asked because a scroll area inside another never scrolls: the outer offers unlimited
    /// height, so the inner never needs a bar. Sections that do not scroll themselves are
    /// wrapped in one.
    pub(crate) const fn scrolls_itself(self) -> bool {
        match self {
            // Two panes each, or one long output, each already in its own scroll area.
            Self::Payloads
            | Self::Packages
            | Self::Titles
            | Self::Saves
            | Self::Cheats
            | Self::Filesystem
            | Self::Log
            | Self::Probe
            | Self::Shell => true,
            Self::Check | Self::Stream | Self::Autoload | Self::System | Self::Controllers => false,
        }
    }

    /// Which kind of tracked list this section shows, if it has one.
    ///
    /// Every two-sided section has one. Saves ship empty because a save is signed for the
    /// target that wrote it; titles ship open-source engines only.
    pub(crate) const fn tracks(self) -> Option<pros_core::manifest::Tracked> {
        match self {
            Self::Payloads => Some(pros_core::manifest::Tracked::Payloads),
            Self::Packages => Some(pros_core::manifest::Tracked::Packages),
            Self::Titles => Some(pros_core::manifest::Tracked::Titles),
            Self::Cheats => Some(pros_core::manifest::Tracked::Cheats),
            Self::Saves => Some(pros_core::manifest::Tracked::Saves),
            _ => None,
        }
    }

    /// Which target service this section cannot work without.
    ///
    /// Declared, not probed: every section reads the one check the `check` section shows, so
    /// there is a single answer about whether a service is up.
    pub(crate) const fn requires(self) -> Option<&'static str> {
        match self {
            Self::Check | Self::Stream | Self::Controllers => None,

            // A probe also uses the shell and file service, but reports those failures in its
            // panel; without the log it captures nothing.
            Self::Log | Self::Probe => Some("klogsrv"),
            Self::Shell | Self::System => Some("shsrv"),
            // Autoload reads and writes its two files through the file service.
            Self::Autoload
            | Self::Payloads
            | Self::Packages
            | Self::Titles
            | Self::Saves
            | Self::Cheats
            | Self::Filesystem => Some("ftpsrv"),
        }
    }

    /// Places this section's things might be, in order of preference.
    ///
    /// Directories that belong to the system (`/user/app`, `/user/appmeta`, `/user/home`,
    /// `/data/pkg`) were confirmed on a target and are constants. Directories made by whichever
    /// payload is installed are asked of the target instead: five taken from one such tool's
    /// source were all absent on a target running a different manager. The cheat runner
    /// documents three locations it reads, so cheats have no single answer.
    pub(crate) const fn looking_for(self) -> pros_core::places::Looking {
        use pros_core::places::Looking;
        match self {
            Self::Payloads => Looking::Payloads,
            Self::Titles => Looking::Titles,
            Self::Packages => Looking::Packages,
            Self::Cheats => Looking::Cheats,
            // Saves are per-account under `/user/home` and a USB stick holds none, so the
            // chooser offers the device root. The rest have no two-pane browser.
            Self::Saves
            | Self::Filesystem
            | Self::Check
            | Self::Stream
            | Self::Autoload
            | Self::System
            | Self::Controllers
            | Self::Log
            | Self::Probe
            | Self::Shell => Looking::Anything,
        }
    }

    /// The places the target is asked about, most preferred first; empty means
    /// [`Section::there`] is the answer.
    pub(crate) const fn candidates(self) -> &'static [Place] {
        match self {
            Self::Cheats => &[
                Place {
                    path: "/data/cheatrunner/cheats",
                    label: "cheatrunner's own",
                    note: "the cheat runner's own folder, which it reads first",
                },
                Place {
                    path: "/data/etaHEN/cheats",
                    label: "etaHEN's",
                    note: "read by the cheat runner too, for cheats put there by etaHEN",
                },
                Place {
                    path: "/data/elf-arsenal/cheats",
                    label: "elf-arsenal's",
                    note: "read by the cheat runner too, for cheats put there by elf-arsenal",
                },
            ],
            // Both are made by an upload tool, not the system. Measured on a target: real
            // directories with no symbolic link between them, so two places; `/data/homebrew`
            // itself held no packages.
            Self::Packages => &[
                Place {
                    path: "/data/homebrew/pkg",
                    label: "uploads",
                    note: "where an upload tool's transfers land and stay - from that tool's \
                           own documentation, not measured here",
                },
                Place {
                    path: "/data/pkg",
                    label: "install staging",
                    note: "where the same tool puts a package on its way to being installed, \
                           and may remove it afterwards - from its documentation, not \
                           measured here. Also reachable as /user/data/pkg: measured, the \
                           same store under two names",
                },
            ],
            _ => &[],
        }
    }

    /// Where on the target this section looks, before the target has been asked.
    ///
    /// Measured on a target, and still editable because one target is one target. (D013)
    ///
    /// For a section with [`Self::candidates`], this is the first of them, shown until the
    /// target has answered.
    pub(crate) const fn there(self) -> &'static str {
        match self {
            // Each installed title is a folder named by its identifier.
            Self::Stream | Self::Titles => "/user/app",
            Self::Autoload => "/data/pldmgr",
            // Neither browses; a path has to be something.
            Self::System | Self::Controllers => "/",
            // Saves are in `/user/home/<user>/savedata_prospero`; which user is the person's
            // choice.
            Self::Saves => "/user/home",
            Self::Packages => "/data/homebrew/pkg",
            // The manager keeps a folder per payload here.
            Self::Payloads => "/data/pldmgr/payloads",
            Self::Cheats => "/data/cheatrunner/cheats",
            _ => "/data",
        }
    }
}

#[cfg(test)]
mod scrolling_tests {
    use super::Section;

    /// Every sidebar section answers whether it scrolls itself.
    #[test]
    fn every_section_answers_whether_it_scrolls() {
        for (_, sections) in Section::GROUPS {
            for section in sections {
                // The exhaustive match in `scrolls_itself` is the real check.
                let _ = section.scrolls_itself();
            }
        }
    }

    /// Two-sided sections scroll inside their panes and are not wrapped.
    #[test]
    fn the_two_sided_sections_bring_their_own() {
        for section in [
            Section::Payloads,
            Section::Packages,
            Section::Titles,
            Section::Saves,
            Section::Cheats,
            Section::Filesystem,
        ] {
            assert!(section.scrolls_itself(), "{} has panes", section.name());
        }
    }

    /// Single-panel sections such as autoload are wrapped in a scroll area.
    #[test]
    fn the_panels_that_overflowed_do_not() {
        for section in [
            Section::Autoload,
            Section::Check,
            Section::System,
            Section::Controllers,
            Section::Stream,
        ] {
            assert!(
                !section.scrolls_itself(),
                "{} needs one wrapped around it",
                section.name()
            );
        }
    }
}
