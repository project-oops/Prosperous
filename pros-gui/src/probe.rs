//! Launching an installed title and capturing what it says, for the probe screen.
//!
//! A probe follows the log for as long as the title talks, up to a cap, so like
//! [`crate::tail`] it runs on its own thread beside the worker rather than as a job that would
//! hold the window for a minute. The steps are `pros_core::probe`'s, the same ones `pros probe`
//! runs: close what the title left running, attach to the log, launch, follow until it parks,
//! exits or the cap passes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::thread;

/// Something the probe thread has to say.
enum Event {
    /// A step, said as it happens.
    Step(String),
    /// A line from the target's log.
    Line(String),
    /// How it ended - the last thing it will send.
    Ended(String),
}

/// A probe in progress, or just finished.
pub(crate) struct Run {
    /// Which target it is attached to, so a change of target can end it.
    pub(crate) target: String,
    /// Which title it launched.
    pub(crate) id: String,
    events: Receiver<Event>,
    /// Raised to ask the follow to end.
    stop: Arc<AtomicBool>,
    /// The log stream, once open, so a stop can shut it rather than wait for the next line.
    stopper: Arc<Mutex<Option<Arc<pros_link::log::Stopper>>>>,
    /// Whether the thread has said its last.
    ended: bool,
}

impl std::fmt::Debug for Run {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Run")
            .field("target", &self.target)
            .field("id", &self.id)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Run {
    /// Starts probing `id` on `target`, following for at most `seconds`.
    pub(crate) fn start(target: &pros_core::target::Target, id: &str, seconds: u64) -> Self {
        let (sender, events) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopper = Arc::new(Mutex::new(None));
        let (link, id_t, stop_t, stopper_t) = (
            target.link(),
            id.to_owned(),
            Arc::clone(&stop),
            Arc::clone(&stopper),
        );
        thread::spawn(move || {
            let ending = probing(&link, &id_t, seconds, &stop_t, &stopper_t, &sender);
            let _ = sender.send(Event::Ended(ending));
        });
        Self {
            target: target.name.clone(),
            id: id.to_owned(),
            events,
            stop,
            stopper,
            ended: false,
        }
    }

    /// Takes what has arrived since last time: log lines and steps into `lines` (a step marked so
    /// it reads as this program talking, not the target), the latest step into `status`.
    ///
    /// Returns whether anything did, so the window repaints only when there is something new.
    pub(crate) fn drain(&mut self, lines: &mut Vec<String>, status: &mut String) -> bool {
        let mut had = false;
        loop {
            match self.events.try_recv() {
                Ok(Event::Line(line)) => lines.push(line),
                Ok(Event::Step(step)) => {
                    lines.push(format!("-- {step}"));
                    *status = step;
                }
                Ok(Event::Ended(ending)) => {
                    lines.push(format!("-- {ending}"));
                    *status = ending;
                    self.ended = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
            had = true;
        }
        if lines.len() > crate::tail::KEPT {
            lines.drain(..lines.len() - crate::tail::KEPT);
        }
        had
    }

    /// Whether it is still going.
    pub(crate) const fn is_running(&self) -> bool {
        !self.ended
    }

    /// Asks it to stop following. The title is left running; the dashboard's close ends it.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(held) = self.stopper.lock()
            && let Some(stopper) = held.as_ref()
        {
            stopper.stop();
        }
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The probe itself, on its own thread. Returns how it ended, in a line.
fn probing(
    link: &pros_link::Link,
    id: &str,
    seconds: u64,
    stop: &Arc<AtomicBool>,
    held: &Mutex<Option<Arc<pros_link::log::Stopper>>>,
    say: &Sender<Event>,
) -> String {
    let note = |text: String| {
        let _ = say.send(Event::Step(text));
    };

    // Best-effort: a parked big-app ignores signals, and the launch says whether the slot is
    // still held.
    match pros_core::probe::close(link, id) {
        0 => note(format!("{id} is not running")),
        closed => note(format!("closed {id} ({closed} process(es))")),
    }
    if stop.load(Ordering::Relaxed) {
        return pros_core::probe::Ending::Stopped.describe(id, seconds);
    }

    // Attach before launching: a title's early output is lost otherwise (`pros_core::probe`).
    note(format!("attaching to {id}'s log before launch..."));
    let (stopper, lines) = match pros_link::log::follow(link) {
        Ok(opened) => opened,
        Err(why) => return format!("could not open the log, so nothing was launched: {why}"),
    };
    let stopper = Arc::new(stopper);
    if let Ok(mut slot) = held.lock() {
        *slot = Some(Arc::clone(&stopper));
    }
    thread::sleep(pros_core::probe::SUBSCRIBE_SETTLE);
    if stop.load(Ordering::Relaxed) {
        return pros_core::probe::Ending::Stopped.describe(id, seconds);
    }

    match pros_core::probe::launch(link, id) {
        Ok(said @ pros_core::launch::Said::Asked(_)) => {
            note(format!("launching {id}: {}", said.describe()));
        }
        Ok(said) => {
            stopper.stop();
            return format!(
                "the launch was refused: {} - if a parked title is holding the slot it cannot be \
                 closed by signal; the console's own dashboard Close ends it",
                said.describe()
            );
        }
        Err(why) => {
            stopper.stop();
            return format!("could not ask the target to launch {id}: {why}");
        }
    }

    note(format!(
        "following {id} (until it parks, exits, or {seconds}s)..."
    ));
    let mut any = false;
    let ending = pros_core::probe::follow(&stopper, lines, link, id, seconds, stop, &mut |line| {
        any = true;
        let _ = say.send(Event::Line(line));
    });
    if !any {
        note(format!(
            "the log was quiet while {id} ran - which is a result, not a failure"
        ));
    }
    ending.describe(id, seconds)
}
