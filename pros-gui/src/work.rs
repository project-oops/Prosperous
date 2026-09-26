//! Doing the asking somewhere other than the drawing thread.
//!
//! Every job is a network round trip to a target that may be switched off, so it runs on its
//! own thread and the answer arrives on a channel the drawing thread collects between frames;
//! otherwise the window stops repainting and looks crashed. One thread per job, not a pool,
//! because [`crate::state::State::begin`] allows only one job at a time.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread;

use pros_core::check::check_declaring;
use pros_link::files;

use pros_core::transfer::Progress;

use crate::state::{Done, Job};

/// How long a check waits on any one port.
const PATIENCE: std::time::Duration = std::time::Duration::from_millis(1500);

/// How long a shell command is given to go quiet.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(1200);

/// How long an install is given.
///
/// Longer than a command because the target reads and unpacks the package before answering.
const UNPACKING: std::time::Duration = std::time::Duration::from_secs(20);

/// What comes back from a running job.
///
/// An answer ends the job and replaces a panel; progress ends nothing.
#[derive(Debug)]
pub(crate) enum Update {
    /// A long job is part way through.
    Progress(Progress),
    /// The job finished, one way or the other.
    Finished(Done),
}

/// The thread that answers, and the channel it answers on.
#[derive(Debug)]
pub(crate) struct Worker {
    answers: Receiver<Update>,
    sender: Sender<Update>,
    /// Set to ask whatever is running to stop.
    ///
    /// A shared flag rather than a message: the channel runs the other way, and the worker
    /// polls this from inside a copy.
    stopping: Arc<AtomicBool>,
}

impl Default for Worker {
    fn default() -> Self {
        Self::new()
    }
}

impl Worker {
    /// A worker with nothing running.
    #[must_use]
    pub(crate) fn new() -> Self {
        let (sender, answers) = channel();
        Self {
            answers,
            sender,
            stopping: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Asks whatever is running to stop.
    ///
    /// Asks rather than kills, so a copy finishes the file it is on instead of leaving a
    /// partial one, and its summary lists what it did not do.
    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
    }

    /// Starts a job on its own thread.
    pub(crate) fn start(&self, job: Job) {
        let sender = self.sender.clone();
        // Cleared at start, not at the end of the last job, so a late stop cannot cancel
        // the next job.
        self.stopping.store(false, Ordering::Relaxed);
        let stopping = Arc::clone(&self.stopping);
        thread::spawn(move || {
            let reporting = sender.clone();
            let done = perform(
                &job,
                &mut move |progress| {
                    let _ = reporting.send(Update::Progress(progress.clone()));
                },
                &move || stopping.load(Ordering::Relaxed),
            );
            // Fails only if the window has gone.
            let _ = sender.send(Update::Finished(done));
        });
    }

    /// Takes an update if one has arrived, without waiting for one.
    #[must_use]
    pub(crate) fn collect(&self) -> Option<Update> {
        // Empty and disconnected both mean no answer this frame; the channel cannot close
        // while this holds a sender.
        self.answers.try_recv().ok()
    }
}

/// Does the thing, and turns any failure into words.
///
/// Every error becomes a sentence in the library's own wording, because the window has one
/// place to show trouble.
fn perform(job: &Job, watch: &mut dyn FnMut(&Progress), stop: &dyn Fn() -> bool) -> Done {
    match job {
        Job::Check(target) => {
            // Read at each check, so an edited list takes effect without a restart.
            let described = pros_core::manifest::Tracked::Payloads
                .read()
                .unwrap_or_else(|_| pros_core::manifest::recommended());
            let report = check_declaring(target, &described, PATIENCE);
            // The boot list answers what runs after a reboot. Unreadable is `None`, reported
            // as unknown rather than as an empty list.
            let chain = pros_core::chain::Chain::read(&target.link()).ok();
            Done::Checked(Box::new(report), chain)
        }
        Job::Shell(target, command) => {
            match pros_link::shell::run(&target.link(), command, SETTLE) {
                Ok(text) if text.trim().is_empty() => {
                    Done::Said("no output - is the shell loaded? a check will say".to_owned())
                }
                Ok(text) => Done::Said(text),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        Job::Pull(target, path, into) => match files::retrieve(&target.link(), path) {
            Ok(bytes) => match std::fs::write(into, &bytes) {
                Ok(()) => Done::Pulled {
                    into: into.clone(),
                    bytes: bytes.len(),
                },
                Err(why) => Done::Failed(format!("fetched it, then could not write it: {why}")),
            },
            Err(why) => Done::Failed(why.to_string()),
        },
        Job::Browse(target, path) => match files::list(&target.link(), path) {
            Ok(entries) => Done::Browsed(pros_core::library::scan(&entries)),
            Err(why) => Done::Failed(why.to_string()),
        },
        Job::Install(target, payload, from, to) => install_payload(target, payload, from, to),
        Job::Push(target, from, to) => match std::fs::read(from) {
            Ok(bytes) => match files::store(&target.link(), to, &bytes) {
                Ok(()) => Done::Said(format!("{} bytes copied to {to}", bytes.len())),
                Err(why) => Done::Failed(why.to_string()),
            },
            Err(why) => Done::Failed(format!("could not read {}: {why}", from.display())),
        },
        other => copying(other, watch, stop),
    }
}

/// Lays a payload out on the target the way the payload manager expects to find one.
///
/// Three writes: the folder (`payload_mgr_resolve_path` looks for `<dir>/<name>/<file>`), the
/// ELF, and the `.json` sidecar beside it. The ELF carries no version string, so the sidecar is
/// the only record of which build is installed. An existing folder is not a failure.
fn install_payload(
    target: &pros_core::target::Target,
    payload: &pros_core::manifest::Payload,
    from: &std::path::Path,
    to: &str,
) -> Done {
    let Ok(bytes) = std::fs::read(from) else {
        return Done::Failed(format!("could not read {}", from.display()));
    };
    let file = payload
        .filename
        .clone()
        .unwrap_or_else(|| format!("{}.elf", payload.name));
    let folder = format!("{to}/{}", payload.name);

    let link = target.link();
    let mut session = match files::Session::open(&link) {
        Ok(session) => session,
        Err(why) => return Done::Failed(why.to_string()),
    };
    if let Err(why) = session.make_directory(&folder) {
        return Done::Failed(format!("could not make {folder}: {why}"));
    }
    let at = format!("{folder}/{file}");
    if let Err(why) = session.store(&at, &bytes) {
        return Done::Failed(format!("could not write {at}: {why}"));
    }
    // A sidecar is written only when the list states a version; none is invented.
    let mut said = format!("{} bytes written to {at}", bytes.len());
    if payload.version.is_some() {
        match pros_core::payloads::sidecar_for(payload) {
            Ok(about) => {
                let beside = format!("{at}.json");
                if let Err(why) = session.store(&beside, &about) {
                    // Not a failure: the payload is in place and loads; only the sidecar is
                    // missing.
                    let _ = write!(said, " - but {beside} was not written: {why}");
                } else {
                    let _ = write!(
                        said,
                        ", and {} recorded beside it",
                        payload.version.as_deref().unwrap_or("its version")
                    );
                }
            }
            Err(why) => {
                let _ = write!(said, " - its description could not be written: {why}");
            }
        }
    } else {
        said.push_str(
            " - the list states no version, so nothing on the target will say which build this is",
        );
    }
    Done::Said(said)
}

/// Copying a folder off the target, and recording where it came from.
///
/// The origin stamp is written here because the source path, which names the account a save
/// belongs to, is only in hand during the copy.
fn backing_up(
    target: &pros_core::target::Target,
    from: &str,
    into: &std::path::Path,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> Done {
    let mut session = match files::Session::open(&target.link()) {
        Ok(session) => session,
        Err(why) => return Done::Failed(why.to_string()),
    };
    let done = pros_core::transfer::download(&mut session, from, into, watch, stop);
    session.close();

    let summary = match done {
        Ok(summary) => summary,
        Err(why) => return Done::Failed(why),
    };
    // Most saves carry no parameter file naming their account, so the path is the only record.
    if let Some(user) = pros_core::origin::user_in(from) {
        let record = pros_core::origin::Origin {
            target: target.name.clone(),
            address: target.address.clone(),
            user,
            from: from.to_owned(),
            when: pros_core::origin::now(),
        };
        // A failed stamp does not fail the backup.
        let _ = pros_core::origin::stamp(into, &record);
    }
    Done::Copied(Box::new(summary), into.display().to_string())
}

/// Putting a folder back, having first decided whether it may go.
///
/// The only job in this module that can decline, before anything is copied.
fn restoring(
    target: &pros_core::target::Target,
    from: &std::path::Path,
    to: &str,
    anyway: bool,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> Done {
    // A save going to an account that did not write it needs decrypting and re-signing; copied
    // as is, it lands cleanly and the target later refuses it.
    if !anyway && pros_core::origin::user_in(to).is_some() {
        let here = pros_core::saves::account_on(&target.link());
        let needs = pros_core::origin::needed(from, to, here.as_deref());
        if !needs.is_plain() {
            return Done::Refused(needs);
        }
    }
    if !anyway && let Some(refusal) = pros_core::guard::check(from, to) {
        return Done::GuardRefused(refusal);
    }

    // The same restore `pros restore` and `pros probe` run. Unchanged files are skipped; the
    // window has no force-all toggle, `pros restore --all` resends everything.
    match pros_core::transfer::restore(
        target,
        from,
        to,
        pros_core::transfer::Resend::OnlyChanged,
        watch,
        stop,
    ) {
        Ok(restored) => Done::Copied(Box::new(restored.summary), to.to_owned()),
        Err(why) => Done::Failed(why),
    }
}

/// The jobs `perform` does not handle itself.
fn copying(job: &Job, watch: &mut dyn FnMut(&Progress), stop: &dyn Fn() -> bool) -> Done {
    match job {
        Job::Backup(target, from, into) => backing_up(target, from, into, watch, stop),
        Job::Restore(target, from, to, anyway) => restoring(target, from, to, *anyway, watch, stop),
        Job::Names(target, ids) => {
            // A title that does not answer is left out rather than given an empty name; its
            // identifier already stands for it.
            let found = ids
                .iter()
                .filter_map(|id| pros_core::titles::read(&target.link(), id).ok())
                .collect();
            Done::Named(found)
        }
        Job::ReadAutoload(_)
        | Job::WriteAutoload(..)
        | Job::EnableAutoload(_)
        | Job::CaptureConfig(_)
        | Job::PlaceFile(..) => settings(job),
        Job::ReadSystem(target) => asking(&target.link()),
        Job::RestartUi(target) => restart_ui(&target.link()),
        Job::CloseTitle(target, id) => close_title(&target.link(), id),
        Job::EndProcess(target, pid) => end_process(&target.link(), pid),
        Job::Launch(target, id) => {
            match pros_link::shell::run(&target.link(), &pros_core::launch::command(id), SETTLE) {
                Ok(said) => Done::Launched(pros_core::launch::read(&said)),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        Job::RunThere(target, path) => {
            // The target's shell cannot quote, so it would run only the first word.
            if !pros_core::hbldr::is_one_argument(path) {
                return Done::Failed(format!(
                    "{path} has a space in it, and the target's shell has no way to quote one"
                ));
            }
            match pros_link::shell::run(&target.link(), &pros_core::hbldr::command(path), SETTLE) {
                Ok(said) => Done::RanThere(pros_core::hbldr::read(&said)),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        Job::ReadList(target, held) => {
            match files::retrieve(&target.link(), &held.path) {
                Ok(bytes) => Done::List(Box::new(pros_core::boot::Boot::parse(
                    &String::from_utf8_lossy(&bytes),
                ))),
                // Unreadable is not empty. An absent autoloader list is normal where the
                // manager is auto-launched.
                Err(why) => Done::Failed(format!("{}: {why}", held.path)),
            }
        }
        Job::FindPayloads(target, root) => {
            // Every place the manager lists, not just `root`: a payload on a USB stick outside
            // its folder there is listed but never autoloaded, and the tag says which.
            let _ = root;
            match pros_core::payloads::on_target_everywhere(&target.link()) {
                Ok(found) => Done::Payloads(found),
                Err(why) => Done::Failed(why),
            }
        }
        Job::DeleteThere(target, what) => removing(&target.link(), what),
        Job::DeleteHere(what) => erasing(what),
        Job::InstallPackage(target, file) => installing(&target.link(), file),
        Job::Locate(target, candidates) => {
            // Only the paths; the labels are the window's.
            let paths: Vec<&str> = candidates.iter().map(|place| place.path).collect();
            match pros_core::locate::first_of(&target.link(), &paths) {
                Ok(found) => Done::Located(found),
                Err(why) => Done::Failed(why),
            }
        }
        Job::Titles(target) => match pros_core::probe::installed(&target.link()) {
            Ok(found) => Done::Titles(found),
            Err(why) => Done::Failed(why),
        },
        Job::FindSaves(target) => match pros_core::saves::find(&target.link()) {
            Ok(found) => Done::FoundSaves(found),
            Err(why) => Done::Failed(why),
        },
        Job::Fetch(payload, dir) => match dir.as_ref().map_or_else(
            || pros_core::fetch::fetch(payload),
            |dir| pros_core::fetch::fetch_into(payload, dir),
        ) {
            Ok(into) => Done::Fetched(payload.name.clone(), into),
            Err(why) => Done::Failed(why.to_string()),
        },
        Job::Relist(payload) => match pros_core::sources::relist(payload) {
            Ok((now, found)) => Done::Relisted(Box::new(now), Box::new(found)),
            Err(why) => Done::Failed(why),
        },
        Job::Send(target, name, from) => match std::fs::read(from) {
            // The library checks the payload's shape before sending.
            Ok(payload) => match pros_link::loader::send(
                &target.link(),
                &payload,
                std::time::Duration::from_secs(4),
            ) {
                Ok(said) if said.trim().is_empty() => Done::Said(format!(
                    "{name} sent - nothing came back on the socket, which is not failure: \
                     only a payload launched this way reports here at all"
                )),
                Ok(said) => Done::Said(said),
                Err(why) => Done::Failed(why.to_string()),
            },
            Err(why) => Done::Failed(format!("could not read the staged payload: {why}")),
        },
        _ => Done::Failed("unreachable: every job is handled above".to_owned()),
    }
}

/// Reading and writing the payload manager's settings.
///
/// Kept apart because it replaces files on the target.
fn settings(job: &Job) -> Done {
    match job {
        Job::ReadAutoload(target) => {
            // Both files in one job, so the panel never shows one without the other.
            let settings = match files::retrieve(&target.link(), pros_core::autoload::CONFIG) {
                Ok(bytes) => pros_core::autoload::Settings::parse(&String::from_utf8_lossy(&bytes)),
                Err(why) => return Done::Failed(why.to_string()),
            };
            let boot = match files::retrieve(&target.link(), pros_core::chain::PATH) {
                Ok(bytes) => pros_core::boot::Boot::parse(&String::from_utf8_lossy(&bytes)),
                Err(why) => return Done::Failed(why.to_string()),
            };
            Done::Autoload(Box::new(settings), Box::new(boot))
        }
        Job::WriteAutoload(target, path, text) => {
            // Sent whole, exactly the text that was reviewed.
            match files::store(&target.link(), path, text.as_bytes()) {
                Ok(()) => Done::Said(format!(
                    "{path} written - {} bytes. The manager reads it at next startup.",
                    text.len()
                )),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        Job::EnableAutoload(target) => {
            // Read, merge, write: only autoload changes. No settings yet gets the default; already
            // on writes nothing.
            let current = files::retrieve(&target.link(), pros_core::autoload::CONFIG)
                .ok()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
            match pros_core::autoload::ensure_autoload_on(current.as_deref()) {
                None => Done::Said("autoload is already on - nothing to change".to_owned()),
                Some(text) => {
                    match files::store(&target.link(), pros_core::autoload::CONFIG, text.as_bytes())
                    {
                        Ok(()) => Done::Said(
                            "autoload turned on - the manager reads its list at next startup"
                                .to_owned(),
                        ),
                        Err(why) => Done::Failed(why.to_string()),
                    }
                }
            }
        }
        Job::CaptureConfig(target) => {
            // The paths come from the chain data, not the code. A path this target lacks is a
            // note, not a failure: a chain may name a file only some setups keep.
            let mut files = Vec::new();
            let mut notes = Vec::new();
            for (path, label) in pros_core::chain::capture_spots() {
                match files::retrieve(&target.link(), &path) {
                    Ok(bytes) => files.push(pros_core::recovery::baseline::Captured {
                        label,
                        path,
                        content: String::from_utf8_lossy(&bytes).into_owned(),
                    }),
                    Err(why) => notes.push(format!(
                        "{path}: not carried - it could not be read from this target ({why})"
                    )),
                }
            }
            Done::Captured(files, notes)
        }
        Job::PlaceFile(target, path, content) => {
            // Written whole, exactly as captured from the other target.
            match files::store(&target.link(), path, content.as_bytes()) {
                Ok(()) => Done::Said(format!(
                    "{path} restored - {} bytes, as the chain carries it",
                    content.len()
                )),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        other => Done::Failed(format!("not a settings job: {}", other.describe())),
    }
}

/// Asking a target what it is.
///
/// The `sysctl` keys in `pros_core::system::FACTS`, `df` and `ps`, each a separate round trip:
/// the shell answers one command at a time.
fn asking(link: &pros_link::Link) -> Done {
    // One connection per question, so a target that stops answering part way still reports
    // what it gave; an empty answer becomes an absent fact.
    let ask = |command: &str| pros_link::shell::run(link, command, SETTLE).unwrap_or_default();

    let mut answers = std::collections::BTreeMap::new();
    for (key, _) in pros_core::system::FACTS {
        let said = ask(&format!("sysctl {key}"));
        if !said.contains("No such file") {
            answers.insert((*key).to_owned(), said);
        }
    }
    let report = pros_core::system::Report::from(&answers, &ask("df"), &ask("ps"));
    Done::System(Box::new(report))
}

/// Restarts the user interface, then reads the target again so the panel is current.
///
/// Which process to signal, and that the system respawns it, is `pros-core`'s.
fn restart_ui(link: &pros_link::Link) -> Done {
    let listing = pros_link::shell::run(link, "ps", SETTLE).unwrap_or_default();
    let note = match pros_core::system::shell_ui(&pros_core::system::processes(&listing)) {
        None => "SceShellUI was not running - nothing to restart".to_owned(),
        Some(ui) => {
            let _ = pros_link::shell::run(
                link,
                &pros_core::system::kill(&ui.pid, pros_core::system::Signal::Terminate),
                SETTLE,
            );
            format!(
                "asked SceShellUI (PID {}) to restart - the system respawns it",
                ui.pid
            )
        }
    };
    // `asking` always returns `Done::System`.
    let Done::System(report) = asking(link) else {
        return Done::Failed("reading the target after the restart did not answer".into());
    };
    Done::Signalled { note, report }
}

/// Ends every process a title owns, then reads the target again to say whether it is gone.
fn close_title(link: &pros_link::Link, id: &str) -> Done {
    let listing = pros_link::shell::run(link, "ps", SETTLE).unwrap_or_default();
    let running = pros_core::system::processes(&listing);
    let mine = pros_core::system::of_title(&running, id);
    let closed = mine.len();
    for process in &mine {
        for command in pros_core::system::end(process) {
            let _ = pros_link::shell::run(link, &command, SETTLE);
        }
    }
    let Done::System(report) = asking(link) else {
        return Done::Failed("reading the target after the close did not answer".into());
    };
    // A title still listed did not close.
    let note = if closed == 0 {
        format!("no running process found for {id}")
    } else if pros_core::system::of_title(&report.processes, id).is_empty() {
        format!("{id} is gone")
    } else {
        format!("{id} is still listed - it did not close")
    };
    Done::Signalled { note, report }
}

/// Ends one process by pid, then reads the target again to say whether it is gone.
///
/// The by-pid form of [`close_title`]; `pros_core::system::end` wakes a stopped process before
/// ending it. An unused pid is reported, not signalled.
fn end_process(link: &pros_link::Link, pid: &str) -> Done {
    let listing = pros_link::shell::run(link, "ps", SETTLE).unwrap_or_default();
    let running = pros_core::system::processes(&listing);
    let found = pros_core::system::by_pid(&running, pid).map(|process| {
        let what = process.command.clone();
        for command in pros_core::system::end(process) {
            let _ = pros_link::shell::run(link, &command, SETTLE);
        }
        what
    });
    let Done::System(report) = asking(link) else {
        return Done::Failed("reading the target after the end did not answer".into());
    };
    let pid = pid.trim();
    let note = match found {
        None => format!("no process with pid {pid} is running"),
        Some(_) if pros_core::system::by_pid(&report.processes, pid).is_none() => {
            format!("pid {pid} is gone")
        }
        Some(what) if what.is_empty() => format!("pid {pid} is still listed - it did not end"),
        Some(what) => format!("{what} (pid {pid}) is still listed - it did not end"),
    };
    Done::Signalled { note, report }
}

/// Handing a package to the target to read and register.
///
/// Given [`UNPACKING`] rather than [`SETTLE`]; silence is not treated as success.
fn installing(link: &pros_link::Link, file: &std::path::Path) -> Done {
    // Served for the length of this job; the target fetches it over the network (measured: a
    // local path on the target is not readable by the installer).
    let offered = match pros_core::handover::offer_to(file, &link.address) {
        Ok(offered) => offered,
        Err(why) => return Done::Failed(why),
    };
    let said = pros_link::shell::run(link, &pros_core::install::command(&offered.url), UNPACKING);
    match said {
        Ok(text) => {
            let read = pros_core::install::read(&text);
            // A target that never fetched and one that rejected the package answer alike; only
            // the fetch count tells them apart.
            if offered.taken() == 0 && !read.is_a_known_failure() {
                return Done::Failed(format!(
                    "the target never fetched the package, so what it said was not about it: {}",
                    read.describe()
                ));
            }
            Done::Installed(read)
        }
        Err(why) => Done::Failed(why.to_string()),
    }
}

/// Removing files from this machine.
///
/// Every failure is collected rather than the first ending the job.
fn erasing(what: &[PathBuf]) -> Done {
    let mut refused = Vec::new();
    for path in what {
        if let Err(why) = std::fs::remove_file(path) {
            refused.push(format!("{}: {why}", path.display()));
        }
    }
    if refused.is_empty() {
        Done::Said(format!("{} deleted from this machine", what.len()))
    } else {
        Done::Failed(refused.join("; "))
    }
}

/// Removing files from the target.
///
/// One session for the whole selection. Every refusal is collected, so a partial removal is
/// reported as such.
fn removing(link: &pros_link::Link, what: &[(String, bool)]) -> Done {
    let mut session = match files::Session::open(link) {
        Ok(session) => session,
        Err(why) => return Done::Failed(why.to_string()),
    };
    // The walk is in `pros_core::remove`, testable against the fake target. What the file
    // service cannot remove (an empty directory its `RMD` refuses, a broken symlink) is
    // finished over the shell with `rmdir` or `rm -f`, never a recursive force.
    let mut shell = pros_core::remove::ShellForce::new(link);
    let gone = pros_core::remove::these_then_force(&mut session, &mut shell, what);
    session.close();
    // The wording covers both halves of a partial removal; any file kept makes it a failure.
    if gone.kept.is_empty() {
        Done::Said(gone.describe())
    } else {
        Done::Failed(gone.describe())
    }
}

#[cfg(test)]
mod tests {
    use pros_core::target::Target;

    use super::{Update, Worker};

    use crate::state::{Done, Job};

    /// A job's answer, including a failure, arrives on the channel without the caller blocking.
    #[test]
    fn an_answer_arrives_without_the_caller_waiting_for_it() {
        let worker = Worker::new();
        assert!(worker.collect().is_none(), "an answer before anything ran");

        worker.start(Job::Browse(
            Target {
                name: "absent".to_owned(),
                address: LOCAL.to_owned(),
                ports: std::collections::BTreeMap::new(),
                chain: None,
            },
            "/data".to_owned(),
        ));

        let mut answer = None;
        for _ in 0..200 {
            if let Some(done) = worker.collect() {
                answer = Some(done);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }

        assert!(
            nothing_is_listening_on(2121),
            "port 2121 (ftpsrv) is answering on this machine, so the browse this test needs \
             to fail may have succeeded - that is this machine's state, not a fault in the tool"
        );
        match answer {
            Some(Update::Finished(Done::Failed(why))) => {
                assert!(!why.is_empty(), "a failure with nothing said");
            }
            other => panic!("expected a failure to arrive as an answer, got {other:?}"),
        }
    }

    /// Somewhere a connection is refused at once. Loopback rather than an unresolvable name,
    /// whose resolver timeout can exceed the test's wait.
    const LOCAL: &str = "127.0.0.1";

    /// Whether anything is listening where these tests need nothing to be.
    ///
    /// A fake target left running holds these ports; the assertion names that as the
    /// machine's state rather than a defect.
    fn nothing_is_listening_on(port: u16) -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(200),
        )
        .is_err()
    }
}
