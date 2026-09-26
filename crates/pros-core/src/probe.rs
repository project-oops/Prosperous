//! Launching a title and following its log: the probe loop's steps, shared by both shims.
//!
//! The restore step stays in the command, since a title picked off the target has no local
//! build. The log is attached before the launch: a probe does its work in the first second or
//! two and then parks, so a follower attached after the launch captures nothing (measured).
//! The klogsrv connection is the subscription, and
//! [`SUBSCRIBE_SETTLE`] is margin on top of that ordering.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long a shell command is given to answer.
const SETTLE: Duration = Duration::from_millis(1200);

/// A pause between attaching the log follower and issuing the launch.
///
/// The measured need is under a second; this is more, because a missed subscription loses the
/// whole run.
pub const SUBSCRIBE_SETTLE: Duration = Duration::from_secs(3);

/// What a payload prints immediately before it parks, from oops-sdk's
/// `oops_system_park_until_closed`.
///
/// oops-sdk's klog renders `[<app id>:<tag>] <message>`, so the line is
/// `[GLPB00001:park] work done`, or `[park] work done` with no app id. Matching from the
/// closing bracket covers both and does not collide with a title's own "work done".
pub const PARK_SENTINEL: &str = "park] work done";

/// Whether a log line is a payload saying it has finished and is about to park.
#[must_use]
pub fn is_park(line: &str) -> bool {
    line.contains(PARK_SENTINEL)
}

/// Ends every process a title owns, best-effort, and says how many there were.
///
/// A parked big-app ignores every signal (measured), so this ends what can be ended and the
/// following launch shows whether the slot is still held. A `ps` that does not answer reads
/// as nothing running.
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
/// The caller attaches the stream before the launch (see the module note). The stream is
/// klogsrv and the poll is shsrv, so they do not contend: a background watcher runs `ps` once
/// a second and shuts the stream to end the drain. The watcher treats absence as an exit only
/// after the title has appeared. Raising `stop` is noticed within a second; to end a quiet log
/// sooner, also shut the stream with the [`pros_link::log::Stopper`].
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
        // A homebrew title cannot exit (`exit`, `_Exit` and `sceKernelExit` are absent, `_exit`
        // raises `SIGSYS` under big-app credentials, and returning from the entry faults), so
        // it parks, and only this line tells a finished probe from a working one.
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
/// A title whose description does not read is kept with no name rather than dropped.
///
/// # Errors
///
/// When the title directory cannot be listed.
pub fn installed(link: &pros_link::Link) -> crate::Result<Vec<crate::titles::Metadata>> {
    let entries = pros_link::files::list(link, crate::titles::APPMETA)?;
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

    /// The park line is recognised with and without an app id.
    #[test]
    fn the_park_line_is_recognised_with_or_without_an_app_id() {
        assert!(is_park("[GLPB00001:park] work done"));
        assert!(is_park("[park] work done"));
        assert!(!is_park("[NVRB00001:stderr] level work done"));
    }

    /// A stopped follow is described as stopped, not as reaching the cap.
    #[test]
    fn a_stopped_follow_says_it_was_stopped() {
        assert!(
            Ending::Stopped
                .describe("NVRB00001", 60)
                .starts_with("stopped")
        );
    }
}
