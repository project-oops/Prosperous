//! What this tool has done this session, and how each of it turned out.
//!
//! # Why this is not the log
//!
//! There is already a log section, and this is deliberately not it. **The log is the
//! target's** - `klogsrv` streaming what the system says about itself, including things no
//! action here caused. This is a record of what *this program* asked and what came back.
//!
//! Putting them together would be a category error with real consequences: somebody debugging
//! a failed send needs to read the target's account and this tool's account side by side and
//! know which is which. Interleaved into one stream, a line saying *sending elfldr* and a line
//! from the kernel look like one narrative, and they are two.
//!
//! # Why a record at all, when the status bar already says
//!
//! The status bar says what is happening **now**. The moment it finishes, that is gone, and
//! with it the answer to *what did I just do, and did it work?*
//!
//! Sessions here are a sequence of small target operations, most of which produce a one-line
//! result that is replaced by the next one. A check, then a fetch, then a send: by the send,
//! the check's verdict is off the screen. This keeps them, so a person can look back at the
//! sequence rather than reconstructing it.
//!
//! # What it records, and what it deliberately does not
//!
//! Every job: what it was, when it started, how long it took, and how it ended. **Not what it
//! transferred** - a copy of four hundred files is one entry saying four hundred files, not
//! four hundred entries. The queue is a record of decisions, and a record that scrolls past
//! the thing somebody is looking for is one they will not use.
//!
//! It lives for the session and is not written to disk. What a target could do at a moment
//! in the past is not a fact anybody should be reading later, which is the same reason no
//! capability here is cached.

use std::time::{Duration, Instant};

/// How an entry ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ending {
    /// Still running.
    Running,
    /// It finished, and this is what it said.
    Done(String),
    /// It did not work.
    Failed(String),
    /// It was declined before anything happened, which is not a failure.
    ///
    /// **Its own ending**, because a refusal is a decision this tool made on purpose and a
    /// failure is something going wrong. A person scanning the list for what went wrong
    /// should not have to read each one to find out which it was.
    Refused(String),
    /// Somebody asked it to stop.
    Stopped,
}

impl Ending {
    /// Whether this needs looking at.
    pub(crate) const fn is_trouble(&self) -> bool {
        matches!(self, Self::Failed(_))
    }

    /// A short word for the column.
    pub(crate) const fn word(&self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done(_) => "done",
            Self::Failed(_) => "failed",
            Self::Refused(_) => "refused",
            Self::Stopped => "stopped",
        }
    }

    /// Whatever was said about it, if anything.
    pub(crate) fn said(&self) -> Option<&str> {
        match self {
            Self::Done(text) | Self::Failed(text) | Self::Refused(text) => Some(text),
            Self::Running | Self::Stopped => None,
        }
    }
}

/// One thing this tool did.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    /// What was asked, in the words the status bar used.
    pub what: String,
    /// Which target, when one was involved.
    pub target: Option<String>,
    /// When it started, for measuring against.
    started: Instant,
    /// How long it took, once it is over.
    took: Option<Duration>,
    /// How it ended.
    pub ending: Ending,
}

impl Entry {
    /// How long this has taken, running or finished.
    pub(crate) fn elapsed(&self) -> Duration {
        self.took.unwrap_or_else(|| self.started.elapsed())
    }

    /// Whether it is still going.
    pub(crate) const fn is_running(&self) -> bool {
        matches!(self.ending, Ending::Running)
    }
}

/// How many entries are kept.
///
/// **A bound rather than a belief.** A session that runs for hours would otherwise grow this
/// without limit, and the oldest entries are the ones nobody is looking for. Old enough that
/// reaching it means somebody has done a great deal.
const KEPT: usize = 200;

/// Everything this tool has done, newest last.
#[derive(Debug, Clone, Default)]
pub(crate) struct Journal {
    entries: Vec<Entry>,
    /// Whether the panel is open.
    pub open: bool,
}

impl Journal {
    /// Records something starting.
    pub(crate) fn began(&mut self, what: String, target: Option<String>) {
        self.entries.push(Entry {
            what,
            target,
            started: Instant::now(),
            took: None,
            ending: Ending::Running,
        });
        if self.entries.len() > KEPT {
            self.entries.remove(0);
        }
    }

    /// Records how the running entry ended.
    ///
    /// **Finds the running one rather than taking the last**, because nothing guarantees the
    /// last entry is the one that just finished - and marking the wrong entry done would put a
    /// result against something that never produced it.
    pub(crate) fn ended(&mut self, ending: Ending) {
        if let Some(entry) = self.entries.iter_mut().rev().find(|e| e.is_running()) {
            entry.took = Some(entry.started.elapsed());
            entry.ending = ending;
        }
    }

    /// Everything, oldest first.
    pub(crate) fn all(&self) -> &[Entry] {
        &self.entries
    }

    /// How many went wrong.
    ///
    /// Shown on the closed bar, so the panel is worth opening without being opened first.
    pub(crate) fn troubles(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.ending.is_trouble())
            .count()
    }

    /// Forgets everything that has finished, keeping anything still running.
    pub(crate) fn clear(&mut self) {
        self.entries.retain(Entry::is_running);
    }
}

#[cfg(test)]
mod tests {
    use super::{Ending, Journal};

    /// The ordinary case: something starts, something finishes.
    #[test]
    fn a_job_is_recorded_starting_and_ending() {
        let mut journal = Journal::default();
        journal.began("checking ps5".to_owned(), Some("ps5".to_owned()));
        assert_eq!(journal.all().len(), 1);
        assert!(journal.all()[0].is_running());

        journal.ended(Ending::Done("usable".to_owned()));
        assert!(!journal.all()[0].is_running());
        assert_eq!(journal.all()[0].ending.said(), Some("usable"));
    }

    /// **A refusal is not a failure**, and the list says which without being read.
    #[test]
    fn a_refusal_reads_differently_from_a_failure() {
        let mut journal = Journal::default();
        journal.began("restoring".to_owned(), None);
        journal.ended(Ending::Refused("another account wrote it".to_owned()));

        assert_eq!(journal.all()[0].ending.word(), "refused");
        assert!(!journal.all()[0].ending.is_trouble());
        assert_eq!(journal.troubles(), 0);
    }

    /// A failure counts, so the closed bar can say the panel is worth opening.
    #[test]
    fn what_went_wrong_is_counted_for_the_closed_bar() {
        let mut journal = Journal::default();
        journal.began("sending".to_owned(), None);
        journal.ended(Ending::Failed("refused: 550".to_owned()));
        journal.began("checking".to_owned(), None);
        journal.ended(Ending::Done(String::new()));

        assert_eq!(journal.troubles(), 1);
    }

    /// **The running entry is the one that ends**, not the last one added.
    ///
    /// Taking the last would put a result against an entry that never produced it, and the
    /// record would read as though something succeeded when something else did.
    #[test]
    fn ending_marks_the_job_that_was_running() {
        let mut journal = Journal::default();
        journal.began("first".to_owned(), None);
        journal.ended(Ending::Done("a".to_owned()));
        journal.began("second".to_owned(), None);
        journal.ended(Ending::Done("b".to_owned()));

        assert_eq!(journal.all()[0].ending.said(), Some("a"));
        assert_eq!(journal.all()[1].ending.said(), Some("b"));
    }

    /// Ending with nothing running does not invent an entry.
    #[test]
    fn ending_nothing_records_nothing() {
        let mut journal = Journal::default();
        journal.ended(Ending::Done("out of nowhere".to_owned()));
        assert!(journal.all().is_empty());
    }

    /// **Clearing keeps what is still running.** Removing an entry for a job that is still
    /// going would lose its result when it arrives, and the record would end mid-sentence.
    #[test]
    fn clearing_keeps_what_has_not_finished() {
        let mut journal = Journal::default();
        journal.began("finished".to_owned(), None);
        journal.ended(Ending::Done(String::new()));
        journal.began("still going".to_owned(), None);

        journal.clear();
        assert_eq!(journal.all().len(), 1);
        assert_eq!(journal.all()[0].what, "still going");
    }

    /// The oldest go first when the bound is reached, and it is the oldest.
    #[test]
    fn a_long_session_forgets_the_oldest_first() {
        let mut journal = Journal::default();
        for at in 0..super::KEPT + 5 {
            journal.began(format!("job {at}"), None);
            journal.ended(Ending::Done(String::new()));
        }
        assert_eq!(journal.all().len(), super::KEPT);
        assert_eq!(journal.all()[0].what, "job 5");
    }
}
