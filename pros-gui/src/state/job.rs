//! The jobs the worker runs, what each one produced, and what it may have changed.

use std::path::PathBuf;

use pros_core::check::Report;
use pros_core::target::Target;

use super::{Place, Section};

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
    pub(super) const fn replaces(&self) -> Panel {
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
pub(super) enum Panel {
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
