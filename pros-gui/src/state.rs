//! What the window is showing, and the rules about changing it.
//!
//! # Why this is a separate module with tests
//!
//! A window cannot be looked at from a test, and on the machine this was written on it
//! cannot be looked at at all. So everything that is a **rule** rather than a pixel lives
//! here, where it can be checked: what may run while something else is running, what a
//! failure does to what is on screen, and how long a person has been waiting.
//!
//! What is left in `app.rs` is genuinely only drawing.
//!
//! # The three rules
//!
//! 1. **One job at a time.** Two checks racing would interleave their results and the
//!    window would show half of each.
//! 2. **A failed job clears what it would have replaced.** A report left on screen after a
//!    refresh that failed is a claim about a target that has stopped being true - the same
//!    mistake as caching a capability, made in pixels instead of in a file.
//! 3. **Waiting is visible and timed.** A window that is working and a window that has hung
//!    look identical, which is this project's recurring defect wearing its most literal
//!    costume.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use pros_core::check::Report;
use pros_core::target::Target;

/// Something asked of a target, to be done away from the drawing thread.
#[derive(Debug, Clone)]
pub(crate) enum Job {
    /// Ask what the target can currently do.
    Check(Target),
    /// Run one command.
    Shell(Target, String),
    /// Fetch a file to a local path.
    Pull(Target, String, PathBuf),
    /// Read the target's own payload repository, at this path on the target.
    /// List a directory and read it as a library.
    Browse(Target, String),
    /// Copy a local file onto the target.
    Push(Target, PathBuf, String),
    /// Copy a whole folder off the target.
    Backup(Target, String, PathBuf),
    /// Put a whole folder back onto the target.
    ///
    /// The flag is **going ahead after being told not to**. A save going somewhere other than
    /// the account that wrote it needs re-signing first, and copying it anyway produces files
    /// a target will refuse - so that is refused by default and only happens when somebody
    /// says so, having read why.
    Restore(Target, PathBuf, String, bool),
    /// Send a staged payload, named as the manifest names it.
    ///
    /// **This runs it; it writes nothing.** The name is a trap and has caught this project
    /// once already - see [`Job::Install`], which is the one that puts a file on the disk.
    Send(Target, String, PathBuf),
    /// Put a staged payload on the target's disk, where the manager can resolve it.
    ///
    /// # Why this is not `Push`
    ///
    /// [`Job::Push`] copies one file to one path, which a person chose. This lays a payload
    /// out the way the manager expects to find one - its own folder under the payload
    /// directory, the ELF inside it, and the `.json` beside the ELF that is the only thing on
    /// the target that ever says which build it is. A payload copied without that folder is
    /// not found, and one copied without the sidecar is found and has no version, which is
    /// why most of the version column reads `?`.
    Install(Target, Box<pros_core::manifest::Payload>, PathBuf, String),
    /// Ask the target what each of these titles is called.
    Names(Target, Vec<String>),
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
    /// **Separate from reading the settings**, because there is more than one list and only
    /// one settings file - and the lists are audited by different rules.
    ReadList(Target, pros_core::chain::Held),
    /// Ask the target what it is.
    ReadSystem(Target),
    /// Find every payload file the manager holds, looking inside its folders.
    FindPayloads(Target, String),
    /// Remove files from the target.
    ///
    /// **Destructive, and named so.** It carries the whole list rather than one path, because
    /// what somebody confirmed was a list and doing them one job at a time would ask again
    /// for each.
    /// Remove things from the target, each with whether it is a directory.
    ///
    /// **The flag travels with the path** because the two are removed by different commands,
    /// and working it out at the far end would mean listing the parent again to ask a question
    /// the listing on screen has already answered.
    DeleteThere(Target, Vec<(String, bool)>),
    /// Remove files from this machine.
    DeleteHere(Vec<PathBuf>),
    /// Hold a package out for the target to fetch and register.
    ///
    /// **A state change.** It goes through a confirm that names the file, and what comes back
    /// says whether the target took it - which needs both halves: what it said, and whether it
    /// ever came for the file at all.
    InstallPackage(Target, PathBuf),
    /// Replace the payload manager's settings with this text.
    ///
    /// **The only job here that writes to a target.** It carries the whole file rather than
    /// an edit, because what was reviewed line by line is a file and sending anything else
    /// would make the review a different document from the write.
    WriteAutoload(Target, String, String),
    /// Point a description at what its project has released now.
    ///
    /// **The one job that changes the payload list rather than a target.** It downloads the
    /// new file to learn its digest, and what comes back is a description for review - nothing
    /// is written to the list until that is accepted.
    Relist(Box<pros_core::manifest::Payload>),
    /// Fetch a described payload and keep it, if it is the one described.
    ///
    /// **No target involved.** This is between this machine and a mirror, and it is here
    /// rather than done inline because it is a network round trip like any other and the
    /// window must not stop repainting for it.
    Fetch(Box<pros_core::manifest::Payload>, Option<PathBuf>),
}

impl Job {
    /// What to say while this is running.
    ///
    /// Present tense and specific: *checking* rather than *working*, and the path when
    /// there is one, so a person waiting knows which thing they are waiting for.
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
            Self::FindSaves(_) => "looking for saves".to_owned(),
            Self::Locate(_, where_) => format!("looking in {} places", where_.len()),
            Self::Launch(_, id) => format!("starting {id}"),
            Self::RunThere(_, path) => format!("running {path} on the target"),
            Self::ReadAutoload(_) => "reading the manager's settings".to_owned(),
            Self::ReadList(_, held) => format!("reading {}", held.path),
            Self::ReadSystem(_) => "asking the target what it is".to_owned(),
            Self::FindPayloads(..) => "looking for payloads".to_owned(),
            Self::DeleteThere(_, what) => format!("deleting {} from the target", what.len()),
            Self::DeleteHere(what) => format!("deleting {} from this machine", what.len()),
            Self::InstallPackage(_, path) => format!("installing {}", path.display()),
            Self::WriteAutoload(_, path, _) => format!("writing {path}"),
        }
    }

    /// What finishing this may have made untrue elsewhere.
    ///
    /// **Empty for anything that only reads.** A check disturbs nothing: it *is* the reading,
    /// and saying otherwise would have it ask again forever.
    pub(crate) const fn disturbs(&self) -> &'static [Disturbs] {
        match self {
            // **Two ways of changing what is running on the target.** Sending a payload is
            // the case that started this: a service that was not answering may be answering
            // now, and the screen that said so is wrong until it asks again. Starting a title
            // is the same consequence by a different route.
            Self::Send(..) | Self::Launch(..) | Self::RunThere(..) => &[Disturbs::Report],
            // Something arrived here, or left it.
            Self::Fetch(..)
            | Self::Relist(..)
            | Self::Pull(..)
            | Self::Backup(..)
            | Self::DeleteHere(..) => &[Disturbs::Here],
            // Something arrived there or left it - and for an install, the target has read
            // and registered a package, so its directory may differ because of that.
            Self::Push(..)
            | Self::Install(..)
            | Self::Restore(..)
            | Self::InstallPackage(..)
            | Self::DeleteThere(..) => &[Disturbs::There],
            // The file that was just replaced is the one being shown.
            Self::WriteAutoload(..) => &[Disturbs::Autoload],
            // **A command can do anything at all**, so this assumes it did. The cost is a
            // listing being read again; the alternative is a window that quietly disagrees
            // with a target somebody has just changed by hand.
            Self::Shell(..) => &[Disturbs::Report, Disturbs::There],
            // Everything else reads and changes nothing.
            Self::Check(..)
            | Self::Browse(..)
            | Self::Names(..)
            | Self::FindSaves(..)
            | Self::Locate(..)
            | Self::ReadList(..)
            | Self::ReadAutoload(..)
            | Self::ReadSystem(..)
            | Self::FindPayloads(..) => &[],
        }
    }

    /// Which screens are waiting on this, if any.
    ///
    /// **Only what a screen cannot be drawn without.** A job that merely improves a screen is
    /// not one that screen is waiting for, and treating it as one would hold a usable panel
    /// shut behind an answer nobody needed to see it.
    pub(crate) const fn fills(&self) -> &'static [Section] {
        match self {
            // The check reads the manager's startup list beside its probe, so the screen that
            // audits that list is waiting on it too.
            Self::Check(_) => &[Section::Check, Section::Autoload],
            Self::ReadAutoload(_) | Self::ReadList(..) => &[Section::Autoload],
            Self::ReadSystem(_) => &[Section::System],
            Self::FindPayloads(..) => &[Section::Payloads],
            Self::FindSaves(_) => &[Section::Saves],
            // One listing, five views over it - see the section dispatcher.
            Self::Browse(..) => &[
                Section::Filesystem,
                Section::Titles,
                Section::Saves,
                Section::Cheats,
                Section::Packages,
            ],
            // Everything else changes something or answers something, and no screen is dark
            // until it comes back.
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
            | Self::Relist(..)
            | Self::Fetch(..) => &[],
        }
    }

    /// Which part of the window this will replace when it finishes.
    ///
    /// Used to clear that part when it **fails** instead, so nothing stale is presented as
    /// current. See rule 2.
    const fn replaces(&self) -> Panel {
        match self {
            Self::Check(_) => Panel::Report,
            Self::Shell(..) => Panel::Said,
            // Nothing on screen belongs to these, and for a list that matters: one that
            // failed to arrive must not leave the last one showing under a different name.
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
            | Self::FindSaves(..)
            | Self::Locate(..)
            | Self::ReadAutoload(..)
            | Self::ReadSystem(..)
            | Self::Launch(..)
            | Self::RunThere(..)
            | Self::InstallPackage(..)
            | Self::FindPayloads(..)
            | Self::DeleteThere(..)
            | Self::DeleteHere(..)
            | Self::WriteAutoload(..) => Panel::Nothing,
            Self::Browse(..) => Panel::Library,
        }
    }
}

/// Something a job may have changed, which whatever shows it must therefore read again.
///
/// # Why this is a property of the job and not a flag somebody remembers to set
///
/// It was flags. A download set `arrived`, so the folder listing refreshed; nothing else did,
/// so **running a payload left the check screen red until somebody pressed the check button
/// themselves** - the service was up, the screen said down, and the screen was not lying about
/// anything it had measured. It was reporting a measurement that had stopped being true.
///
/// That is this project's own defect in the window: a display whose *stale* state is
/// indistinguishable from its *accurate* state. One flag per case means the next job added is
/// one nobody thinks to wire up, and the symptom is silence.
///
/// So every job declares it, the compiler insists on an arm for each, and the window acts on
/// what it is told rather than on what it was written to remember.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disturbs {
    /// What the target can currently do. Running a payload makes a service answer.
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
    /// What the target is.
    System(Box<pros_core::system::Report>),
    /// An install ran, and this is what the target said about it.
    Installed(pros_core::install::Said),
    /// Titles said what they are called.
    ///
    /// Carries only what was read. **A title that did not answer is absent from this**,
    /// rather than present with an empty name - the identifier stands for it, which is true.
    Named(Vec<pros_core::titles::Metadata>),
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
    /// **Distinct from [`Done::Said`] because the folder now holds a file it did not.** A row
    /// offering to download something is drawn from a listing of that folder, so a download
    /// that does not refresh the listing leaves the button saying *download* over a file that
    /// is already there - and pressing it again fetches it again.
    Fetched(String, PathBuf),
    /// A description now points at what its project has released.
    ///
    /// Carries the whole new description rather than the fields that changed: what is written
    /// to the list is a record, and rebuilding one from a diff is a second way of saying the
    /// same thing that can disagree with the first.
    Relisted(
        Box<pros_core::manifest::Payload>,
        Box<pros_core::sources::Upstream>,
    ),
    /// It was not attempted, because it would not have worked.
    ///
    /// **Separate from [`Done::Failed`], because nothing went wrong.** A refusal is a finding
    /// made before anything moved, and it carries what would be needed instead - which is the
    /// part somebody acts on.
    Refused(pros_core::origin::Needs),
    /// It did not work, in the target's words or the system's.
    Failed(String),
}

/// Which section of the window is showing.
///
/// The sidebar is a list of these, because they are the things a person came to do rather
/// than parts of one screen. Each is a view with its own toolbar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Section {
    /// What the target can currently do.
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
    /// A command and what it printed.
    Shell,
}

/// One place a section's things might be kept.
///
/// **The label is the point.** These used to be bare paths, and a button took its caption from
/// a fragment of the path - so the packages section offered a choice between *homebrew* and
/// *pkg*, two words that describe nothing and cannot be chosen between. A place has to say what
/// it **is**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Place {
    /// Where on the target.
    pub(crate) path: &'static str,
    /// What to call it - what the place is for, not a piece of its path.
    pub(crate) label: &'static str,
    /// Why things are there, and **where that is known from**.
    ///
    /// Some of these are read from another tool's own documentation rather than measured here,
    /// and say so: a target running something else has neither the tool nor the folder, and
    /// the difference between *documented* and *seen on this target* is the difference this
    /// project exists to keep.
    pub(crate) note: &'static str,
}

impl Section {
    /// The sidebar, in groups.
    ///
    /// **Grouped because they are different kinds of thing.** Copying files between two
    /// machines and reading a log are not neighbours, and a flat list of nine says they are.
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
        ("diagnose", &[Self::Log, Self::Shell]),
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
            Self::Shell => "shell",
        }
    }

    /// One line under the heading, saying what this section is for.
    ///
    /// **Because the section name is not the answer to "what does this do".** *Payloads* and
    /// *titles* both list things that end up running on the target, and nothing in either word
    /// says that one sends bytes to a loader and the other asks the system to boot something it
    /// already has. A person who has to hover a button to find that out has already guessed.
    ///
    /// Kept to a line. Anything longer is documentation, and belongs in `docs/`.
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
            Self::Shell => "one command on the target, and what it printed",
        }
    }

    /// Whether this section already scrolls its own content.
    ///
    /// # Why this is asked rather than one scroll area around everything
    ///
    /// **A scroll area inside a scroll area never scrolls.** The outer one offers the inner
    /// unlimited height, the inner grows to fit, and it therefore never needs a bar - so the
    /// content runs off the bottom of the window and nothing moves. This project has already
    /// closed that defect once, in the panes.
    ///
    /// The two-sided sections scroll inside each pane, and the log and shell scroll their own
    /// output. Everything else was drawn straight into the panel with nothing to catch the
    /// overflow: a startup list plus its settings plus a pending diff is simply taller than a
    /// window, and the diff - the part somebody is being asked to review before writing - was
    /// the part that fell off the bottom.
    pub(crate) const fn scrolls_itself(self) -> bool {
        match self {
            // Two panes, each with its own scroll area.
            // Two panes each, and the log and shell each have one long output. All of them
            // already sit in a scroll area of their own.
            Self::Payloads
            | Self::Packages
            | Self::Titles
            | Self::Saves
            | Self::Cheats
            | Self::Filesystem
            | Self::Log
            | Self::Shell => true,
            Self::Check | Self::Stream | Self::Autoload | Self::System | Self::Controllers => false,
        }
    }

    /// Which kind of tracked list this section shows, if it has one.
    ///
    /// **Every section has one, including the two whose list is empty on purpose.** Saves
    /// ship empty because a save is signed for the target that wrote it, so a downloaded one
    /// is a file the target rejects. Titles ships open-source engines only - a list of urls
    /// for commercial games is a list of pirated games.
    /// Both still have two sides to compare and copy between - see `docs/DECISIONS.md`.
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
    /// **Declared, not checked here.** Every section asks the same check the `check` section
    /// shows, so there is one place that knows whether a service is answering and one place
    /// that knows what to do about it. A section that probed for itself would be a second
    /// opinion nobody asked for, and the two would disagree the moment one was refreshed.
    pub(crate) const fn requires(self) -> Option<&'static str> {
        match self {
            // Nothing: it *is* the asking.
            Self::Check | Self::Stream | Self::Controllers => None,

            Self::Log => Some("klogsrv"),
            // Both are the shell: one runs a command somebody typed, the other runs the
            // handful this asks on their behalf.
            Self::Shell | Self::System => Some("shsrv"),
            // Everything that reads or moves a file needs the file service - including
            // autoload, whose two files are fetched and written through it.
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
    /// # Why a path is sometimes a question rather than a constant
    ///
    /// Some directories are properties of the machine - `/user/app`, `/user/appmeta`,
    /// `/user/home`, `/data/pkg` - and were confirmed by listing them on a target. A constant
    /// is the right way to name one of those.
    ///
    /// Others are made by whichever payload is installed, and reading another tool's source
    /// code cannot settle them. Checking five paths taken from one such tool against a real
    /// target found **all five absent**: `/data/etaHEN`, `/data/garlic`, `/data/payloads`,
    /// `/data/AVATARS` and `/data/ps5_autoloader` exist only if you run the thing that makes
    /// them, and this target runs a different payload manager.
    ///
    /// **Cheats are the case where there is provably no single answer.** The most-used cheat
    /// runner documents three locations and says it reads all of them - its own, etaHEN's,
    /// and elf-arsenal's. None is *the* path; which applies depends on what somebody
    /// installed. So the section asks the target instead of asserting, and a target with
    /// none of them is told exactly that, rather than shown an empty listing of a directory
    /// that is not there.
    ///
    /// An empty list means the single path in [`Section::there`] is the answer.
    pub(crate) const fn looking_for(self) -> pros_core::places::Looking {
        use pros_core::places::Looking;
        match self {
            Self::Payloads => Looking::Payloads,
            Self::Titles => Looking::Titles,
            Self::Packages => Looking::Packages,
            Self::Cheats => Looking::Cheats,
            // Saves are per-account under `/user/home` and a stick holds none, so the chooser
            // offers the device root and claims nothing about what is in it. The rest have no
            // two-pane browser at all, so nothing asks them.
            Self::Saves
            | Self::Filesystem
            | Self::Check
            | Self::Stream
            | Self::Autoload
            | Self::System
            | Self::Controllers
            | Self::Log
            | Self::Shell => Looking::Anything,
        }
    }

    pub(crate) const fn candidates(self) -> &'static [Place] {
        match self {
            Self::Cheats => &[
                // The cheat runner's own, first: if it is installed, this is where it looks.
                Place {
                    path: "/data/cheatrunner/cheats",
                    label: "cheatrunner's own",
                    note: "the cheat runner's own folder, which it reads first",
                },
                // Then the two it reads for compatibility with other payloads.
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
            // **Packages are the same shape of question as cheats**, which is why they moved
            // here from a constant. Neither path is a property of the machine: both are made
            // by whichever upload tool somebody runs, so a target running something else has
            // neither.
            //
            // Measured on a target 2026-08-27: three real directories, **no symbolic link
            // between them** - so these are two places, not one place under two names. Both
            // held the same three packages, and `/data/homebrew` on its own held none. That
            // was the section's default, so it opened on a folder containing one subfolder
            // and no packages at all.
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
    /// **Measured against a target on 2026-08-26 and again on 2026-08-27**, not guessed. The
    /// boxes are still editable, because one target is one target. (D013)
    ///
    /// For a section with [`Self::candidates`], this is only the first of them - what is shown
    /// while the question of which one exists is still outstanding.
    pub(crate) const fn there(self) -> &'static str {
        match self {
            // Six titles were here, each a folder named exactly as an identifier.
            Self::Stream | Self::Titles => "/user/app",
            Self::Autoload => "/data/pldmgr",
            // Neither browses anything: one asks the shell, the other has nowhere to look
            // yet. The root is what a path has to be, not where either of them points.
            Self::System | Self::Controllers => "/",
            // Saves are further down, under a per-user folder: `/user/home/<user>/
            // savedata_prospero`. The starting point is the level above, because which
            // user is a question this project cannot answer for somebody.
            Self::Saves => "/user/home",
            // The first candidate, used only until the target has been asked. `/data/homebrew`
            // was here and was wrong: the packages are one level further down.
            Self::Packages => "/data/homebrew/pkg",
            // The manager keeps a folder per payload here.
            Self::Payloads => "/data/pldmgr/payloads",
            // The first candidate, used only until the target has been asked - see
            // `candidates`, which is where the real answer comes from.
            Self::Cheats => "/data/cheatrunner/cheats",
            _ => "/data",
        }
    }
}

/// A chain read off a target, being written down as a preset.
///
/// # Why the name is separate from the preset
///
/// Everything else here was measured off the target and does not change while somebody types.
/// The name is the one thing they choose, so it is held apart and put onto the preset when they
/// agree - which also means the panel can refuse a name without having anything to undo.
#[derive(Debug, Clone)]
pub(crate) struct Exporting {
    /// What to call it. One word, because a preset name goes in a whitespace-delimited file.
    pub(crate) name: String,
    /// The preset as measured, with the name not yet applied.
    pub(crate) preset: pros_core::recovery::baseline::Preset,
    /// What the export could not know, in its own words.
    pub(crate) notes: Vec<String>,
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

/// Which windows are open.
///
/// **Grouped because they are one kind of thing.** Loose flags on the state read as unrelated
/// switches; these two open a window and the others are notes the drawing loop leaves itself,
/// and the grouping is what says which is which.
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
    /// Kept beside [`State::after_transfers`] and applied at the same moment, for the same
    /// reason: the entries name files that the sends in the plan are what put in place.
    ///
    /// # Why more than one
    ///
    /// **A payload-manager chain is two lists and they are not alternatives.** The autoloader's
    /// list starts the manager; the manager then runs its own, at a path compiled into it. A
    /// deploy that wrote one of them left a target half configured, and which half depended on
    /// which entry happened to be selected in a dropdown.
    ///
    /// They are still reviewed one at a time. Each is a whole file replacing a whole file, and
    /// two diffs under one button is one of them going unread.
    pub(crate) rebuild: Vec<(String, Vec<String>)>,
    /// Which chain the configurator would build, by name.
    ///
    /// **A name rather than an index**, because the presets are read from a file somebody can
    /// edit between one run and the next - an index would quietly come to mean a different
    /// chain when they added one above it.
    pub(crate) preset: String,
    /// Which list the configurator is being pointed at, while somebody is choosing.
    ///
    /// `None` when it is not open. A separate choice from the list being *viewed*, because
    /// setting one up and reading another at the same time is a reasonable thing to do and
    /// conflating them would move somebody's view under them as they picked.
    pub(crate) setting_up: Option<usize>,
    /// List edits a plan has agreed but that cannot be made until its transfers land.
    ///
    /// # Why they wait
    ///
    /// **An entry may only name a file the manager can resolve**, so adding one is refused
    /// unless the payload is on internal storage. In a plan that is the *last* step and the
    /// send before it is what makes it true - so applying it at the moment somebody presses
    /// the button asks the question before the answer exists, and the edit is refused for a
    /// reason that is about to stop being true.
    ///
    /// That is exactly what happened: a plan reading *copy it off the USB, send it, list it*
    /// did the first two and silently dropped the third, leaving the finding it was answering
    /// still on screen and nothing to show for the button.
    pub(crate) after_transfers: Vec<pros_core::recovery::Fix>,
    /// Whether everything should be asked again about the target already selected.
    ///
    /// **Distinct from having no answers.** Both used to be said the same way - by forgetting
    /// which target had been surveyed - and the survey could not then tell *this is a different
    /// machine* from *ask this one again*. Only the first is a reason to take what is on screen
    /// away, and treating a refresh as the first is what blanked four panels every time a
    /// payload was sent.
    pub(crate) resurvey: bool,
    /// Which target the last check was about.
    pub(crate) checked_for: Option<String>,
    /// The target's listing of wherever the browser is looking.
    pub(crate) library: Vec<pros_core::library::Item>,
    /// Listings already fetched this session, by the path they are of.
    ///
    /// **One slot was shared by six sections.** Every two-sided screen browses into the same
    /// `library`, so moving between them re-fetched what had just been read - which looks,
    /// correctly, like a tool that keeps nothing.
    ///
    /// Emptied whenever a job says it disturbed the target, because a cached listing that
    /// outlived a copy or a delete is the stale-answer defect with a directory in it.
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
    /// Conventional and **unmeasured**, which is why it is a box a person can correct
    /// rather than a constant they cannot. (D007)
    pub(crate) install_dir: String,
    /// The log, as it arrives.
    ///
    /// Replaces a five-second sample: a window that reads for five seconds is a window that
    /// is not reading for the other five, and the line somebody is waiting for is the one
    /// that arrives in the gap.
    pub(crate) lines: Vec<String>,
    /// The last command's output.
    pub(crate) said: String,
    /// The last thing that went wrong.
    pub(crate) trouble: Option<String>,
    /// What the target said about where this section's things live, and **which section
    /// asked**.
    ///
    /// The section is not decoration. Without it the answer outlived the question: leaving the
    /// cheats section left its *none of these* notice on screen everywhere else, still worded
    /// as if it were about cheats while listing the stream section's candidate. An answer that
    /// survives the question it answered is worse than no answer.
    pub(crate) located: Option<(Section, pros_core::locate::Where)>,
    /// The two sides as one list, with what is ticked in it.
    pub(crate) listing: crate::listing::Listing,
    /// Whether to draw that list as one table rather than two panes.
    pub(crate) merged: bool,
    /// What this tool has done this session.
    ///
    /// **Not the log.** The log is the target's account of itself; this is this program's
    /// account of what it asked. Somebody debugging a failed send needs both and needs to know
    /// which is which.
    pub(crate) journal: crate::journal::Journal,
    /// The four pad slots, what drives each, and the key layout they share.
    pub(crate) pads: pros_link::pads::Pads,
    /// How many pad records have been built this session.
    ///
    /// **Counted rather than sent**, because nothing receives them yet. A number that climbs
    /// while keys are pressed is how somebody confirms the mapping works before there is
    /// anything to confirm it against.
    pub(crate) pad_records: u64,
    /// Where controller records go, when anywhere.
    pub(crate) feed: pros_link::feed::Feed,
    /// The port the input payload is expected on.
    ///
    /// Editable, because nothing has been measured about it - both ends are ours, and a
    /// number this project chose is a number somebody may need to change.
    pub(crate) feed_port: String,
    /// The stream coming back the other way, when one is.
    ///
    /// **Owned here and not by the panel**, so switching sections and coming back does not
    /// end a stream somebody is watching - and so the panel draws from a snapshot of what the
    /// pump has counted rather than from anything it did itself.
    pub(crate) watching: pros_core::watch::Watching,
    /// The port the video payload is expected on. Editable, for the same reason.
    pub(crate) watch_port: String,
    /// Which slot the pending binding belongs to.
    ///
    /// **Held beside the button**, because a rebinding is a change to one slot's layout and
    /// applying it to whichever slot happens to be on screen would put one player's key on
    /// another player's pad.
    pub(crate) binding_slot: Option<u8>,
    /// Which button is waiting to be bound to the next key pressed.
    ///
    /// **Held here rather than in the panel** because it survives a repaint, and a rebinding
    /// that forgot itself between frames would be one nobody could complete.
    pub(crate) binding: Option<pros_link::pad::Button>,
    /// Groups the person has folded away in the payloads table.
    ///
    /// **What is folded rather than what is open**, so a group that appears later - a category
    /// the repository grows - starts open like every other one rather than hidden.
    pub(crate) folded: std::collections::BTreeSet<String>,
    /// What the target is, once asked.
    pub(crate) system: Option<pros_core::system::Report>,
    /// Every payload file on the target, once looked for.
    ///
    /// **`None` until asked**, which is not the same as none found. A startup list checked
    /// against a set nobody has read would mark every entry missing.
    pub(crate) payloads_there: Option<Vec<pros_core::payloads::There>>,
    /// A file somebody dropped that nothing describes.
    ///
    /// **Held rather than refused.** Something just built has no publisher and no digest, and
    /// asking for one in order to run it once is asking somebody to describe what they are
    /// about to throw away.
    pub(crate) adhoc: Option<PathBuf>,
    /// A destructive action waiting to be confirmed, and what it would act on.
    ///
    /// **Held rather than done.** Everything else here is recoverable by doing it again;
    /// this is not, so it goes through a panel naming each thing that would go.
    pub(crate) pending_delete: Option<(crate::listing::Offer, Vec<crate::listing::Entry>)>,
    /// The startup list, once read, with any edits not yet written.
    pub(crate) boot: Option<pros_core::boot::Boot>,
    /// Which row of it is selected.
    ///
    /// **One, not many.** The actions move a single step; several at once would have an order
    /// of their own that nobody stated.
    pub(crate) boot_at: Option<usize>,
    /// The payload manager's settings, once read.
    pub(crate) settings: Option<pros_core::autoload::Settings>,
    /// An edit to those settings that has not been written.
    pub(crate) pending_change: Option<pros_core::autoload::Change>,
    /// Packages on this machine waiting for somebody to confirm installing them.
    ///
    /// **A list, because the toolbar has a multi-select.** Holding one meant a selection of
    /// four confirmed and installed the first, which is the same defect as the one the queue
    /// below was written to fix.
    pub(crate) pending_install: Option<Vec<PathBuf>>,
    /// Which target the log was last started for, so a refusal is not retried every frame.
    pub(crate) followed_for: Option<String>,
    /// Every startup list the loaded chains declare.
    ///
    /// **Read once, at startup.** Working it out parses the shipped chains and somebody's own
    /// file, and drawing a menu is not a reason to read a file from disk every frame. A chain
    /// added while the window is open is picked up the next time it starts, which is the same
    /// rule the chains themselves have always had.
    pub(crate) lists: Vec<pros_core::chain::Held>,
    /// Which startup list the autoload screen is showing, into [`Self::lists`].
    pub(crate) list_at: usize,
    /// A chain read off a target, waiting to be written down as a preset.
    ///
    /// **Built once, when the button is pressed.** Rebuilding it per frame would read the
    /// presets file off disk once per entry per frame to find out what already explains each
    /// one, and the answer would be the same every time.
    pub(crate) exporting: Option<Exporting>,
    /// What to keep on the log screen, if anything.
    ///
    /// **Filters the view, never the record.** Every line that arrived is kept, so clearing
    /// the box shows them all again - a filter that discarded what it hid would quietly turn
    /// a diagnostic tool into a lossy one.
    pub(crate) log_filter: String,
    /// A doctor's plan that has been shown to somebody and not yet agreed to.
    ///
    /// **Nothing here is carried out until this is emptied by a press.** It is the whole of the
    /// promise that this program suggests and never acts: a plan reaches the queue at exactly
    /// one place, and that place is behind a button.
    pub(crate) pending_plan: Option<Pending>,
    /// Which finding a plan was carried out for, while it is still being carried out.
    ///
    /// **Kept so a fix can be checked rather than assumed.** Every job reports whether it
    /// worked, and none of them can say whether the *finding* is answered - only asking the
    /// target again can. This is what that answer gets matched back to.
    pub(crate) fixing: Option<String>,
    /// Where the two panes are split, as the left one share of the usable width.
    ///
    /// **A fraction rather than a pixel count**, so the split survives the window being
    /// resized. One stored in pixels creeps towards an edge every time the window shrinks,
    /// and the pane it was protecting is the one that disappears.
    pub(crate) split: f32,
    /// Jobs behind the one that is running.
    ///
    /// **Not a second scheduler - a line in front of the first one.** The rule that one job
    /// runs at a time is unchanged; what changes is what happens to the second thing somebody
    /// asked for. It used to be dropped: a selection of four and one press of *send* started
    /// one file, unticked it, and said nothing about the other three.
    ///
    /// Nothing here decides *when* to run. [`Self::finish`] takes the next one when the
    /// previous ends, and that is the whole of it.
    pub(crate) queued: std::collections::VecDeque<Job>,
    /// A copy that was not attempted, and what it would need.
    pub(crate) refused: Option<pros_core::origin::Needs>,
    /// What the last finished job may have made untrue.
    ///
    /// **Set here and acted on by the window**, so the state machine keeps owning what is
    /// displayed rather than the worker reaching into it.
    pub(crate) disturbed: Vec<Disturbs>,
    /// What a person has typed into the command box.
    pub(crate) command: String,
    /// What a person has typed into the address box.
    pub(crate) address: String,
    /// What a person has typed into the name box.
    pub(crate) name: String,
}

impl State {
    /// A window that has just opened, with whatever is registered.
    #[must_use]
    pub(crate) fn new(targets: Vec<Target>) -> Self {
        Self {
            chosen: (!targets.is_empty()).then_some(0),
            targets,
            // Conventional, and unmeasured by this project. The window says so beside the
            // box, and the box is editable, which is the whole handling this deserves.
            library_path: "/user/app".to_owned(),
            // **Where the payload manager actually looks.** It defaulted to `/data/payloads`,
            // which is on the console's drive and is not one of the two directories
            // `payload_mgr_resolve_path` searches - so the send button's own default put
            // payloads somewhere nothing could ever autoload them from, and the check screen
            // then reported them missing. The doctor's plans have always said `INTERNAL`; this
            // is the window agreeing with them.
            preset: pros_core::recovery::baseline::first().name,
            // **Read here, once.** Every startup list on offer comes from the chains - the ones
            // this repository ships and the ones somebody has added beside the registry - so a
            // path is on the screen because a file said so rather than because this program
            // was built believing it.
            lists: pros_core::chain::lists(),
            install_dir: pros_core::payloads::INTERNAL.to_owned(),
            name: "ps5".to_owned(),
            // Chosen by this project rather than measured, so it is filled in rather than
            // left blank - a box somebody has to guess the contents of is worse than one
            // holding an answer they can change.
            feed_port: pros_link::feed::PORT.to_string(),
            watching: pros_core::watch::Watching::idle(),
            // Even, until somebody drags it.
            split: 0.5,
            watch_port: pros_core::watch::PORT.to_string(),
            ..Self::default()
        }
    }

    /// The target being acted on.
    #[must_use]
    pub(crate) fn target(&self) -> Option<&Target> {
        self.chosen.and_then(|which| self.targets.get(which))
    }

    /// The log lines the filter keeps, in order.
    pub(crate) fn kept_lines(&self) -> impl Iterator<Item = &String> {
        let wanted = self.log_filter.trim().to_lowercase();
        self.lines
            .iter()
            .filter(move |line| wanted.is_empty() || line.to_lowercase().contains(&wanted))
    }

    /// The startup list currently being shown.
    pub(crate) fn list(&self) -> pros_core::chain::Held {
        self.lists
            .get(self.list_at)
            .or_else(|| self.lists.first())
            .cloned()
            // Only reachable before the lists have been read, which is before anything can be
            // selected. `chain::lists` guarantees at least one entry afterwards.
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
    /// Answers whether it was started. **Refusing while busy is the point**: the work
    /// happens on one thread away from the drawing, and two jobs racing would interleave
    /// their answers into a window showing half of each.
    pub(crate) fn begin(&mut self, job: Job) -> bool {
        if self.waiting.is_some() {
            return false;
        }
        self.trouble = None;
        self.progress = None;
        // Recorded as it starts, not as it finishes: a job that never comes back should still
        // appear in the record, as one that never came back.
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
    /// **What a multi-select needs.** [`Self::begin`] answers *no* when busy, and every caller
    /// that acted on a selection took that as "stop" - so the second and later items of a
    /// selection were silently discarded. This is the same rule with the discarding removed.
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
    /// # Why only the first one
    ///
    /// **Once there is something to show, a re-read leaves it on screen.** A panel that emptied
    /// itself every time it asked again would flicker through blank on every refresh, and worse,
    /// would take away the very thing somebody was reading in order to tell them it was
    /// fetching it again. What is on screen is a measurement from a moment ago, which is what
    /// it always was.
    ///
    /// So this is true only when there is **nothing yet** and something is on its way. Both
    /// halves matter: nothing yet with nothing coming is a screen nobody has asked about, which
    /// is a different sentence and a different thing for somebody to do about it.
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
            // Saves land in the same listing as the rest: asking where they are is a
            // navigation, and what comes back is a directory like any other.
            Section::Saves
            | Section::Filesystem
            | Section::Titles
            | Section::Cheats
            | Section::Packages => self.library.is_empty(),
            // A log, a shell, a stream and the controllers have nothing to fetch before they
            // can be used: they are all things somebody starts.
            Section::Log | Section::Shell | Section::Stream | Section::Controllers => false,
        }
    }

    /// Whether an answer this screen needs is running or waiting its turn.
    ///
    /// **The queue counts, not just the running one.** The survey on arrival queues four jobs
    /// behind one another, so a screen whose answer is third would otherwise say *nobody asked*
    /// for as long as the first two take - which is the one thing it must not say while its
    /// answer is on the way.
    fn expecting(&self, section: Section) -> bool {
        let wanted = |job: &Job| job.fills().contains(&section);
        self.waiting
            .as_ref()
            .is_some_and(|running| wanted(&running.job))
            || self.queued.iter().any(wanted)
    }

    /// Whether a screen already showing something is being read again.
    ///
    /// Said quietly beside what is on screen, rather than instead of it: the difference between
    /// *this is a moment old* and *this is being checked right now* is worth a word and is not
    /// worth taking the panel away.
    pub(crate) fn re_reading(&self, section: Section) -> bool {
        !self.nothing_yet(section) && self.expecting(section)
    }

    /// Forgets everything not yet started.
    ///
    /// **The running one is not touched.** Stopping work that is already moving files is a
    /// different promise, and this project does not make it.
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
            | Job::FindSaves(target)
            | Job::Locate(target, _)
            | Job::Launch(target, _)
            | Job::RunThere(target, _)
            | Job::ReadList(target, _)
            | Job::ReadAutoload(target)
            | Job::ReadSystem(target)
            | Job::InstallPackage(target, _)
            | Job::FindPayloads(target, _)
            | Job::DeleteThere(target, _)
            | Job::WriteAutoload(target, ..) => Some(&target.name),
            // Between this machine and a mirror. No target is involved, and saying one was
            // would put a fetch in the record under a machine that had nothing to do with it.
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
            // A copy that was asked to stop is not a copy that finished, and the record
            // should not read as though four hundred files went across when forty did.
            Done::Copied(summary, _)
                if summary
                    .skipped
                    .iter()
                    .any(|one| one.why.contains("stopped")) =>
            {
                Ending::Stopped
            }
            Done::Copied(summary, into) => Ending::Done(format!(
                "{} files, {} bytes to {into}{}",
                summary.files,
                summary.bytes,
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
            Done::System(report) => Ending::Done(format!("{} facts", report.facts.len())),
            Done::Said(_) | Done::FoundSaves(_) => Ending::Done(String::new()),
        }
    }

    /// **Beginning a job leaves it waiting to be handed over.**
    ///
    /// Pinned because the window's own loop got this wrong once, and the symptom was a
    /// clock ticking beside a job nobody had started.
    #[cfg(test)]
    fn pending_after_begin(&self) -> bool {
        self.pending.is_some()
    }

    /// Takes the result of the job that was running.
    ///
    /// A failure clears the panel the job would have filled, so nothing stale is left on
    /// screen looking current.
    pub(crate) fn finish(&mut self, done: Done) {
        let Some(waiting) = self.waiting.take() else {
            return;
        };
        self.journal.ended(Self::how_it_went(&done));
        // **Recorded whatever the outcome.** A copy that failed part way still moved files,
        // and a listing that was right before it is not right after it.
        self.disturbed = waiting.job.disturbs().to_vec();
        match done {
            Done::Checked(report, chain) => {
                self.report = Some(*report);
                self.chain = chain;
            }
            // Handed up rather than kept: the payload list belongs to the window, and this
            // state machine owning a copy of it would be a second answer to what it says.
            Done::Relisted(payload, found) => self.relisted = Some((*payload, *found)),
            Done::Browsed(items) => {
                if let Job::Browse(_, where_) = &waiting.job {
                    self.seen.insert(where_.clone(), items.clone());
                }
                self.library = items;
            }
            Done::FoundSaves(found) => match found {
                pros_core::saves::Found::Here(path) => self.go_to = Some(path),
                // Named rather than chosen between, and put where trouble goes so it is
                // read: a target with two accounts has two people's saves on it.
                pros_core::saves::Found::Several(users) => {
                    self.trouble = Some(format!(
                        "several users, so this does not choose: {}",
                        users.join(", ")
                    ));
                }
                pros_core::saves::Found::None => {
                    self.trouble =
                        Some(format!("no user folders under {}", pros_core::saves::HOME));
                }
            },
            Done::Named(found) => {
                for about in found {
                    if let Some(name) = about.name {
                        self.names.insert(about.id, name);
                    }
                }
            }
            Done::Copied(summary, where_to) => {
                // **An incomplete copy is trouble, not news.** A backup that quietly missed
                // a file is trusted at the moment it matters, so it goes where failures go
                // and says how many.
                self.said = format!(
                    "{} files, {} bytes -> {where_to}",
                    summary.files, summary.bytes
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
            Done::Payloads(found) => self.payloads_there = Some(found),
            Done::Launched(said) => self.said = said.describe(),
            Done::RanThere(said) => self.said = said.describe(),
            Done::List(boot) => {
                // The list only: a settings file belongs to the manager and this may not be
                // the manager list at all.
                self.boot = Some(*boot);
                self.boot_at = None;
            }
            Done::Autoload(settings, boot) => {
                self.settings = Some(*settings);
                self.boot = Some(*boot);
                self.boot_at = None;
            }
            Done::System(report) => self.system = Some(*report),
            Done::Installed(said) => {
                // A known failure is trouble; anything else is only what was said. Putting
                // an unrecognised answer in the failure slot would claim knowledge this
                // project does not have, in the direction that stops somebody looking.
                if said.is_a_known_failure() {
                    self.trouble = Some(said.describe());
                } else {
                    self.said = said.describe();
                }
            }
            Done::Located(found) => {
                self.located = Some((self.section, found.clone()));
                // Only moved when there is somewhere to move to. Pointing the box at a
                // directory the target has just said it does not have would make an empty
                // listing that looks exactly like an installed tool with nothing in it.
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

    /// Starts whatever is waiting, unless the last one gave somebody something to read.
    ///
    /// # Why trouble stops the line
    ///
    /// [`Self::begin`] clears `trouble` and `progress`, because a job starting means the last
    /// message is about to be replaced. Run a queue through that and the third item wipes the
    /// message explaining why the second failed - so a send of four files that lost one would
    /// end showing a success, and the loss would exist nowhere on screen. **The queue would
    /// have become a way of hiding failures rather than a way of doing more work.**
    ///
    /// So anything that produced trouble - a failure, or a copy that finished with files
    /// missing - stops the rest, and what stopped is counted and said. Pressing the button
    /// again is how somebody who has read it carries on.
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
            // The listing goes, and so does the trail that led to it: a path that could not
            // be opened must not stay in the breadcrumb as though it had been.
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

    /// **A job that has been begun is waiting to be run.**
    ///
    /// The window said *checking* for half a minute with nothing checking, because the
    /// handover depended on when in the frame the job happened to be begun.
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

    /// Two jobs racing would interleave their answers into one window.
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

    /// **A refresh that failed must not leave the previous answer on screen.**
    ///
    /// A report shown after a check that could not complete is a claim about a target that
    /// has stopped being true - the same mistake as caching a capability, made in pixels.
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

    /// A failure clears its own panel and leaves the others alone: a listing that failed
    /// says nothing about whether the last check was true.
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

    /// Starting something clears the last complaint, so an old error is not read as new.
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

    /// What is being waited for is said specifically, because *working* is not an answer.
    #[test]
    fn waiting_says_which_thing_it_is_waiting_for() {
        assert_eq!(Job::Check(target("desk")).describe(), "checking desk");
        assert_eq!(
            Job::Browse(target("desk"), "/data/pldmgr".to_owned()).describe(),
            "opening /data/pldmgr"
        );
    }

    /// A window that opens with nothing registered selects nothing, rather than pretending.
    #[test]
    fn nothing_registered_means_nothing_chosen() {
        let state = State::new(Vec::new());
        assert!(state.target().is_none());
        assert_eq!(State::new(vec![target("only")]).chosen, Some(0));
    }

    /// **A refusal is not a failure, and does not clear a panel.**
    ///
    /// Nothing went wrong: the copy was decided against before anything moved. Treating it as
    /// a failure would wipe the listing somebody is looking at, which is the behaviour meant
    /// for a job that half-happened.
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

    /// **An answer about one section does not show under another.**
    ///
    /// Leaving the cheats section used to leave its *none of these* notice on screen
    /// everywhere else - still worded as being about cheats, while listing the stream
    /// section's candidate directory. Nothing had gone wrong; the answer simply outlived the
    /// question, which is this project's recurring defect wearing a new hat.
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

    /// **Running a payload makes the check stale, and the job says so.**
    ///
    /// The bug this exists to stop: sending klogsrv from the log screen left the check screen
    /// showing it red until somebody pressed check themselves. Nothing was lying - the screen
    /// was reporting a measurement that had stopped being true, which is this project's own
    /// defect wearing a window.
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

    /// **A check disturbs nothing.** It is the reading, and saying otherwise would have it
    /// ask again forever.
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

    /// **What was disturbed is recorded even when the job failed.**
    ///
    /// A copy that stopped part way still moved files, and a listing that was right before it
    /// is not right after it. Recording only on success would leave the window most wrong
    /// exactly when something had gone wrong.
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

    /// A command can do anything, so it is assumed to have. The cost is one listing read;
    /// the alternative is a window quietly disagreeing with a target somebody just changed.
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

    /// **The first starts and the rest wait**, rather than the rest being dropped.
    ///
    /// This is the reported bug: four ticked, *send* pressed once, one file sent and nothing
    /// said about the other three.
    #[test]
    fn a_selection_of_four_asks_for_four() {
        let mut state = state();
        for which in 0..4 {
            state.queue(push(which));
        }
        assert!(!state.is_idle(), "the first one runs");
        assert_eq!(state.queued(), 3, "the rest are waiting, not gone");
    }

    /// Each finish takes the next, until there is none.
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

    /// **A failure stops the line and says how much it stopped.**
    ///
    /// Without this the next job's start would clear the message explaining the failure, and a
    /// send of four that lost one would end on screen looking like a send of four that worked.
    /// That would make the queue a way of hiding failures.
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

    /// A copy that finished with files missing is trouble too, and stops the line for the
    /// same reason - the count of what it missed must not be overwritten.
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

    /// Clearing the queue leaves what is running alone. **Stopping work already moving files
    /// is a different promise**, and one this does not make.
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

    /// **Every section either scrolls itself or gets one wrapped around it.**
    ///
    /// Pinned by exhaustive match rather than by a list here, so a section added later has to
    /// answer the question rather than default to whichever is wrong for it. The failure it
    /// guards against is silent: content simply runs off the bottom of the window.
    #[test]
    fn every_section_answers_whether_it_scrolls() {
        for (_, sections) in Section::GROUPS {
            for section in sections {
                // The call itself is the assertion: a new variant fails to compile until it
                // is classified, which is the only way this stays true.
                let _ = section.scrolls_itself();
            }
        }
    }

    /// The two-sided sections scroll inside their panes, so wrapping them would stop those
    /// panes scrolling at all.
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

    /// **The autoload screen does not**, which is why a startup list, its settings and a
    /// pending diff together ran off the bottom of the window with nothing to reach them.
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

    /// **A listing read once is kept**, so six sections sharing one slot do not re-fetch what
    /// was just fetched every time somebody moves between them.
    #[test]
    fn a_listing_is_remembered_by_its_path() {
        let mut state = State::new(vec![target()]);
        state.begin(Job::Browse(target(), "/data/pkg".to_owned()));
        state.finish(Done::Browsed(listing()));
        assert_eq!(state.seen.get("/data/pkg").map(Vec::len), Some(1));
    }

    /// **And forgotten the moment something changed the target.** A kept listing that outlived
    /// a copy or a delete is this project's recurring defect with a directory in it.
    #[test]
    fn what_changed_the_target_is_not_remembered_from_before() {
        let mut state = State::new(vec![target()]);
        state.begin(Job::Browse(target(), "/data/pkg".to_owned()));
        state.finish(Done::Browsed(listing()));
        assert!(!state.seen.is_empty());

        // A push says it disturbed the target; the window clears the cache on that signal.
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
