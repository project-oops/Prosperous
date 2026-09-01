//! Keeping a probe alive on a target that cannot restart it.
//!
//! # The gap this fills
//!
//! A conformance probe answers questions by calling functions whose arity nobody knows yet, so
//! **faulting is the normal case rather than the exceptional one**. Its protocol is built for
//! that: the acknowledgement is flushed before the call runs, so a fault reads as *died*
//! rather than as silence, and a command that did not answer is never recorded as having
//! answered.
//!
//! What that protocol explicitly does not cover is restarting afterwards - it says so, and
//! names the supervisor as *"a person on a console"*. This is that person, done by machine.
//!
//! # Why re-sending is cheap and rebuilding is not
//!
//! A fault costs a **re-send**, not a rebuild: the same bytes go back through the loader that
//! sent them the first time. Seconds, no toolchain. So a probing session that would otherwise
//! stop at each fault and wait for somebody to notice can keep going.
//!
//! # What this refuses to do
//!
//! **It never sends while the probe is answering.** Two copies of a probe on one target is a
//! second listener that cannot bind, or worse, one that does - and results from an unknown
//! copy are worse than no results.
//!
//! **It gives up rather than loop.** A probe that dies immediately on every start is telling
//! you something, and hammering the target hides it behind a wall of identical restarts. The
//! count is bounded and the reason is reported.
//!
//! **Every restart is announced.** The driver on the other end detects one by the session
//! identifier changing; this side knows for certain, and a restart that went unmentioned
//! would make two separate processes look like one continuous session - which is exactly the
//! discontinuity the protocol takes such care to keep visible.

use std::time::Duration;

/// The port a serving probe listens on.
///
/// From its own client documentation rather than a guess, and overridable because a build can
/// be told otherwise.
pub const PORT: u16 = 9803;

/// What the supervisor decided to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The probe is answering. Nothing to do.
    Answering,
    /// It is not answering and has not been restarted too often. Send it again.
    Resend {
        /// How many times it will have been sent, counting this one.
        attempt: usize,
    },
    /// It keeps dying, and something is wrong that re-sending will not fix.
    ///
    /// **A finding, not a failure of this code.** A probe that dies immediately every time is
    /// saying something about the last command or about the target, and the way to hear it is
    /// to stop and report rather than to keep restarting.
    GaveUp {
        /// How many times it was sent.
        after: usize,
        /// What to tell somebody.
        why: String,
    },
}

/// How a supervised probe is being kept alive.
#[derive(Debug, Clone)]
pub struct Supervisor {
    /// How many times the probe has been sent.
    sent: usize,
    /// How many restarts to allow before giving up.
    limit: usize,
    /// Restarts that happened without the probe ever answering in between.
    ///
    /// **Separate from the total**, because a probe that has answered a hundred questions and
    /// then faulted is in a different state from one that has never answered at all. The
    /// first is ordinary; the second means the build or the target is wrong.
    barren: usize,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new(Self::PATIENCE)
    }
}

impl Supervisor {
    /// How many consecutive dead starts to tolerate.
    ///
    /// Three, because one is a coincidence and two is bad luck. It is deliberately small: the
    /// cost of stopping early is asking a person, and the cost of not stopping is a target
    /// being sent the same payload forever while somebody reads a log of identical lines.
    pub const PATIENCE: usize = 3;

    /// A supervisor that has sent nothing yet.
    #[must_use]
    pub const fn new(limit: usize) -> Self {
        Self {
            sent: 0,
            limit,
            barren: 0,
        }
    }

    /// How many times the probe has been sent.
    #[must_use]
    pub const fn sent(&self) -> usize {
        self.sent
    }

    /// Decides what to do, given whether the probe is answering.
    ///
    /// **Takes the observation rather than making it**, so the decision can be tested without
    /// a target and so the same rule governs a probe on a target and one in an emulator.
    pub fn next(&mut self, answering: bool) -> Step {
        if answering {
            // Whatever went before, it is alive now, and the next fault starts a fresh count.
            self.barren = 0;
            return Step::Answering;
        }
        if self.barren >= self.limit {
            return Step::GaveUp {
                after: self.sent,
                why: format!(
                    "sent {} times and it never answered - the payload, the loader or the \
                     target is wrong, and sending it again will not say which",
                    self.sent
                ),
            };
        }
        self.sent = self.sent.saturating_add(1);
        self.barren = self.barren.saturating_add(1);
        Step::Resend { attempt: self.sent }
    }

    /// Records that the probe answered, without asking for a decision.
    ///
    /// For a caller that learned it from something other than a probe of the port - a reply on
    /// an open connection, say.
    pub const fn answered(&mut self) {
        self.barren = 0;
    }
}

/// Whether the probe's port is answering.
///
/// A plain connect. **Not a protocol exchange**: the question here is only whether something
/// is listening, and a supervisor that spoke the protocol would be a second client competing
/// with the real one for a probe that serves one at a time.
#[must_use]
pub fn is_answering(address: &str, port: u16, patience: Duration) -> bool {
    pros_link::probe(address, port, patience).open
}

#[cfg(test)]
mod tests {
    use super::{Step, Supervisor};

    /// A probe that answers is left alone.
    #[test]
    fn nothing_is_sent_to_a_probe_that_is_answering() {
        let mut supervisor = Supervisor::default();
        assert_eq!(supervisor.next(true), Step::Answering);
        assert_eq!(supervisor.sent(), 0, "nothing should have been sent");
    }

    /// **A fault after a working session is ordinary, and re-sending is the whole point.**
    #[test]
    fn a_probe_that_died_is_sent_again() {
        let mut supervisor = Supervisor::default();
        assert_eq!(supervisor.next(true), Step::Answering);
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 1 });
        assert_eq!(supervisor.next(true), Step::Answering);
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 2 });
    }

    /// **A probe that answers between faults can fault forever.**
    ///
    /// That is a probing session working as intended - each fault is one answered question
    /// about an arity - and a limit on the total would stop the useful case rather than the
    /// broken one.
    #[test]
    fn faulting_repeatedly_is_fine_as_long_as_it_answers_in_between() {
        let mut supervisor = Supervisor::new(2);
        for _ in 0..20 {
            assert!(matches!(supervisor.next(false), Step::Resend { .. }));
            assert_eq!(supervisor.next(true), Step::Answering);
        }
        assert_eq!(supervisor.sent(), 20);
    }

    /// **A probe that never answers is a different problem, and re-sending will not fix it.**
    ///
    /// Bounded on purpose: the alternative is a target being handed the same payload forever
    /// while somebody reads a log of identical lines and concludes the tool has hung.
    #[test]
    fn a_probe_that_never_answers_is_given_up_on_rather_than_hammered() {
        let mut supervisor = Supervisor::new(3);
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 1 });
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 2 });
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 3 });

        let given_up = supervisor.next(false);
        match given_up {
            Step::GaveUp { after, why } => {
                assert_eq!(after, 3);
                assert!(why.contains("never answered"), "{why}");
                assert!(
                    why.contains("will not say which"),
                    "it should say what re-sending cannot establish: {why}"
                );
            }
            other => panic!("expected it to stop: {other:?}"),
        }
    }

    /// One answer clears the run of dead starts, so a slow first boot is not a give-up.
    #[test]
    fn answering_once_clears_the_count_of_dead_starts() {
        let mut supervisor = Supervisor::new(2);
        assert!(matches!(supervisor.next(false), Step::Resend { .. }));
        assert!(matches!(supervisor.next(false), Step::Resend { .. }));
        supervisor.answered();
        // Without the clear, this would be the give-up.
        assert!(matches!(supervisor.next(false), Step::Resend { .. }));
    }
}
