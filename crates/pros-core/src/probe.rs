//! Launching a title and following what it says: the probe loop's steps, shared by both shims.
//!
//! # Why this is here and not in the command
//!
//! `pros probe` ran the whole loop in `pros-cli` - close, restore, launch, follow - and the window
//! then wanted the same launch-and-follow for a title picked from a list. Principle 3: a capability
//! in only one shim is one that drifts, so the steps both need live here and each shim only says
//! what it saw. The restore stays in the command, because a title picked off the target has no
//! local build to restore from.
//!
//! # The ordering, which is the whole reason this is fiddly
//!
//! **Attach to the log before launching.** A probe does its whole job in the first second or two
//! and then parks, so a follower attached *after* the launch misses all of it: the output lands in
//! the gap between the launch returning and the stream opening, and the run succeeds with an empty
//! capture (measured). The connection is the subscription - klogsrv buffers what it emits once the
//! socket is open - so following first and launching second is the fix, and
//! [`SUBSCRIBE_SETTLE`](crate::probe::SUBSCRIBE_SETTLE) is insurance on top of it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a shell command is given to answer.
const SETTLE: Duration = Duration::from_millis(1200);

/// A beat between attaching the log follower and issuing the launch.
///
/// **The ordering is the mechanism; this is the margin.** Measured need is sub-second; this is
/// deliberately more, because missing the subscription loses the whole run.
pub const SUBSCRIBE_SETTLE: Duration = Duration::from_secs(3);

/// What a payload prints immediately before it parks, from oops-sdk's
/// `oops_system_park_until_closed`.
///
/// **The tag goes inside the brackets, not before the message.** oops-sdk's klog renders
/// `[<app id>:<tag>] <message>`, so the line on the wire is `[GLPB00001:park] work done` and a
/// payload with no app id set prints `[park] work done`. This matched `park: work done` when it
/// first shipped and therefore matched nothing: the sentinel was printed on 2026-09-21 at 11:47Z
/// and the watch ran to its cap anyway. Matching from the closing bracket covers both spellings
/// and cannot collide with a title whose own log says "work done".
pub const PARK_SENTINEL: &str = "park] work done";

/// Whether a log line is a payload saying it has finished and is about to park.
#[must_use]
pub fn is_park(line: &str) -> bool {
    line.contains(PARK_SENTINEL)
}

/// Ends every process a title owns, best-effort, and says how many there were.
///
/// **Best-effort on purpose.** A parked big-app ignores every signal (measured; oops-mesa's
/// b1e4), so this ends a killable process and no more - the launch that follows is what says
/// whether the slot is still held. A `ps` that will not answer reads as nothing running.
#[must_use]
pub fn close(link: &pros_link::Link, id: &str) -> usize {
    let listing = pros_link::shell::run(link, "ps", SETTLE).unwrap_or_default();
    let running = crate::system::processes(&listing);
    let mine = crate::system::of_title(&running, id);
    for process in &mine {
        for command in crate::system::end(process) {
            let _ = pros_link::shell::run(link, &command, SETTLE);
        }
    }
    mine.len()
}

/// Asks the target to start a title, and reads what it said.
///
/// # Errors
///
/// When the shell service does not answer.
pub fn launch(link: &pros_link::Link, id: &str) -> pros_link::Result<crate::launch::Said> {
    Ok(crate::launch::read(&pros_link::shell::run(
        link,
        &crate::launch::command(id),
        SETTLE,
    )?))
}

/// How a follow ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// The payload printed its park sentinel - it finished its work and is now idling. The only
    /// ending that tells "done" from "still going" for a title that cannot exit.
    Finished,
    /// The title was seen and then left the process list - it exited or crashed.
    Exited,
    /// The cap elapsed with the title still present. A finished probe that parked rather than
    /// exiting looks exactly like this.
    Parked,
    /// The cap elapsed without the title ever appearing - it exited at once or never started.
    NeverSeen,
    /// Somebody stopped the follow before any of the above.
    Stopped,
}

impl Ending {
    /// What to say about it, in a line.
    #[must_use]
    pub fn describe(self, id: &str, seconds: u64) -> String {
        match self {
            Self::Finished => format!(
                "{id} finished its work and parked - a payload cannot exit, so it idles until \
                 the dashboard Close ends it."
            ),
            Self::Exited => format!("{id} left the process list - it exited or crashed."),
            Self::Parked => format!(
                "reached the {seconds}s cap - {id} is still running (a probe that finished may \
                 have parked; the dashboard Close ends it)."
            ),
            Self::NeverSeen => format!(
                "reached the {seconds}s cap - {id} was never seen in the process list, so it \
                 exited at once or did not start."
            ),
            Self::Stopped => format!("stopped following {id} - it may still be running."),
        }
    }
}

/// Streams an already-attached log until the title parks, leaves the process list, `seconds`
/// elapse, or `stop` is raised - handing each line to `each`.
///
/// **The follower is attached by the caller, before the launch** - see the module note. This takes
/// the open stream rather than opening it, so the subscription is already live by the time the
/// title prints anything.
///
/// **Two connections, two services, on purpose.** The stream is klogsrv and the poll is shsrv, so
/// they do not contend: a background watcher runs `ps` once a second while this drains the log,
/// and shutting the stream is what ends the drain. The watcher waits for the title to *appear*
/// before treating its absence as an exit, so the gap between a launch and the process showing is
/// never read as a crash. Raising `stop` is noticed by that watcher within a second; to end a
/// quiet log sooner, shut the stream with the [`pros_link::log::Stopper`] too.
pub fn follow<I>(
    stopper: &Arc<pros_link::log::Stopper>,
    stream: I,
    link: &pros_link::Link,
    id: &str,
    seconds: u64,
    stop: &Arc<AtomicBool>,
    each: &mut dyn FnMut(String),
) -> Ending
where
    I: Iterator<Item = pros_link::log::Line>,
{
    let done = Arc::new(AtomicBool::new(false));
    let exited = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicBool::new(false));
    let (stopper_w, done_w, exited_w, seen_w, stop_w) = (
        Arc::clone(stopper),
        Arc::clone(&done),
        Arc::clone(&exited),
        Arc::clone(&seen),
        Arc::clone(stop),
    );
    let link_w = link.clone();
    let id_w = id.to_owned();
    let deadline = Instant::now() + Duration::from_secs(seconds);

    let watcher = std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if done_w.load(Ordering::Relaxed) {
                break;
            }
            if stop_w.load(Ordering::Relaxed) || Instant::now() >= deadline {
                stopper_w.stop();
                break;
            }
            let ps = pros_link::shell::run(&link_w, "ps", SETTLE).unwrap_or_default();
            let present =
                !crate::system::of_title(&crate::system::processes(&ps), &id_w).is_empty();
            if present {
                seen_w.store(true, Ordering::Relaxed);
            } else if seen_w.load(Ordering::Relaxed) {
                exited_w.store(true, Ordering::Relaxed);
                stopper_w.stop();
                break;
            }
        }
    });

    let mut finished = false;
    for read in stream {
        let Ok(text) = read else {
            break;
        };
        // **The payload saying it is done.** A homebrew title cannot exit - `exit`, `_Exit` and
        // `sceKernelExit` are absent, `_exit` raises `SIGSYS` under a big-app's credentials, and
        // returning from the entry point faults at zero - so the conforming ending is to park,
        // and a finished probe is indistinguishable from a working one by the process list
        // alone. This line lets a watcher end without waiting out its whole cap.
        let parked = is_park(&text);
        each(text);
        if parked {
            finished = true;
            break;
        }
    }
    done.store(true, Ordering::Relaxed);
    stopper.stop();
    let _ = watcher.join();

    if finished {
        Ending::Finished
    } else if exited.load(Ordering::Relaxed) {
        Ending::Exited
    } else if stop.load(Ordering::Relaxed) {
        Ending::Stopped
    } else if seen.load(Ordering::Relaxed) {
        Ending::Parked
    } else {
        Ending::NeverSeen
    }
}

/// What is installed, by name - what `pros titles` lists.
///
/// A title whose description will not read is kept, with no name, rather than dropped: it is
/// installed, and the identifier is true about it.
///
/// # Errors
///
/// When the title directory cannot be listed.
pub fn installed(link: &pros_link::Link) -> Result<Vec<crate::titles::Metadata>, String> {
    let entries =
        pros_link::files::list(link, crate::titles::APPMETA).map_err(|why| why.to_string())?;
    let found = crate::library::scan(&entries);
    Ok(crate::library::titles(&found)
        .into_iter()
        .map(|item| {
            crate::titles::read(link, &item.name).unwrap_or_else(|_| crate::titles::Metadata {
                id: item.name.clone(),
                name: None,
                version: None,
                content_id: None,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Ending, is_park};

    /// Both spellings oops-sdk prints: with an app id and without one.
    #[test]
    fn the_park_line_is_recognised_with_or_without_an_app_id() {
        assert!(is_park("[GLPB00001:park] work done"));
        assert!(is_park("[park] work done"));
        assert!(!is_park("[NVRB00001:stderr] level work done"));
    }

    /// A stop is said as a stop, not as a title that is still running at a cap nobody reached.
    #[test]
    fn a_stopped_follow_says_it_was_stopped() {
        assert!(
            Ending::Stopped
                .describe("NVRB00001", 60)
                .starts_with("stopped")
        );
    }
}
