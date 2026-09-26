//! Asking every payload's project what it has released, without stopping anything else.
//!
//! A sweep is spaced out and waits out refusals, so it runs on its own thread beside the worker
//! rather than as a job that would hold the window for its whole length. Each answer is sent as
//! it arrives, so the column fills in visibly instead of all at once after a silence that looks
//! like a hang. Dropping the sweep raises a flag the thread checks between projects.

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
    /// How many were asked about, so a panel can show a count rather than "working".
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
    /// `None` when there is nothing to ask, so no progress line is drawn for work nobody is
    /// doing.
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
                // Spaced between asks only; the first goes at once.
                if at > 0 {
                    thread::sleep(between());
                }
                let found = look(payload);
                // A rate-limit refusal ends the sweep: the limit is per address, so every
                // remaining ask would be refused the same way.
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
    /// Returns the answers for the caller to record; the sweep keeps no copy.
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
        self.stopping.store(true, Ordering::Relaxed);
    }
}

/// Asks about one payload and records what happened, whichever way it went.
///
/// Always an answer: a payload that could not be asked about gets a stored result saying so,
/// so a source that never answers is not retried on every launch.
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

    /// Nothing due starts no sweep.
    #[test]
    fn an_empty_sweep_does_not_start() {
        assert!(Sweep::start(Vec::new()).is_none());
    }
}
