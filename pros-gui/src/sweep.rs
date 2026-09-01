//! Asking every payload's project what it has released, without stopping anything else.
//!
//! # Why this is not a job
//!
//! For the same reason following a log is not: the queue runs one thing at a time so two
//! answers cannot interleave, and that is right for a request that finishes. **A sweep does
//! not finish quickly on purpose.** It is spaced out and it waits out refusals, so putting it
//! in the queue would mean the window can do nothing at all for as long as it runs - including
//! on launch, which is when it runs by default.
//!
//! So it runs beside the worker on its own thread, and its answers arrive one at a time.
//!
//! # Why the answers arrive one at a time
//!
//! A sweep that returned everything at the end would show nothing for a minute and then all of
//! it, which is indistinguishable from a sweep that hung. Each answer is sent as it comes, so
//! the column fills in and somebody can see it working.
//!
//! # Stopping
//!
//! Dropping it. The thread checks a flag between projects, which works here and would not for
//! a log: a sweep is a loop that comes back to the top regularly, where a log is blocked
//! inside a read that only a shutdown can interrupt.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;

use pros_core::manifest::Payload;
use pros_core::sources::{NotAsked, Upstream, ask, between, now, repository_of};

/// One project's answer.
pub(crate) struct Answer {
    /// Which payload, as the list names it.
    pub(crate) name: String,
    /// What came back.
    pub(crate) found: Upstream,
}

/// A sweep in progress.
pub(crate) struct Sweep {
    answers: Receiver<Answer>,
    stopping: Arc<AtomicBool>,
    /// How many were asked about, so a panel can say *3 of 34* rather than *working*.
    asked: usize,
    /// How many have come back.
    back: usize,
    /// Whether the thread has finished.
    ended: bool,
}

impl std::fmt::Debug for Sweep {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Sweep")
            .field("asked", &self.asked)
            .field("back", &self.back)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Sweep {
    /// Starts asking about these payloads.
    ///
    /// `None` when there is nothing to ask, which is the ordinary state a few minutes after the
    /// last sweep - and is deliberately not a sweep that starts and immediately says it is
    /// done, because that draws a progress line for work nobody is doing.
    pub(crate) fn start(due: Vec<Payload>) -> Option<Self> {
        if due.is_empty() {
            return None;
        }
        let asked = due.len();
        let stopping = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stopping);
        let (sender, answers) = channel();

        thread::spawn(move || {
            for (at, payload) in due.iter().enumerate() {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                // **Spaced, and only between asks.** Sleeping before the first one would make
                // the whole feature feel broken for no benefit at all.
                if at > 0 {
                    thread::sleep(between());
                }
                let found = look(payload);
                // **A refusal that named a time ends the sweep.** Carrying on would spend the
                // rest of the list on the same refusal and finish with thirty-three rows all
                // saying *too many requests* - and the limit is per address, so the next one
                // was never going to be treated differently.
                let limited = found.latest.is_none() && found.said.starts_with("too many");
                if sender
                    .send(Answer {
                        name: payload.name.clone(),
                        found,
                    })
                    .is_err()
                    || limited
                {
                    break;
                }
            }
        });

        Some(Self {
            answers,
            stopping,
            asked,
            back: 0,
            ended: false,
        })
    }

    /// Takes whatever has arrived since last time.
    ///
    /// Returns the answers, so the caller records them - this holds none of them itself, for
    /// the usual reason: two places keeping the same list is two places to disagree.
    pub(crate) fn drain(&mut self) -> Vec<Answer> {
        let mut arrived = Vec::new();
        loop {
            match self.answers.try_recv() {
                Ok(answer) => {
                    self.back += 1;
                    arrived.push(answer);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
        }
        arrived
    }

    /// Whether the thread has finished.
    pub(crate) const fn has_ended(&self) -> bool {
        self.ended
    }

    /// How far along it is, for a person watching.
    pub(crate) const fn progress(&self) -> (usize, usize) {
        (self.back, self.asked)
    }
}

impl Drop for Sweep {
    fn drop(&mut self) {
        // Checked between projects, which is where this loop spends its waiting.
        self.stopping.store(true, Ordering::Relaxed);
    }
}

/// Asks about one payload and records what happened, whichever way it went.
///
/// **Always an answer.** A payload that could not be asked about gets a stored result saying
/// so, rather than no result - otherwise the next sweep asks again immediately and a source
/// that will never answer is retried on every launch forever.
fn look(payload: &Payload) -> Upstream {
    let Some((owner, repo)) = repository_of(payload) else {
        return Upstream {
            latest: None,
            assets: Vec::new(),
            asked_at: now(),
            said: NotAsked::NoRepository.to_string(),
        };
    };
    match ask(&owner, &repo) {
        Ok((tag, assets)) => Upstream {
            latest: Some(tag),
            assets,
            asked_at: now(),
            said: format!("{owner}/{repo}"),
        },
        Err(why) => Upstream {
            latest: None,
            assets: Vec::new(),
            asked_at: now(),
            said: why.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::Sweep;

    /// **Nothing due is no sweep**, rather than one that starts and instantly finishes.
    ///
    /// This is the normal case: everything was asked about within the staleness window, and a
    /// progress line for zero projects is a line that says work is happening when none is.
    #[test]
    fn an_empty_sweep_does_not_start() {
        assert!(Sweep::start(Vec::new()).is_none());
    }
}
