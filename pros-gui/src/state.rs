//! What the window is showing, and the rules about changing it.
//!
//! Every rule rather than pixel lives here, where tests reach it; `app.rs` only draws.
//!
//! 1. One job at a time, so two answers never interleave.
//! 2. A failed job clears what it would have replaced, so nothing stale is shown as current.
//! 3. Waiting is visible and timed, so working and hung look different.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pros_core::check::Report;
use pros_core::target::Target;

/// Something asked of a target, to be done away from the drawing thread.
#[derive(Debug, Clone)]
pub(crate) enum Job {
    /// Ask what the target can do now.
    Check(Target),
    /// Run one command.
    Shell(Target, String),
    /// Fetch a file to a local path.
    Pull(Target, String, PathBuf),
    /// List a directory on the target and read it as a library.
    Browse(Target, String),
    /// Copy a local file onto the target.
    Push(Target, PathBuf, String),
    /// Copy a whole folder off the target.
    Backup(Target, String, PathBuf),
    /// Put a whole folder back onto the target.
    ///
    /// The flag overrides the refusal. A save for an account other than the one that wrote it
    /// needs re-signing, so by default it is refused.
    Restore(Target, PathBuf, String, bool),
    /// Send a staged payload, named as the manifest names it.
    ///
    /// This runs it and writes nothing; [`Job::Install`] puts a file on the disk.
    Send(Target, String, PathBuf),
    /// Put a staged payload on the target's disk, where the manager can resolve it.
    ///
    /// Unlike [`Job::Push`], this lays the payload out as the manager expects: its own folder,
    /// the ELF inside, and the `.json` sidecar that is the only record of its build.
    Install(Target, Box<pros_core::manifest::Payload>, PathBuf, String),
    /// Ask the target what each of these titles is called.
    Names(Target, Vec<String>),
    /// List what is installed, by name - the probe screen's choice of titles.
    Titles(Target),
    /// Ask the target where its saves are.
    FindSaves(Target),
    /// Ask the target which of these directories it actually has.
    Locate(Target, &'static [Place]),
    /// Ask the target to start a title it has installed.
    Launch(Target, String),
    /// Run an ELF the target already has, by its path there.
    RunThere(Target, String),
    /// Read the payload manager's settings.
    ReadAutoload(Target),
    /// Read one startup list, by which of the known ones it is.
    ///
    /// Separate from the settings: there are several lists, audited by different rules.
    ReadList(Target, pros_core::chain::Held),
    /// Ask the target what it is.
    ReadSystem(Target),
    /// Restart the user interface to clear a softlock.
    ///
    /// Ends `SceShellUI`, which the system respawns, then reads the target again.
    RestartUi(Target),
    /// Close a title, ending every process it owns, then read the target again.
    CloseTitle(Target, String),
    /// End one process by its pid, then read the target again.
    ///
    /// The by-pid form of [`Job::CloseTitle`], the same primitive `pros kill` uses; a stopped
    /// process is woken first.
    EndProcess(Target, String),
    /// Find every payload file the manager holds, looking inside its folders.
    FindPayloads(Target, String),
    /// Remove things from the target, each with whether it is a directory.
    ///
    /// The whole confirmed list is one job. The directory flag comes from the listing on
    /// screen, because files and directories are removed by different commands.
    DeleteThere(Target, Vec<(String, bool)>),
    /// Remove files from this machine.
    DeleteHere(Vec<PathBuf>),
    /// Hold a package out for the target to fetch and register.
    ///
    /// Confirmed first. The answer combines what the target said with whether it fetched the
    /// file at all.
    InstallPackage(Target, PathBuf),
    /// Replace the payload manager's settings with this text.
    ///
    /// Carries the whole reviewed file rather than an edit, so what is written is what was
    /// reviewed.
    WriteAutoload(Target, String, String),
    /// Turn autoload on in the manager's settings, keeping every other setting.
    ///
    /// Reads the current settings, turns `AUTOLOAD_ENABLED` on and writes the result, so a
    /// deployed list is read at startup. Deploying a manager chain queues it.
    EnableAutoload(Target),
    /// Read the files a chain should carry off a target, to write a chain down with them.
    ///
    /// The files half of `export chain`; the payload order comes from the boot list on screen.
    /// The paths are declared in the chain data ([`pros_core::chain::capture_spots`]); a missing
    /// one is noted, not an error.
    CaptureConfig(Target),
    /// Put a file a chain carries back on the target, verbatim.
    ///
    /// The deploy half of `export chain`: the captured bytes go back unread, each file to its
    /// own path.
    PlaceFile(Target, String, String),
    /// Point a description at what its project has released now.
    ///
    /// Downloads the new file to learn its digest and returns a description for review; the
    /// list is written only once that is accepted.
    Relist(Box<pros_core::manifest::Payload>),
    /// Fetch a described payload and keep it, if it is the one described.
    ///
    /// No target involved, but a network round trip all the same.
    Fetch(Box<pros_core::manifest::Payload>, Option<PathBuf>),
}

impl Job {
    /// What to say while this is running.
    ///
    /// Specific ("checking", with the path when there is one), never "working".
    #[must_use]
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Check(target) => format!("checking {}", target.name),
            Self::Shell(_, command) => format!("running {command}"),
            Self::Pull(_, path, _) => format!("fetching {path}"),
            Self::Browse(_, path) => format!("opening {path}"),
            Self::Push(_, _, to) => format!("copying to {to}"),
            Self::Backup(_, from, _) => format!("backing up {from}"),
            Self::Restore(_, _, to, _) => format!("restoring to {to}"),
            Self::Send(_, name, _) => format!("sending {name}"),
            Self::Install(_, payload, _, to) => format!("putting {} in {to}", payload.name),
            Self::Fetch(payload, _) => format!("fetching {}", payload.name),
            Self::Relist(payload) => {
                format!("asking what {} has released, and checking it", payload.name)
            }
            Self::Names(_, ids) => format!("reading {} title names", ids.len()),
            Self::Titles(_) => "listing installed titles".to_owned(),
            Self::FindSaves(_) => "looking for saves".to_owned(),
            Self::Locate(_, where_) => format!("looking in {} places", where_.len()),
            Self::Launch(_, id) => format!("starting {id}"),
            Self::RunThere(_, path) => format!("running {path} on the target"),
            Self::ReadAutoload(_) => "reading the manager's settings".to_owned(),
            Self::ReadList(_, held) => format!("reading {}", held.path),
            Self::ReadSystem(_) => "asking the target what it is".to_owned(),
            Self::RestartUi(_) => "restarting the user interface".to_owned(),
            Self::CloseTitle(_, id) => format!("closing {id}"),
            Self::EndProcess(_, pid) => format!("ending pid {pid}"),
            Self::FindPayloads(..) => "looking for payloads".to_owned(),
            Self::DeleteThere(_, what) => format!("deleting {} from the target", what.len()),
            Self::DeleteHere(what) => format!("deleting {} from this machine", what.len()),
            Self::InstallPackage(_, path) => format!("installing {}", path.display()),
            Self::WriteAutoload(_, path, _) => format!("writing {path}"),
            Self::EnableAutoload(_) => "turning autoload on".to_owned(),
            Self::CaptureConfig(_) => "reading the files the chain carries".to_owned(),
            Self::PlaceFile(_, path, _) => format!("restoring {path}"),
        }
    }

    /// What finishing this may have made untrue elsewhere.
    ///
    /// Empty for anything that only reads; a check is the reading, so it would loop otherwise.
    pub(crate) const fn disturbs(&self) -> &'static [Disturbs] {
        match self {
            // These change what is running, so a service may answer differently now.
            Self::Send(..)
            | Self::Launch(..)
            | Self::RunThere(..)
            | Self::RestartUi(..)
            | Self::CloseTitle(..)
            | Self::EndProcess(..) => &[Disturbs::Report],
            Self::Fetch(..)
            | Self::Relist(..)
            | Self::Pull(..)
            | Self::Backup(..)
            | Self::DeleteHere(..) => &[Disturbs::Here],
            Self::Push(..)
            | Self::Install(..)
            | Self::Restore(..)
            | Self::InstallPackage(..)
            | Self::DeleteThere(..) => &[Disturbs::There],
            // A placed chain file sits beside the settings, so that screen re-reads too.
            Self::WriteAutoload(..) | Self::EnableAutoload(..) | Self::PlaceFile(..) => {
                &[Disturbs::Autoload]
            }
            // A command can change anything; the cost of assuming it did is one re-read.
            Self::Shell(..) => &[Disturbs::Report, Disturbs::There],
            Self::Check(..)
            | Self::Browse(..)
            | Self::Names(..)
            | Self::Titles(..)
            | Self::FindSaves(..)
            | Self::Locate(..)
            | Self::ReadList(..)
            | Self::ReadAutoload(..)
            | Self::ReadSystem(..)
            | Self::CaptureConfig(..)
            | Self::FindPayloads(..) => &[],
        }
    }

    /// Which screens are waiting on this, if any.
    ///
    /// Only screens that cannot be drawn without the answer, so a usable panel is never held
    /// shut.
    pub(crate) const fn fills(&self) -> &'static [Section] {
        match self {
            // The check also reads the startup list the autoload screen audits.
            Self::Check(_) => &[Section::Check, Section::Autoload],
            Self::ReadAutoload(_) | Self::ReadList(..) => &[Section::Autoload],
            Self::ReadSystem(_)
            | Self::RestartUi(_)
            | Self::CloseTitle(..)
            | Self::EndProcess(..) => &[Section::System],
            Self::FindPayloads(..) => &[Section::Payloads],
            Self::FindSaves(_) => &[Section::Saves],
            Self::Titles(_) => &[Section::Probe],
            // One listing, five views over it.
            Self::Browse(..) => &[
                Section::Filesystem,
                Section::Titles,
                Section::Saves,
                Section::Cheats,
                Section::Packages,
            ],
            Self::Shell(..)
            | Self::Pull(..)
            | Self::Push(..)
            | Self::Install(..)
            | Self::Backup(..)
            | Self::Restore(..)
            | Self::Send(..)
            | Self::Names(..)
            | Self::Locate(..)
            | Self::Launch(..)
            | Self::RunThere(..)
            | Self::DeleteThere(..)
            | Self::DeleteHere(..)
            | Self::InstallPackage(..)
            | Self::WriteAutoload(..)
            | Self::EnableAutoload(..)
            | Self::CaptureConfig(..)
            | Self::PlaceFile(..)
            | Self::Relist(..)
            | Self::Fetch(..) => &[],
        }
    }

    /// Which part of the window this will replace when it finishes.
    ///
    /// That part is cleared if the job fails (rule 2).
    const fn replaces(&self) -> Panel {
        match self {
            Self::Check(_) => Panel::Report,
            Self::Shell(..) => Panel::Said,
            Self::Pull(..)
            | Self::ReadList(..)
            | Self::Send(..)
            | Self::Push(..)
            | Self::Install(..)
            | Self::Backup(..)
            | Self::Restore(..)
            | Self::Relist(..)
            | Self::Fetch(..)
            | Self::Names(..)
            | Self::Titles(..)
            | Self::FindSaves(..)
            | Self::Locate(..)
            | Self::ReadAutoload(..)
            | Self::ReadSystem(..)
            | Self::RestartUi(..)
            | Self::CloseTitle(..)
            | Self::EndProcess(..)
            | Self::Launch(..)
            | Self::RunThere(..)
            | Self::InstallPackage(..)
            | Self::FindPayloads(..)
            | Self::DeleteThere(..)
            | Self::DeleteHere(..)
            | Self::WriteAutoload(..)
            | Self::EnableAutoload(..)
            | Self::CaptureConfig(..)
            | Self::PlaceFile(..) => Panel::Nothing,
            Self::Browse(..) => Panel::Library,
        }
    }
}

/// Something a job may have changed, which whatever shows it must therefore read again.
///
/// Declared by every job in [`Job::disturbs`], so the compiler demands an answer for each new
/// job and no screen is left showing a measurement that has stopped being true.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disturbs {
    /// What the target can do now. Running a payload makes a service answer.
    Report,
    /// The folder on this machine.
    Here,
    /// The directory being browsed on the target.
    There,
    /// The startup list and the manager's settings.
    Autoload,
}

/// A part of the window that holds an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Report,
    Said,
    /// The library listing.
    Library,
    /// An action with no panel of its own - it either happened or it did not.
    Nothing,
}

/// What a job produced.
#[derive(Debug, Clone)]
pub(crate) enum Done {
    /// A check finished, with the boot list if it could be read.
    Checked(Box<Report>, Option<pros_core::chain::Chain>),
    /// A command answered.
    Said(String),
    /// A library listing arrived.
    Browsed(Vec<pros_core::library::Item>),
    /// The target said where its saves are, or that it cannot say.
    FoundSaves(pros_core::saves::Found),
    /// The target said which of several directories it has.
    Located(pros_core::locate::Where),
    /// A title was asked to start.
    Launched(pros_core::launch::Said),
    /// A payload already on the target was asked to run.
    RanThere(pros_core::hbldr::Said),
    /// One startup list, read.
    List(Box<pros_core::boot::Boot>),
    /// The manager's settings and its startup list, as read.
    Autoload(
        Box<pros_core::autoload::Settings>,
        Box<pros_core::boot::Boot>,
    ),
    /// The files a chain should carry, read off a target, with a note for any that could not be.
    ///
    /// The notes say what could not be carried, so an export never drops a file silently.
    Captured(Vec<pros_core::recovery::baseline::Captured>, Vec<String>),
    /// What the target is.
    System(Box<pros_core::system::Report>),
    /// A process was signalled and the target read again afterwards.
    Signalled {
        /// What happened, in a line.
        note: String,
        /// The target as it is after.
        report: Box<pros_core::system::Report>,
    },
    /// An install ran, and this is what the target said about it.
    Installed(pros_core::install::Said),
    /// Titles said what they are called.
    ///
    /// A title that did not answer is absent rather than present with an empty name.
    Named(Vec<pros_core::titles::Metadata>),
    /// What is installed, by name, for the probe screen to choose from.
    Titles(Vec<pros_core::titles::Metadata>),
    /// A folder was copied, in one direction or the other.
    Copied(Box<pros_core::transfer::Summary>, String),
    /// Every payload file the manager holds, once looked for.
    Payloads(Vec<pros_core::payloads::There>),
    /// A file was written here.
    Pulled {
        /// Where it went.
        into: PathBuf,
        /// How big it was.
        bytes: usize,
    },
    /// Something was downloaded, checked, and kept here.
    ///
    /// Distinct from [`Done::Said`] because the folder listing must be read again.
    Fetched(String, PathBuf),
    /// A description now points at what its project has released.
    ///
    /// Carries the whole new description, not a diff, because the list stores whole records.
    Relisted(
        Box<pros_core::manifest::Payload>,
        Box<pros_core::sources::Upstream>,
    ),
    /// It was not attempted, because it would not have worked.
    ///
    /// Not a [`Done::Failed`]: decided before anything moved, carrying what would be needed.
    Refused(pros_core::origin::Needs),
    /// A staged title was refused: an inert destination path or an incompatible title prefix.
    GuardRefused(pros_core::guard::Refusal),
    /// It did not work, in the target's words or the system's.
    Failed(String),
}

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

/// A chain read off a target, being written down as a preset.
///
/// The name is the one thing a person types, so it is held apart and applied on agreement;
/// the panel can refuse a name with nothing to undo.
#[derive(Debug, Clone)]
pub(crate) struct Exporting {
    /// What to call it. One word, because a preset name goes in a whitespace-delimited file.
    pub(crate) name: String,
    /// The preset as measured, with the name not yet applied.
    pub(crate) preset: pros_core::recovery::baseline::Preset,
    /// What the export could not know, in its own words.
    pub(crate) notes: Vec<String>,
    /// Whether the files the chain should carry are still being read off the target.
    ///
    /// The panel opens with the list and fills the files in when the read lands; while this is
    /// set it will not write, so a chain never carries the list without its files.
    pub(crate) capturing: bool,
    /// How many disabled lines were left out.
    pub(crate) disabled: usize,
    /// Where it would be written.
    pub(crate) into: String,
    /// The presets that already exist, so a name that replaces one says so.
    pub(crate) taken: Vec<String>,
}

/// A plan, and the finding it answers, waiting for somebody to agree to it.
#[derive(Debug, Clone)]
pub(crate) struct Pending {
    /// Which finding this answers, so the result can be checked against it.
    pub(crate) id: String,
    /// What the finding was called, for the panel's heading.
    pub(crate) label: String,
    /// The steps.
    pub(crate) plan: pros_core::doctor::Plan,
}

/// What is currently being waited for.
#[derive(Debug, Clone)]
pub(crate) struct Waiting {
    /// What was asked.
    pub(crate) job: Job,
    /// When it was asked.
    pub(crate) since: Instant,
}

impl Waiting {
    /// How long this has been running.
    #[must_use]
    pub(crate) fn elapsed(&self) -> Duration {
        self.since.elapsed()
    }
}

/// The probe screen: what it can launch, and what the last run captured.
///
/// Only what is drawn; the run's thread lives beside the worker, like the log's.
#[derive(Debug, Default)]
pub(crate) struct Probing {
    /// What is installed, to choose from. `None` until asked.
    pub(crate) titles: Option<Vec<pros_core::titles::Metadata>>,
    /// Which target the titles were last asked of, so a refusal is not retried every frame.
    pub(crate) titles_for: Option<String>,
    /// Which title will be launched.
    pub(crate) id: Option<String>,
    /// How long a run follows the log before it stops on its own, in seconds.
    pub(crate) seconds: u64,
    /// What the last run captured - its steps, marked `--`, and the log lines between them.
    ///
    /// Kept apart from [`State::lines`] so the run's start and end stay visible.
    pub(crate) lines: Vec<String>,
    /// The step the run is on, or how it ended.
    pub(crate) status: String,
    /// What to keep on screen - the log screen's filter, separately held.
    pub(crate) filter: String,
    /// Whether the filter is a regular expression.
    pub(crate) regex: bool,
}

/// Which windows are open.
///
/// Grouped apart from the drawing loop's own notes on [`State`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Showing {
    /// The register dialog.
    pub(crate) registering: bool,
    /// The about window.
    pub(crate) about: bool,
}

/// Everything the window is showing.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// Targets this machine knows about.
    pub(crate) targets: Vec<Target>,
    /// Which one is selected, by position in `targets`.
    pub(crate) chosen: Option<usize>,
    /// Windows that are open.
    pub(crate) showing: Showing,
    /// Which section is on screen.
    pub(crate) section: Section,
    /// What is running, and since when.
    pub(crate) waiting: Option<Waiting>,
    /// A job that has begun and not yet been handed to the worker.
    pub(crate) pending: Option<Job>,
    /// How far a long copy has got.
    pub(crate) progress: Option<pros_core::transfer::Progress>,
    /// The last check, when one has been run.
    pub(crate) report: Option<Report>,
    /// The boot list, when it could be read.
    pub(crate) chain: Option<pros_core::chain::Chain>,
    /// A description that now points at a newer release, waiting to be written to the list.
    pub(crate) relisted: Option<(pros_core::manifest::Payload, pros_core::sources::Upstream)>,
    /// Whole startup lists a plan has agreed to write, once its transfers have landed.
    ///
    /// Applied with [`State::after_transfers`], because the entries name files the plan's sends
    /// put in place. More than one because a payload-manager chain has two lists: the
    /// autoloader's starts the manager, which then runs its own. Each is reviewed separately.
    pub(crate) rebuild: Vec<(String, Vec<String>)>,
    /// Which chain the configurator would build, by name.
    ///
    /// A name, not an index, because the presets file can be edited between runs.
    pub(crate) preset: String,
    /// Which list the configurator is being pointed at, while somebody is choosing.
    ///
    /// `None` when it is not open. Separate from the list being viewed, so picking one does
    /// not move the view.
    pub(crate) setting_up: Option<usize>,
    /// List edits a plan has agreed but that cannot be made until its transfers land.
    ///
    /// An entry may only name a file the manager can resolve on internal storage, and the
    /// plan's own send is what puts it there, so the edit waits for the send.
    pub(crate) after_transfers: Vec<pros_core::recovery::Fix>,
    /// Whether everything should be asked again about the target already selected.
    ///
    /// Distinct from a change of target: a refresh keeps what is on screen until the new
    /// answers arrive, a new target clears it.
    pub(crate) resurvey: bool,
    /// Which target the last check was about.
    pub(crate) checked_for: Option<String>,
    /// The target's listing of wherever the browser is looking.
    pub(crate) library: Vec<pros_core::library::Item>,
    /// Listings already fetched this session, by the path they are of.
    ///
    /// Every two-sided section browses into the one `library`, so this saves re-fetching when
    /// moving between them. Emptied whenever a job disturbs the target.
    pub(crate) seen: std::collections::BTreeMap<String, Vec<pros_core::library::Item>>,
    /// This machine's listing of the section's folder.
    pub(crate) local: Vec<pros_core::library::Item>,
    /// Where the browser is looking on the target.
    pub(crate) library_path: String,
    /// Which section that path was settled for.
    pub(crate) library_place: Option<Section>,
    /// Where the browser is looking on this machine.
    pub(crate) local_path: String,
    /// Somewhere the target said to go, once it had been asked.
    pub(crate) go_to: Option<String>,
    /// Title names, by identifier, once the target has said.
    pub(crate) names: std::collections::BTreeMap<String, String>,
    /// Where a payload is installed to on the target.
    ///
    /// Conventional and unmeasured, so it is an editable box rather than a constant.
    pub(crate) install_dir: String,
    /// The log, as it arrives.
    pub(crate) lines: Vec<String>,
    /// The last command's output.
    pub(crate) said: String,
    /// The last thing that went wrong.
    pub(crate) trouble: Option<String>,
    /// What the target said about where this section's things live, and which section asked,
    /// so the answer is not shown in another section.
    pub(crate) located: Option<(Section, pros_core::locate::Where)>,
    /// The two sides as one list, with what is ticked in it.
    pub(crate) listing: crate::listing::Listing,
    /// Whether to draw that list as one table rather than two panes.
    pub(crate) merged: bool,
    /// What this tool has done this session.
    ///
    /// Not the log: this is this program's account of what it asked.
    pub(crate) journal: crate::journal::Journal,
    /// The four pad slots, what drives each, and the key layout they share.
    pub(crate) pads: pros_link::pads::Pads,
    /// How many pad records have been built this session.
    ///
    /// A count that climbs while keys are pressed confirms the mapping without a receiver.
    pub(crate) pad_records: u64,
    /// Where controller records go, when anywhere.
    pub(crate) feed: pros_link::feed::Feed,
    /// The port the input payload is expected on.
    ///
    /// Editable: both ends are ours and the number is chosen, not measured.
    pub(crate) feed_port: String,
    /// The stream coming back the other way, when one is.
    ///
    /// Owned here, not by the panel, so switching sections does not end it.
    pub(crate) watching: pros_core::watch::Watching,
    /// The port the video payload is expected on. Editable, for the same reason.
    pub(crate) watch_port: String,
    /// Which slot the pending binding belongs to.
    ///
    /// Held so the binding lands on the slot it was started for, not the one on screen.
    pub(crate) binding_slot: Option<u8>,
    /// Which button is waiting to be bound to the next key pressed.
    ///
    /// Held here so it survives between frames.
    pub(crate) binding: Option<pros_link::pad::Button>,
    /// Groups the person has folded away in the payloads table.
    ///
    /// Records what is folded, so a new group starts open.
    pub(crate) folded: std::collections::BTreeSet<String>,
    /// What the target is, once asked.
    pub(crate) system: Option<pros_core::system::Report>,
    /// Every payload file on the target, once looked for.
    ///
    /// `None` until asked, which is not none found; otherwise every startup entry would read
    /// as missing.
    pub(crate) payloads_there: Option<Vec<pros_core::payloads::There>>,
    /// A file somebody dropped that nothing describes.
    ///
    /// Held rather than refused: something just built has no publisher or digest to describe.
    pub(crate) adhoc: Option<PathBuf>,
    /// A destructive action waiting to be confirmed, and what it would act on.
    ///
    /// Not undoable, so it goes through a panel naming each thing that would go.
    pub(crate) pending_delete: Option<(crate::listing::Offer, Vec<crate::listing::Entry>)>,
    /// The startup list, once read, with any edits not yet written.
    pub(crate) boot: Option<pros_core::boot::Boot>,
    /// Which row of it is selected.
    ///
    /// One row: the actions move a single step.
    pub(crate) boot_at: Option<usize>,
    /// The payload manager's settings, once read.
    pub(crate) settings: Option<pros_core::autoload::Settings>,
    /// An edit to those settings that has not been written.
    pub(crate) pending_change: Option<pros_core::autoload::Change>,
    /// Packages on this machine waiting for somebody to confirm installing them.
    ///
    /// A list, because the toolbar selects several.
    pub(crate) pending_install: Option<Vec<PathBuf>>,
    /// Which target the log was last started for, so a refusal is not retried every frame.
    pub(crate) followed_for: Option<String>,
    /// Every startup list the loaded chains declare.
    ///
    /// Read once at startup, not per frame; a chain added while open is seen on next start.
    pub(crate) lists: Vec<pros_core::chain::Held>,
    /// Which startup list the autoload screen is showing, into [`Self::lists`].
    pub(crate) list_at: usize,
    /// A chain read off a target, waiting to be written down as a preset.
    ///
    /// Built once when the button is pressed, since building reads the presets file.
    pub(crate) exporting: Option<Exporting>,
    /// What to keep on the log screen, if anything.
    ///
    /// Filters the view, never the record: clearing it shows every line again.
    pub(crate) log_filter: String,
    /// Whether the log filter box is read as a regular expression rather than plain text.
    ///
    /// Off by default; plain substring is the usual case.
    pub(crate) log_regex: bool,
    /// The probe screen: what it can launch, and what the last run captured.
    pub(crate) probing: Probing,
    /// A doctor's plan that has been shown to somebody and not yet agreed to.
    ///
    /// A plan reaches the queue only from here, behind a button: this program suggests and
    /// never acts on its own.
    pub(crate) pending_plan: Option<Pending>,
    /// Which finding a plan was carried out for, while it is still being carried out.
    ///
    /// Kept so the next check can confirm the finding is answered, not assume it.
    pub(crate) fixing: Option<String>,
    /// Where the two panes are split, as the left one share of the usable width.
    ///
    /// A fraction rather than pixels, so it survives a resize.
    pub(crate) split: f32,
    /// Jobs behind the one that is running.
    ///
    /// Not a second scheduler: one job still runs at a time, and [`Self::finish`] starts the
    /// next when the previous ends.
    pub(crate) queued: std::collections::VecDeque<Job>,
    /// A copy that was not attempted, and what it would need.
    pub(crate) refused: Option<pros_core::origin::Needs>,
    /// A title transfer refused for an inert destination path or an incompatible prefix.
    pub(crate) guard_refusal: Option<pros_core::guard::Refusal>,
    /// What the last finished job may have made untrue.
    ///
    /// Set here and acted on by the window, so the worker never reaches into the display.
    pub(crate) disturbed: Vec<Disturbs>,
    /// What a person has typed into the command box.
    pub(crate) command: String,
    /// What a person has typed into the address box.
    pub(crate) address: String,
    /// What a person has typed into the name box.
    pub(crate) name: String,
    /// The target whose address is being edited, when the register dialog is in edit mode.
    ///
    /// `None` is a fresh registration; `Some(name)` re-registers that name with a new address,
    /// keeping its ports and chain.
    pub(crate) editing: Option<String>,
}

impl State {
    /// A window that has just opened, with whatever is registered.
    #[must_use]
    pub(crate) fn new(targets: Vec<Target>) -> Self {
        Self {
            chosen: (!targets.is_empty()).then_some(0),
            targets,
            // Conventional and unmeasured; the window says so beside the editable box.
            library_path: "/user/app".to_owned(),
            preset: pros_core::recovery::baseline::first().name,
            // Every startup list comes from the chains, shipped and user-added.
            lists: pros_core::chain::lists(),
            // One of the two directories `payload_mgr_resolve_path` searches, as the doctor's
            // plans use.
            install_dir: pros_core::payloads::INTERNAL.to_owned(),
            name: "ps5".to_owned(),
            // Chosen, not measured; filled in so the box is editable rather than guessed.
            feed_port: pros_link::feed::PORT.to_string(),
            watching: pros_core::watch::Watching::idle(),
            split: 0.5,
            watch_port: pros_core::watch::PORT.to_string(),
            // The command line's own default for `pros probe --seconds`.
            probing: Probing {
                seconds: 120,
                ..Probing::default()
            },
            ..Self::default()
        }
    }

    /// The target being acted on.
    #[must_use]
    pub(crate) fn target(&self) -> Option<&Target> {
        self.chosen.and_then(|which| self.targets.get(which))
    }

    /// The startup list currently being shown.
    pub(crate) fn list(&self) -> pros_core::chain::Held {
        self.lists
            .get(self.list_at)
            .or_else(|| self.lists.first())
            .cloned()
            // Only before the lists are read; `chain::lists` returns at least one entry.
            .unwrap_or_else(|| pros_core::chain::Held {
                label: "no chain declares a list".to_owned(),
                path: String::new(),
                editable: false,
                autoloader: false,
            })
    }

    /// Whether anything may be started right now.
    #[must_use]
    pub(crate) const fn is_idle(&self) -> bool {
        self.waiting.is_none()
    }

    /// Starts a job, if nothing else is running.
    ///
    /// Answers whether it was started; refused while busy so two answers never interleave.
    pub(crate) fn begin(&mut self, job: Job) -> bool {
        if self.waiting.is_some() {
            return false;
        }
        self.trouble = None;
        self.progress = None;
        self.guard_refusal = None;
        // Recorded at the start, so a job that never returns still appears.
        self.journal
            .began(job.describe(), Self::target_in(&job).map(str::to_owned));
        self.pending = Some(job.clone());
        self.waiting = Some(Waiting {
            job,
            since: Instant::now(),
        });
        true
    }

    /// Starts a job, or puts it behind whatever is running.
    ///
    /// For a multi-select: where [`Self::begin`] refuses while busy, this queues.
    pub(crate) fn queue(&mut self, job: Job) {
        if self.waiting.is_some() {
            self.queued.push_back(job);
        } else {
            self.begin(job);
        }
    }

    /// How many are waiting their turn.
    #[must_use]
    pub(crate) fn queued(&self) -> usize {
        self.queued.len()
    }

    /// Whether a screen is waiting on the answer it cannot be drawn without.
    ///
    /// True only when there is nothing to show yet and an answer is on its way. A re-read
    /// leaves what is shown in place ([`Self::re_reading`]); nothing shown and nothing coming
    /// is a screen nobody has asked about.
    pub(crate) fn still_arriving(&self, section: Section) -> bool {
        self.nothing_yet(section) && self.expecting(section)
    }

    /// Whether a screen has anything at all to draw.
    fn nothing_yet(&self, section: Section) -> bool {
        match section {
            Section::Check => self.report.is_none(),
            Section::Autoload => self.boot.is_none(),
            Section::System => self.system.is_none(),
            Section::Payloads => self.payloads_there.is_none(),
            // Finding saves is a navigation into the shared listing.
            Section::Saves
            | Section::Filesystem
            | Section::Titles
            | Section::Cheats
            | Section::Packages => self.library.is_empty(),
            // Nothing to fetch before use; somebody starts them.
            Section::Log | Section::Shell | Section::Stream | Section::Controllers => false,
            Section::Probe => self.probing.titles.is_none(),
        }
    }

    /// Whether an answer this screen needs is running or waiting its turn.
    ///
    /// Queued jobs count too: the survey on arrival queues several, and a screen whose answer
    /// is behind others must not say nobody asked.
    fn expecting(&self, section: Section) -> bool {
        let wanted = |job: &Job| job.fills().contains(&section);
        self.waiting
            .as_ref()
            .is_some_and(|running| wanted(&running.job))
            || self.queued.iter().any(wanted)
    }

    /// Whether a screen already showing something is being read again.
    ///
    /// Shown beside what is on screen, not instead of it.
    pub(crate) fn re_reading(&self, section: Section) -> bool {
        !self.nothing_yet(section) && self.expecting(section)
    }

    /// Forgets everything not yet started.
    ///
    /// The running job is not touched.
    pub(crate) fn drop_queued(&mut self) -> usize {
        std::mem::take(&mut self.queued).len()
    }

    /// Which target a job is about, when it is about one.
    fn target_in(job: &Job) -> Option<&str> {
        match job {
            Job::Check(target)
            | Job::Shell(target, _)
            | Job::Pull(target, ..)
            | Job::Browse(target, _)
            | Job::Push(target, ..)
            | Job::Install(target, ..)
            | Job::Backup(target, ..)
            | Job::Restore(target, ..)
            | Job::Send(target, ..)
            | Job::Names(target, _)
            | Job::Titles(target)
            | Job::FindSaves(target)
            | Job::Locate(target, _)
            | Job::Launch(target, _)
            | Job::RunThere(target, _)
            | Job::ReadList(target, _)
            | Job::ReadAutoload(target)
            | Job::ReadSystem(target)
            | Job::RestartUi(target)
            | Job::CloseTitle(target, _)
            | Job::EndProcess(target, _)
            | Job::InstallPackage(target, _)
            | Job::FindPayloads(target, _)
            | Job::DeleteThere(target, _)
            | Job::WriteAutoload(target, ..)
            | Job::EnableAutoload(target)
            | Job::CaptureConfig(target)
            | Job::PlaceFile(target, ..) => Some(&target.name),
            // No target involved.
            Job::Fetch(..) | Job::Relist(..) | Job::DeleteHere(..) => None,
        }
    }

    /// How a result should read in the record.
    fn how_it_went(done: &Done) -> crate::journal::Ending {
        use crate::journal::Ending;
        match done {
            Done::Failed(why) => Ending::Failed(why.clone()),
            Done::Refused(needs) => Ending::Refused(match needs {
                pros_core::origin::Needs::Resigning { wrote, .. } => {
                    format!("written by another account ({wrote})")
                }
                pros_core::origin::Needs::Unknown(why) => why.clone(),
                pros_core::origin::Needs::Nothing => String::new(),
            }),
            Done::GuardRefused(refusal) => Ending::Refused(refusal.explanation.clone()),
            // A stopped copy is recorded as stopped, not done.
            Done::Copied(summary, _)
                if summary
                    .skipped
                    .iter()
                    .any(|one| one.why.contains("stopped")) =>
            {
                Ending::Stopped
            }
            Done::Copied(summary, into) => Ending::Done(format!(
                "{} files, {} bytes to {into}{}{}",
                summary.files,
                summary.bytes,
                if summary.unchanged > 0 {
                    format!(", {} unchanged", summary.unchanged)
                } else {
                    String::new()
                },
                if summary.is_complete() {
                    String::new()
                } else {
                    format!(" - {} not copied", summary.skipped.len())
                }
            )),
            Done::Fetched(name, into) => {
                Ending::Done(format!("{name} verified into {}", into.display()))
            }
            Done::Relisted(payload, _) => Ending::Done(format!(
                "{} now describes {}",
                payload.name,
                payload.version.as_deref().unwrap_or("a new release")
            )),
            Done::Installed(said) => {
                if said.is_a_known_failure() {
                    Ending::Failed(said.describe())
                } else {
                    Ending::Done(said.describe())
                }
            }
            Done::Checked(report, _) => Ending::Done(format!("{:?}", report.verdict())),
            Done::Browsed(items) => Ending::Done(format!("{} entries", items.len())),
            Done::Named(found) => Ending::Done(format!("{} names", found.len())),
            Done::Titles(found) => Ending::Done(format!("{} titles", found.len())),
            Done::Pulled { into, bytes } => {
                Ending::Done(format!("{bytes} bytes to {}", into.display()))
            }
            Done::Located(found) => Ending::Done(match found.path() {
                Some(path) => path.to_owned(),
                None => "none of them".to_owned(),
            }),
            Done::Payloads(found) => Ending::Done(format!("{} payload files", found.len())),
            Done::Launched(said) => match said {
                pros_core::launch::Said::NotAnId | pros_core::launch::Said::Refused(_) => {
                    Ending::Failed(said.describe())
                }
                pros_core::launch::Said::Asked(_) => Ending::Done(said.describe()),
            },
            Done::RanThere(said) => match said {
                pros_core::hbldr::Said::NotFound(_) | pros_core::hbldr::Said::NoArgument => {
                    Ending::Failed(said.describe())
                }
                pros_core::hbldr::Said::Ran(_) => Ending::Done(said.describe()),
            },
            Done::List(boot) => Ending::Done(format!("{} entries", boot.steps.len())),
            Done::Autoload(settings, boot) => Ending::Done(format!(
                "{} settings, {} startup entries",
                settings.all().len(),
                boot.steps.len()
            )),
            Done::Captured(files, _) => Ending::Done(format!(
                "{} file{} the chain will carry",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            )),
            Done::System(report) => Ending::Done(format!("{} facts", report.facts.len())),
            Done::Signalled { note, .. } => Ending::Done(note.clone()),
            Done::Said(_) | Done::FoundSaves(_) => Ending::Done(String::new()),
        }
    }

    /// Whether a begun job is waiting to be handed to the worker.
    #[cfg(test)]
    fn pending_after_begin(&self) -> bool {
        self.pending.is_some()
    }

    /// Takes the result of the job that was running.
    ///
    /// A failure clears the panel the job would have filled (rule 2).
    pub(crate) fn finish(&mut self, done: Done) {
        let Some(waiting) = self.waiting.take() else {
            return;
        };
        self.journal.ended(Self::how_it_went(&done));
        // Whatever the outcome: a copy that failed part way still moved files.
        self.disturbed = waiting.job.disturbs().to_vec();
        match done {
            Done::Checked(report, chain) => {
                self.report = Some(*report);
                self.chain = chain;
            }
            // Handed up: the payload list belongs to the window.
            Done::Relisted(payload, found) => self.relisted = Some((*payload, *found)),
            Done::Browsed(items) => {
                if let Job::Browse(_, where_) = &waiting.job {
                    self.seen.insert(where_.clone(), items.clone());
                }
                self.library = items;
            }
            Done::FoundSaves(found) => self.carry_saves(found),
            Done::Named(found) => {
                for about in found {
                    if let Some(name) = about.name {
                        self.names.insert(about.id, name);
                    }
                }
            }
            Done::Titles(found) => self.carry_titles(found),
            Done::Copied(summary, where_to) => {
                // An incomplete copy is trouble, with a count, not a quiet summary.
                self.said = format!(
                    "{} files, {} bytes -> {where_to}{}",
                    summary.files,
                    summary.bytes,
                    if summary.unchanged > 0 {
                        format!(" ({} unchanged, not re-sent)", summary.unchanged)
                    } else {
                        String::new()
                    }
                );
                if !summary.is_complete() {
                    self.trouble = Some(format!(
                        "{} not copied - this is not a backup. First: {}",
                        summary.skipped.len(),
                        summary
                            .skipped
                            .first()
                            .map_or_else(String::new, |one| format!("{} ({})", one.path, one.why))
                    ));
                }
            }
            Done::Said(text) => self.said = text,
            Done::Refused(needs) => {
                self.refused = Some(needs.clone());
            }
            Done::GuardRefused(refusal) => {
                self.guard_refusal = Some(refusal.clone());
            }
            Done::Payloads(found) => self.payloads_there = Some(found),
            Done::Launched(said) => self.said = said.describe(),
            Done::RanThere(said) => self.said = said.describe(),
            Done::List(boot) => {
                // The list only; this may not be the manager's list.
                self.boot = Some(*boot);
                self.boot_at = None;
            }
            Done::Autoload(settings, boot) => {
                self.settings = Some(*settings);
                self.boot = Some(*boot);
                self.boot_at = None;
            }
            Done::Captured(files, notes) => self.carry_captured(files, notes),
            Done::System(report) | Done::Signalled { report, .. } => {
                self.system = Some(*report);
            }
            Done::Installed(said) => {
                // Only a recognised failure is trouble; an unrecognised answer is shown as said.
                if said.is_a_known_failure() {
                    self.trouble = Some(said.describe());
                } else {
                    self.said = said.describe();
                }
            }
            Done::Located(found) => {
                self.located = Some((self.section, found.clone()));
                // Not moved to an absent directory, whose empty listing would look like an
                // installed tool with nothing in it.
                if let Some(path) = found.path() {
                    self.go_to = Some(path.to_owned());
                }
            }
            Done::Fetched(name, into) => {
                self.said = format!("{name} kept and verified: {}", into.display());
            }
            Done::Pulled { into, bytes } => {
                self.said = format!("{bytes} bytes written to {}", into.display());
            }
            Done::Failed(why) => {
                self.trouble = Some(why);
                self.clear(waiting.job.replaces());
            }
        }
        self.next_in_line();
    }

    /// What is installed, for the probe screen to choose from.
    fn carry_titles(&mut self, found: Vec<pros_core::titles::Metadata>) {
        // Other screens show these names in place of identifiers too.
        for about in &found {
            if let Some(name) = &about.name {
                self.names.insert(about.id.clone(), name.clone());
            }
        }
        // Keep the choice when it is still installed; otherwise start on the first.
        if self
            .probing
            .id
            .as_ref()
            .is_none_or(|id| !found.iter().any(|about| &about.id == id))
        {
            self.probing.id = found.first().map(|about| about.id.clone());
        }
        self.probing.titles = Some(found);
    }

    /// Where the target said its saves are, or why it could not say.
    ///
    /// Several accounts are named as trouble rather than one being picked.
    fn carry_saves(&mut self, found: pros_core::saves::Found) {
        match found {
            pros_core::saves::Found::Here(path) => self.go_to = Some(path),
            pros_core::saves::Found::Several(users) => {
                self.trouble = Some(format!(
                    "several users, so this does not choose: {}",
                    users.join(", ")
                ));
            }
            pros_core::saves::Found::None => {
                self.trouble = Some(format!("no user folders under {}", pros_core::saves::HOME));
            }
        }
    }

    /// Folds a capture into the export waiting for it; discarded if the panel was closed.
    fn carry_captured(
        &mut self,
        files: Vec<pros_core::recovery::baseline::Captured>,
        notes: Vec<String>,
    ) {
        if let Some(export) = self.exporting.as_mut() {
            export.preset.files = files;
            export.notes.extend(notes);
            export.capturing = false;
        }
    }

    /// Starts whatever is waiting, unless the last one gave somebody something to read.
    ///
    /// Trouble stops the rest of the queue, with a count: [`Self::begin`] clears `trouble`, so
    /// the next job would otherwise erase the message. Pressing the button again carries on.
    fn next_in_line(&mut self) {
        if self.trouble.is_some() {
            let dropped = self.drop_queued();
            if dropped > 0 {
                let why = self.trouble.take().unwrap_or_default();
                self.trouble = Some(format!(
                    "{why}\n{dropped} more were not started - the rest of the selection is \
                     still ticked"
                ));
            }
            return;
        }
        if let Some(next) = self.queued.pop_front() {
            self.begin(next);
        }
    }

    /// Empties a panel, because what was in it is no longer known to be true.
    fn clear(&mut self, panel: Panel) {
        match panel {
            Panel::Report => self.report = None,
            Panel::Said => self.said.clear(),
            Panel::Library => self.library.clear(),
            Panel::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pros_core::check::{Finding, Report};
    use pros_core::target::Target;
    use pros_link::service::{Reachability, SERVICES};

    use super::{Disturbs, Section};

    use super::{Done, Job, State};

    fn target(name: &str) -> Target {
        Target {
            name: name.to_owned(),
            address: "127.0.0.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }
    }

    fn a_report() -> Report {
        let findings = SERVICES
            .iter()
            .map(|service| Finding {
                service: service.clone(),
                reachability: Reachability {
                    open: true,
                    took: Duration::from_millis(5),
                },
            })
            .collect();
        Report::new("ps5", "127.0.0.1", findings)
    }

    /// A begun job is left pending for the worker to start.
    #[test]
    fn beginning_a_job_leaves_it_for_the_worker_to_start() {
        let mut state = State::new(vec![target("ps5")]);
        assert!(
            !state.pending_after_begin(),
            "nothing begun, nothing pending"
        );
        assert!(state.begin(Job::Check(target("ps5"))));
        assert!(
            state.pending_after_begin(),
            "a begun job was not left anywhere the window would find it"
        );
    }

    /// A second job is refused while one runs.
    #[test]
    fn only_one_job_runs_at_a_time() {
        let mut state = State::new(vec![target("ps5")]);
        assert!(state.begin(Job::Check(target("ps5"))));
        assert!(
            !state.begin(Job::Browse(target("ps5"), "/data".to_owned())),
            "a second job started while the first was still running"
        );
        state.finish(Done::Checked(Box::new(a_report()), None));
        assert!(state.is_idle());
        assert!(state.begin(Job::Browse(target("ps5"), "/data".to_owned())));
    }

    /// A failed refresh clears the previous answer and says why.
    #[test]
    fn a_failed_job_clears_what_it_would_have_replaced() {
        let mut state = State::new(vec![target("ps5")]);
        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Checked(Box::new(a_report()), None));
        assert!(state.report.is_some());

        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Failed("the target stopped answering".to_owned()));

        assert!(
            state.report.is_none(),
            "a stale report survived a failed refresh"
        );
        assert!(state.trouble.is_some(), "and nothing said why");
    }

    /// A failure clears only its own panel.
    #[test]
    fn a_failure_clears_only_its_own_panel() {
        let mut state = State::new(vec![target("ps5")]);
        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Checked(Box::new(a_report()), None));

        state.begin(Job::Browse(target("ps5"), "/data".to_owned()));
        state.finish(Done::Failed("no such directory".to_owned()));

        assert!(state.report.is_some(), "an unrelated panel was cleared");
        assert!(state.library.is_empty());
    }

    /// Starting a job clears the previous trouble.
    #[test]
    fn starting_a_job_clears_the_previous_trouble() {
        let mut state = State::new(vec![target("ps5")]);
        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Failed("nothing answered".to_owned()));
        assert!(state.trouble.is_some());

        state.begin(Job::Check(target("ps5")));
        assert!(
            state.trouble.is_none(),
            "the previous failure is still on screen while the next attempt runs"
        );
    }

    /// An answer arriving when nothing was asked is dropped rather than displayed.
    #[test]
    fn an_answer_with_nothing_waiting_for_it_is_ignored() {
        let mut state = State::new(vec![target("ps5")]);
        state.finish(Done::Checked(Box::new(a_report()), None));
        assert!(state.report.is_none(), "an unasked-for answer was shown");
    }

    /// The waiting text names what is being waited for.
    #[test]
    fn waiting_says_which_thing_it_is_waiting_for() {
        assert_eq!(Job::Check(target("desk")).describe(), "checking desk");
        assert_eq!(
            Job::Browse(target("desk"), "/data/pldmgr".to_owned()).describe(),
            "opening /data/pldmgr"
        );
    }

    /// With nothing registered, nothing is selected.
    #[test]
    fn nothing_registered_means_nothing_chosen() {
        let state = State::new(Vec::new());
        assert!(state.target().is_none());
        assert_eq!(State::new(vec![target("only")]).chosen, Some(0));
    }

    /// A refusal is kept to show, is not trouble, and clears no panel.
    #[test]
    fn a_refusal_says_what_was_needed_without_clearing_anything() {
        let mut state = State::new(vec![Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }]);
        state.library = vec![item("PPSA01650")];

        state.begin(Job::Restore(
            state.target().cloned().expect("one target"),
            std::path::PathBuf::from("."),
            "/user/home/beefcafe/savedata_prospero".to_owned(),
            false,
        ));
        state.finish(Done::Refused(pros_core::origin::Needs::Resigning {
            wrote: "769f77716958d37e".to_owned(),
            going_to: "00112233445566aa".to_owned(),
        }));

        assert!(state.refused.is_some(), "the reason should be kept to show");
        assert!(
            state.trouble.is_none(),
            "a refusal is not trouble - nothing went wrong"
        );
        assert_eq!(
            state.library.len(),
            1,
            "the listing should survive a copy that was declined"
        );
        assert!(state.is_idle(), "and the job is over");
    }

    /// One listing entry, named.
    fn item(name: &str) -> pros_core::library::Item {
        pros_core::library::Item {
            name: name.to_owned(),
            id: None,
            kind: pros_core::library::Kind::Folder,
            size: None,
        }
    }

    /// A locate answer belongs to the section that asked, not the one on screen.
    #[test]
    fn a_locate_answer_belongs_to_the_section_that_asked() {
        let mut state = State::new(vec![Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }]);
        state.section = Section::Cheats;
        state.begin(Job::Locate(
            state.target().cloned().expect("one target"),
            Section::Cheats.candidates(),
        ));
        state.finish(Done::Located(pros_core::locate::Where::NoneOfThem(vec![
            "/data/cheatrunner/cheats".to_owned(),
        ])));

        let (asked, _) = state.located.as_ref().expect("an answer was kept");
        assert_eq!(*asked, Section::Cheats, "it should remember who asked");

        state.section = Section::Titles;
        let still_ours = state
            .located
            .as_ref()
            .is_some_and(|(asked, _)| *asked == state.section);
        assert!(
            !still_ours,
            "the cheats answer should not read as an answer about titles"
        );
    }

    /// Sending a payload marks the check report stale.
    #[test]
    fn running_a_payload_makes_what_the_target_can_do_stale() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        let job = Job::Send(
            target,
            "klogsrv".to_owned(),
            std::path::PathBuf::from("klogsrv.elf"),
        );
        assert_eq!(job.disturbs(), [Disturbs::Report]);
    }

    /// Read-only jobs disturb nothing, so a check does not trigger itself.
    #[test]
    fn asking_what_is_true_does_not_make_anything_untrue() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        assert!(Job::Check(target.clone()).disturbs().is_empty());
        assert!(Job::ReadSystem(target.clone()).disturbs().is_empty());
        assert!(
            Job::Browse(target, "/data".to_owned())
                .disturbs()
                .is_empty()
        );
    }

    /// What a job disturbs is recorded even when it failed.
    #[test]
    fn a_job_that_failed_still_leaves_the_world_changed() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        let mut state = State::new(vec![target.clone()]);
        state.begin(Job::Push(
            target,
            std::path::PathBuf::from("a.elf"),
            "/data/a.elf".to_owned(),
        ));
        state.finish(Done::Failed("refused".to_owned()));

        assert_eq!(
            state.disturbed,
            [Disturbs::There],
            "a failed copy may still have written something"
        );
    }

    /// A shell command is assumed to have changed the target.
    #[test]
    fn an_arbitrary_command_is_assumed_to_have_changed_things() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        let job = Job::Shell(target, "rm /data/thing".to_owned());
        assert!(job.disturbs().contains(&Disturbs::Report));
        assert!(job.disturbs().contains(&Disturbs::There));
    }
}

#[cfg(test)]
mod queue_tests {
    use std::path::PathBuf;

    use pros_core::target::Target;

    use super::{Done, Job, State};

    fn target() -> Target {
        Target {
            name: "ps5".to_owned(),
            address: "127.0.0.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }
    }

    fn state() -> State {
        State::new(vec![target()])
    }

    fn push(which: u8) -> Job {
        Job::Push(
            target(),
            PathBuf::from(format!("{which}.pkg")),
            format!("/data/{which}.pkg"),
        )
    }

    /// Queuing a selection starts the first and keeps the rest waiting.
    #[test]
    fn a_selection_of_four_asks_for_four() {
        let mut state = state();
        for which in 0..4 {
            state.queue(push(which));
        }
        assert!(!state.is_idle(), "the first one runs");
        assert_eq!(state.queued(), 3, "the rest are waiting, not gone");
    }

    /// Each finish starts the next, until the queue is empty.
    #[test]
    fn finishing_one_starts_the_next() {
        let mut state = state();
        for which in 0..3 {
            state.queue(push(which));
        }
        for left in [1, 0] {
            state.finish(Done::Said("ok".to_owned()));
            assert_eq!(state.queued(), left);
            assert!(!state.is_idle(), "{left} left, so something is running");
        }
        state.finish(Done::Said("ok".to_owned()));
        assert!(state.is_idle(), "the line is empty and nothing is running");
    }

    /// A failure stops the queue and says how many were not started.
    #[test]
    fn a_failure_stops_the_rest_and_says_so() {
        let mut state = state();
        for which in 0..4 {
            state.queue(push(which));
        }
        state.finish(Done::Failed("the target refused STOR".to_owned()));

        assert!(state.is_idle(), "nothing carried on past the failure");
        assert_eq!(state.queued(), 0, "the line was dropped, not left dangling");
        let trouble = state.trouble.expect("a failure leaves something to read");
        assert!(
            trouble.contains("the target refused STOR"),
            "the reason survives: {trouble}"
        );
        assert!(
            trouble.contains('3'),
            "and says how many did not start: {trouble}"
        );
    }

    /// A copy that finished with files missing also stops the queue.
    #[test]
    fn an_incomplete_copy_also_stops_the_rest() {
        let mut state = state();
        for which in 0..3 {
            state.queue(push(which));
        }
        let mut summary = pros_core::transfer::Summary::default();
        summary.skipped.push(pros_core::transfer::Skipped {
            path: "one.pkg".to_owned(),
            why: "refused".to_owned(),
        });
        state.finish(Done::Copied(Box::new(summary), "/data".to_owned()));

        assert!(state.is_idle());
        assert_eq!(state.queued(), 0);
        assert!(state.trouble.is_some_and(|why| why.contains('2')));
    }

    /// Clearing the queue leaves the running job alone.
    #[test]
    fn clearing_the_queue_does_not_touch_what_is_running() {
        let mut state = state();
        for which in 0..3 {
            state.queue(push(which));
        }
        assert_eq!(state.drop_queued(), 2);
        assert!(!state.is_idle(), "the running one is untouched");
        assert_eq!(state.queued(), 0);
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

#[cfg(test)]
mod retention_tests {
    use pros_core::library::{Item, Kind};
    use pros_core::target::Target;

    use super::{Done, Job, State};

    fn target() -> Target {
        Target {
            name: "ps5".to_owned(),
            address: "127.0.0.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }
    }

    fn listing() -> Vec<Item> {
        vec![Item {
            name: "thing.elf".to_owned(),
            kind: Kind::File,
            size: Some(1),
            id: None,
        }]
    }

    /// A listing is kept by its path.
    #[test]
    fn a_listing_is_remembered_by_its_path() {
        let mut state = State::new(vec![target()]);
        state.begin(Job::Browse(target(), "/data/pkg".to_owned()));
        state.finish(Done::Browsed(listing()));
        assert_eq!(state.seen.get("/data/pkg").map(Vec::len), Some(1));
    }

    /// A push announces that it disturbed the target, the signal that clears kept listings.
    #[test]
    fn what_changed_the_target_is_not_remembered_from_before() {
        let mut state = State::new(vec![target()]);
        state.begin(Job::Browse(target(), "/data/pkg".to_owned()));
        state.finish(Done::Browsed(listing()));
        assert!(!state.seen.is_empty());

        state.begin(Job::Push(
            target(),
            std::path::PathBuf::from("x"),
            "/data/pkg/x".to_owned(),
        ));
        assert!(
            state
                .pending
                .as_ref()
                .is_some_and(|job| job.disturbs().contains(&super::Disturbs::There)),
            "a push must announce that it changed the target"
        );
    }
}
