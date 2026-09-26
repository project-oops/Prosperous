//! Keeping a conformance probe alive on a target that cannot restart it.
//!
//! A probe calls functions of unknown arity, so faulting is normal; its protocol flushes the
//! acknowledgement before each call so a fault reads as died, but leaves restarting to a
//! supervisor. This is that supervisor: a fault costs a re-send of the same bytes through the
//! loader, not a rebuild. It never sends while the probe is answering (a second copy either
//! fails to bind or produces results from an unknown copy), it gives up after a bounded run
//! of starts that never answer, and every restart is announced so two processes are never
//! read as one session.

use std::time::Duration;

/// The port a serving probe listens on, from the probe's client documentation.
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
    /// A finding about the last command or the target, reported rather than retried.
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
    /// Separate from the total: a fault after answering is ordinary, while never answering
    /// means the build or the target is wrong.
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
    /// Small, because stopping early only costs asking a person.
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
    /// Takes the observation rather than making it, so the rule is testable without a target
    /// and the same for a target and an emulator.
    pub fn next(&mut self, answering: bool) -> Step {
        if answering {
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
    /// For a caller that learned it some other way, such as a reply on an open connection.
    pub const fn answered(&mut self) {
        self.barren = 0;
    }
}

/// Whether the probe's port is answering.
///
/// A plain connect, not a protocol exchange: the probe serves one client at a time, and the
/// supervisor must not compete with the real one.
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

    /// A probe that died after answering is sent again.
    #[test]
    fn a_probe_that_died_is_sent_again() {
        let mut supervisor = Supervisor::default();
        assert_eq!(supervisor.next(true), Step::Answering);
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 1 });
        assert_eq!(supervisor.next(true), Step::Answering);
        assert_eq!(supervisor.next(false), Step::Resend { attempt: 2 });
    }

    /// Faults with answers in between are never limited.
    #[test]
    fn faulting_repeatedly_is_fine_as_long_as_it_answers_in_between() {
        let mut supervisor = Supervisor::new(2);
        for _ in 0..20 {
            assert!(matches!(supervisor.next(false), Step::Resend { .. }));
            assert_eq!(supervisor.next(true), Step::Answering);
        }
        assert_eq!(supervisor.sent(), 20);
    }

    /// A probe that never answers is given up on after the limit.
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
