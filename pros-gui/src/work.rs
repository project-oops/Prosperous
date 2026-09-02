//! Doing the asking somewhere other than the drawing thread.
//!
//! # Why this exists at all
//!
//! Every operation here is a network round trip to a machine that may be switched off. A
//! check is five ports at a second and a half each; a log window is however long somebody
//! asked for. Done on the drawing thread, the window stops repainting for that whole time -
//! and a window that has stopped repainting is indistinguishable from one that has crashed.
//!
//! So the job runs on its own thread and the answer arrives on a channel, which the drawing
//! thread collects between frames.
//!
//! # Why a thread per job rather than a pool
//!
//! Because only one job runs at a time - [`crate::state::State::begin`] refuses while
//! anything is running - so a pool would be a queue that never holds more than one thing. A
//! thread that exists for the length of one target operation is the honest size of this.

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
/// **Longer than a command, because the target does the work.** It reads the package and
/// unpacks it, and a window sized for something that answers at once would report silence -
/// which this deliberately does not treat as success, so the only cost of waiting is waiting.
const UNPACKING: std::time::Duration = std::time::Duration::from_secs(20);

/// What comes back from a running job.
///
/// **Two kinds, because they mean different things to the window.** An answer ends the job
/// and replaces a panel; a report of progress ends nothing and only says how far a long copy
/// has got. Folding them into one would make the state machine decide which by inspection.
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
    /// **Shared rather than signalled through the channel**, because the channel is read by
    /// the drawing thread and the thing that needs to hear this is the worker. A message
    /// would arrive only when the worker next looked, which is exactly what it cannot do
    /// while it is inside a copy.
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
    /// **Asks rather than kills.** A copy that was terminated mid-file would leave a partial
    /// file looking like a whole one; this lets the walk finish the file it is on, record
    /// everything it did not do, and come back with a summary that says so.
    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::Relaxed);
    }

    /// Starts a job on its own thread.
    pub(crate) fn start(&self, job: Job) {
        let sender = self.sender.clone();
        // Cleared here rather than when the last job ended: a stop that arrives as a job
        // finishes must not silently cancel the next one somebody starts.
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
            // The send can only fail if the window has gone, in which case there is nobody
            // to tell and nothing to do about it.
            let _ = sender.send(Update::Finished(done));
        });
    }

    /// Takes an update if one has arrived, without waiting for one.
    #[must_use]
    pub(crate) fn collect(&self) -> Option<Update> {
        // Both kinds of nothing mean the same thing here: no answer this frame. A closed
        // channel cannot happen while this holds a sender, and if it somehow did there
        // would be nothing to do about it mid-frame.
        self.answers.try_recv().ok()
    }
}

/// Does the thing, and turns any failure into words.
///
/// **Every error becomes a sentence here rather than being propagated**, because the window
/// has one place to show trouble and a person reading it wants the target's own wording -
/// which the library's errors already carry.
fn perform(job: &Job, watch: &mut dyn FnMut(&Progress), stop: &dyn Fn() -> bool) -> Done {
    match job {
        Job::Check(target) => {
            // The manifest is read here rather than passed in, because what it widens the
            // check to cover is a property of the list on disk at the moment of asking - and
            // a list edited between one check and the next should take effect at the next
            // one, with no restart.
            let described = pros_core::manifest::Tracked::Payloads
                .read()
                .unwrap_or_else(|_| pros_core::manifest::recommended());
            let report = check_declaring(target, &described, PATIENCE);
            // Asked in the same job because they are one question in a person's head: what
            // is running, and what will still be running after a reboot. A boot list that
            // could not be read becomes `None`, which the survey reports as unknown rather
            // than as a list naming nothing.
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
/// # Three writes, and the third is the one nobody notices missing
///
/// The folder, because `payload_mgr_resolve_path` looks for `<dir>/<name>/<file>`; the ELF;
/// and the `.json` beside it. **The sidecar is the only thing on a target that says which
/// build a payload is** - the ELF carries no version string at all - so a payload put there
/// without one is a payload nothing can ever report as out of date. That is why so much of the
/// version column reads `?`.
///
/// An existing folder is not a failure, which is the normal case for a payload being replaced.
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
    // **The version, or an honest silence.** A sidecar claiming a version the description does
    // not state would be this program inventing the one fact the target is asked for.
    let mut said = format!("{} bytes written to {at}", bytes.len());
    if payload.version.is_some() {
        match pros_core::payloads::sidecar_for(payload) {
            Ok(about) => {
                let beside = format!("{at}.json");
                if let Err(why) = session.store(&beside, &about) {
                    // Not a failure: the payload is in place and will load. Only the thing that
                    // would have said which build it is did not arrive.
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

/// The jobs that move whole folders, and the ones that put a file somewhere.
///
/// Split from the rest because `perform` had grown past the point where a reader can hold
/// it, not because these are different in kind.
/// Copying a folder off the target, and recording where it came from.
///
/// **Its own function because of the stamp**, not because of its length. Which account a save
/// belongs to is knowable at exactly one moment - while the path it came out of is still in
/// hand - and burying that inside a dispatcher is how it would eventually be moved somewhere
/// the path is gone.
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
    // **Written at the only moment it is known for certain.** Which account a save belongs to
    // is in the path it came out of, and two saves in three carry no parameter file of their
    // own to say later.
    if let Some(user) = pros_core::origin::user_in(from) {
        let record = pros_core::origin::Origin {
            target: target.name.clone(),
            address: target.address.clone(),
            user,
            from: from.to_owned(),
            when: pros_core::origin::now(),
        };
        // A record that could not be written is not worth losing the backup over - the copy is
        // the thing somebody asked for.
        let _ = pros_core::origin::stamp(into, &record);
    }
    Done::Copied(Box::new(summary), into.display().to_string())
}

/// Putting a folder back, having first decided whether it may go.
///
/// **Its own function because of the refusal**, which happens before a byte moves and is the
/// only place in this module that declines to do what it was asked.
fn restoring(
    target: &pros_core::target::Target,
    from: &std::path::Path,
    to: &str,
    anyway: bool,
    watch: &mut dyn FnMut(&Progress),
    stop: &dyn Fn() -> bool,
) -> Done {
    // **Decided before a byte moves.** A save going to an account that did not write it needs
    // decrypting and re-signing; copying it regardless finishes cleanly and leaves files the
    // target will refuse, with nothing to connect the refusal back to this moment.
    if !anyway && pros_core::origin::user_in(to).is_some() {
        let here = pros_core::saves::account_on(&target.link());
        let needs = pros_core::origin::needed(from, to, here.as_deref());
        if !needs.is_plain() {
            return Done::Refused(needs);
        }
    }

    let mut session = match files::Session::open(&target.link()) {
        Ok(session) => session,
        Err(why) => return Done::Failed(why.to_string()),
    };
    let done = pros_core::transfer::upload(&mut session, from, to, watch, stop);
    session.close();
    match done {
        Ok(summary) => Done::Copied(Box::new(summary), to.to_owned()),
        Err(why) => Done::Failed(why),
    }
}

fn copying(job: &Job, watch: &mut dyn FnMut(&Progress), stop: &dyn Fn() -> bool) -> Done {
    match job {
        Job::Backup(target, from, into) => backing_up(target, from, into, watch, stop),
        Job::Restore(target, from, to, anyway) => restoring(target, from, to, *anyway, watch, stop),
        Job::Names(target, ids) => {
            // One round trip each, and **a title that will not answer is left out** rather
            // than added with an empty name: the identifier already stands for it, and an
            // empty name would look like a title that is called nothing.
            let found = ids
                .iter()
                .filter_map(|id| pros_core::titles::read(&target.link(), id).ok())
                .collect();
            Done::Named(found)
        }
        Job::ReadAutoload(_) | Job::WriteAutoload(..) => settings(job),
        Job::ReadSystem(target) => asking(&target.link()),
        Job::Launch(target, id) => {
            match pros_link::shell::run(&target.link(), &pros_core::launch::command(id), SETTLE) {
                Ok(said) => Done::Launched(pros_core::launch::read(&said)),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        Job::RunThere(target, path) => {
            // Refused here rather than sent and hoped for: the shell would split it and run
            // the first word, which is a different file or none.
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
                // **Not an empty list.** A list that could not be read says nothing about
                // what is in it, and an autoloader list that is simply absent is the normal
                // case on a target where the manager is auto-launched.
                Err(why) => Done::Failed(format!("{}: {why}", held.path)),
            }
        }
        Job::FindPayloads(target, root) => {
            // Everywhere the manager can see, not one folder: a payload on a stick is listed
            // by the manager and, outside its own folder there, can never be autoloaded. The
            // tag is the difference, and it is only knowable by looking in all three places.
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
            // The paths alone: what a place is called is for the window, and asking a target
            // about a label would be asking it a question about this program.
            let paths: Vec<&str> = candidates.iter().map(|place| place.path).collect();
            match pros_core::locate::first_of(&target.link(), &paths) {
                Ok(found) => Done::Located(found),
                Err(why) => Done::Failed(why),
            }
        }
        Job::FindSaves(target) => match pros_core::saves::find(&target.link()) {
            Ok(found) => Done::FoundSaves(found),
            Err(why) => Done::Failed(why),
        },
        Job::Fetch(payload, dir) => match dir.as_ref().map_or_else(
            || pros_core::fetch::fetch(payload),
            |dir| pros_core::fetch::fetch_into(payload, dir),
        ) {
            Ok(into) => Done::Fetched(payload.name.clone(), into),
            // Everything that can go wrong here already words itself, including the one
            // that matters: it arrived and was not what the manifest describes.
            Err(why) => Done::Failed(why.to_string()),
        },
        Job::Relist(payload) => match pros_core::sources::relist(payload) {
            Ok((now, found)) => Done::Relisted(Box::new(now), Box::new(found)),
            Err(why) => Done::Failed(why),
        },
        Job::Send(target, name, from) => match std::fs::read(from) {
            // The shape guard is the library's and it runs before anything is sent. Nothing
            // is announced here that is not going to happen, because the announcement is the
            // answer that comes back.
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
/// **Its own function so the write is one short, readable thing.** This is the only place
/// in the tool that replaces a file on a target, and it should be possible to read all of
/// it without scrolling past nine other jobs first.
fn settings(job: &Job) -> Done {
    match job {
        Job::ReadAutoload(target) => {
            // **Both files in one job**, because they are always shown together and a person
            // who asked to see the startup configuration did not ask twice. Two round trips
            // either way; one answer means the panel can never show half of it.
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
            // **The one write.** Sent whole, because what somebody reviewed line by line was
            // a whole file, and sending anything assembled differently would make the review
            // a document about something else.
            match files::store(&target.link(), path, text.as_bytes()) {
                Ok(()) => Done::Said(format!(
                    "{path} written - {} bytes. The manager reads it at next startup.",
                    text.len()
                )),
                Err(why) => Done::Failed(why.to_string()),
            }
        }
        other => Done::Failed(format!("not a settings job: {}", other.describe())),
    }
}

/// Asking a target what it is.
///
/// **Its own function because it is several questions, not one.** Four `sysctl` keys, a
/// storage listing and a process listing, each a separate round trip - the shell answers
/// one command at a time and does not pipeline.
fn asking(link: &pros_link::Link) -> Done {
    // One connection per question. Slower than it could be and simpler to reason about, and a
    // target that stops answering half way through reports what it managed rather than
    // nothing - an empty answer becomes an absent fact, not a blank one.
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

/// Handing a package to the target to read and register.
///
/// **Given longer than an ordinary command**, because the target does the work: it reads the
/// package and unpacks it, and a window sized for something that answers at once would report
/// silence. Silence is not treated as success, so the only cost of waiting is waiting.
fn installing(link: &pros_link::Link, file: &std::path::Path) -> Done {
    // Held out only for as long as this takes. The target fetches it itself - a path on its
    // own disk gives it nothing it can read, which was measured rather than assumed.
    let offered = match pros_core::handover::offer_to(file, &link.address) {
        Ok(offered) => offered,
        Err(why) => return Done::Failed(why),
    };
    let said = pros_link::shell::run(link, &pros_core::install::command(&offered.url), UNPACKING);
    match said {
        Ok(text) => {
            let read = pros_core::install::read(&text);
            // **Whether it came for the file is the other half of the answer.** A target that
            // never fetched and one that fetched and disliked it say indistinguishable things,
            // and only this side knows which happened.
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
/// **Every failure is collected rather than the first one ending it.** Somebody who selected
/// ten and got one refusal should be told which one, with the other nine gone as asked.
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
/// One session for the lot: a login per file is a round trip per file, and somebody deleting
/// ten things asked once.
///
/// **Every refusal is collected**, so nine that went and one that did not is reported as
/// exactly that rather than as a failure that says nothing about the nine.
fn removing(link: &pros_link::Link, what: &[(String, bool)]) -> Done {
    let mut session = match files::Session::open(link) {
        Ok(session) => session,
        Err(why) => return Done::Failed(why.to_string()),
    };
    // One session for the whole selection, and one guarded walk per directory. The walk is in
    // `pros_core::remove`, where it can be tested against a pretend target - the same reason
    // the backup's walk lives there rather than here.
    let gone = pros_core::remove::these(&mut session, what);
    session.close();
    // **A partial removal is neither.** Something went and something did not, and reporting it
    // as done or as failed describes one half. The wording carries both, and which it is
    // decides only whether the message is the ordinary one or the one that stops the queue.
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

    /// A job's answer comes back on the channel, and nothing blocks waiting for it.
    ///
    /// The target here is one that is not there, so this also pins the shape of a failure:
    /// **it arrives as an answer**, not as a panic on a thread nobody is watching.
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

    /// Somewhere a connection is refused rather than left hanging.
    ///
    /// **These tests used an unresolvable hostname**, which failed the moment this machine's
    /// resolver took longer to give up than the test was willing to wait - 11.1 seconds
    /// against a budget of 5. That is a test measuring the resolver, not the worker.
    ///
    /// A closed port on the loopback answers instantly and always, so what is left being
    /// tested is the only thing this test was ever about: that a failure comes back as an
    /// answer rather than as silence.
    const LOCAL: &str = "127.0.0.1";

    /// Whether anything is listening where these tests need nothing to be.
    ///
    /// **Said out loud rather than failing as though the code were wrong.** A stand-in left
    /// running from other work holds these ports, and a test that reported that as a defect
    /// would send somebody looking in the wrong place - which happened during this project's
    /// own development.
    fn nothing_is_listening_on(port: u16) -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            std::time::Duration::from_millis(200),
        )
        .is_err()
    }
}
