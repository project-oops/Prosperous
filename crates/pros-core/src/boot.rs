//! Editing what the target loads at startup.
//!
//! # What the file actually looks like
//!
//! Read off a target rather than assumed:
//!
//! ```text
//! !3000
//! kstuff-lite_v1.09.elf
//! !3000
//! nanodns.elf
//! !3000
//! elfldr_v0.24.elf
//! ```
//!
//! A `!` line is a delay. That was *by every appearance* until the manager's own source was
//! read, and now it is measured: `atoi(line + 1)` then `usleep(delay * 1000)`, so the number
//! is milliseconds and a value of zero does nothing.
//!
//! [`crate::chain::Chain`] reads the same file and throws the delays away, because it is
//! answering *what loads*. This one keeps them, because it is going to write the file back and
//! **an edit that dropped every wait would change the startup timing of a machine somebody
//! asked to reorder.**
//!
//! # Disabling an entry, and the guess that was wrong
//!
//! This module used to say an entry could only be removed, on the grounds that a line the
//! manager did not understand *might* be taken as a filename, fail, and stop the chain - which
//! would be found out at the next restart on a machine that then came up without its file
//! service.
//!
//! **That was a guess, and reading the source showed it was wrong in the direction that
//! matters.** The loop is:
//!
//! ```c
//! if (payload_mgr_resolve_path(line, full_path, sizeof(full_path)) == 0) {
//!     ps5_launch_elf(full_path);
//! } else {
//!     pldmgr_log("[Autoload] !!! Payload not found: %s\n", line);
//! }
//! ```
//!
//! A name it cannot resolve is **logged and skipped**, and the loop continues to the next
//! line. Nothing stops. So a commented-out entry is an unresolvable name, which is a line in
//! the log and nothing else.
//!
//! That is why [`crate::boot::Boot::disable`] exists and why the fear that prevented it is written down
//! here rather than quietly deleted: the caution was reasonable and it was still a guess, and
//! the thing that settled it was reading the program rather than reasoning about it.
//!
//! The prefix is `#` because it is conventional, and because **the manager reserves `!` for
//! itself** - a disabled line must not resolve to a payload and must not look like a delay.

use crate::autoload::Change;

/// What marks a line the manager will not resolve.
///
/// Conventional, and chosen so it cannot collide: `!` is the manager's own prefix for a delay,
/// and a disabled line must be neither a delay nor a resolvable filename.
pub const DISABLED: &str = "#";

/// One thing the manager does at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The instruction that precedes it, kept verbatim.
    ///
    /// **Carried whole rather than parsed into a number.** `!3000` is a wait by appearance and
    /// nothing here has confirmed the units, so re-emitting exactly what was read cannot get
    /// them wrong.
    pub before: Option<String>,
    /// The file the manager loads.
    pub payload: String,
}

impl Step {
    /// Whether this one is turned off.
    #[must_use]
    pub fn is_disabled(&self) -> bool {
        self.payload.starts_with(DISABLED)
    }

    /// The filename, without whatever marks it as disabled.
    #[must_use]
    pub fn name(&self) -> &str {
        self.payload.trim_start_matches(DISABLED).trim()
    }
}

/// The startup list, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Boot {
    /// The steps, first to last.
    pub steps: Vec<Step>,
    /// The file as it was read, for producing a change against.
    was: String,
    /// Anything after the last payload, kept so it is not lost.
    trailing: Vec<String>,
}

impl Boot {
    /// Reads the file.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut steps: Vec<Step> = Vec::new();
        let mut pending: Option<String> = None;
        let mut trailing = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('!') {
                // Two instructions in a row: the first belongs to nothing, and dropping it
                // would change what the manager does. Kept as its own step-less line.
                if let Some(orphan) = pending.replace(trimmed.to_owned()) {
                    trailing.push(orphan);
                }
                continue;
            }
            steps.push(Step {
                before: pending.take(),
                payload: trimmed.to_owned(),
            });
        }
        // An instruction after the last payload has nothing to precede.
        if let Some(last) = pending {
            trailing.push(last);
        }
        Self {
            steps,
            was: text.to_owned(),
            trailing,
        }
    }

    /// Moves one step earlier, if it is not already first.
    ///
    /// Returns whether anything moved, so a caller can leave the button alone rather than
    /// reporting a change that did not happen.
    pub fn earlier(&mut self, at: usize) -> bool {
        if at == 0 || at >= self.steps.len() {
            return false;
        }
        self.steps.swap(at - 1, at);
        true
    }

    /// Moves one step later, if it is not already last.
    pub fn later(&mut self, at: usize) -> bool {
        if at + 1 >= self.steps.len() {
            return false;
        }
        self.steps.swap(at, at + 1);
        true
    }

    /// Turns one off without losing it, or back on.
    ///
    /// A disabled entry keeps its place and its delay, and comes back where it was. **That is
    /// the whole point of it over removing**: the order is the part somebody spent thought on.
    ///
    /// Safe because the manager logs an unresolvable name and carries on - see the module
    /// note, which also records that this was believed unsafe until its source was read.
    pub fn disable(&mut self, at: usize, off: bool) -> bool {
        let Some(step) = self.steps.get_mut(at) else {
            return false;
        };
        let disabled = step.payload.starts_with(DISABLED);
        if disabled == off {
            return false;
        }
        if off {
            step.payload = format!("{DISABLED}{}", step.payload);
        } else {
            step.payload = step.payload.trim_start_matches(DISABLED).trim().to_owned();
        }
        true
    }

    /// Takes one out.
    pub fn remove(&mut self, at: usize) -> bool {
        if at >= self.steps.len() {
            return false;
        }
        self.steps.remove(at);
        true
    }

    /// Puts one on the end, copying whatever instruction the others use.
    ///
    /// **The instruction is copied rather than invented.** Every measured entry is preceded by
    /// the same one, and a new entry without it would be the only step that behaves
    /// differently - for no reason a person asked for. When the list is empty there is nothing
    /// to copy and the entry goes in bare, which is the honest version of not knowing.
    pub fn add(&mut self, payload: &str) -> bool {
        let payload = payload.trim();
        if payload.is_empty() || self.steps.iter().any(|step| step.payload == payload) {
            return false;
        }
        let before = self
            .steps
            .last()
            .and_then(|step| step.before.clone())
            .or_else(|| self.steps.first().and_then(|step| step.before.clone()));
        self.steps.push(Step {
            before,
            payload: payload.to_owned(),
        });
        true
    }

    /// The file as it would be written.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for step in &self.steps {
            if let Some(before) = &step.before {
                out.push_str(before);
                out.push('\n');
            }
            out.push_str(&step.payload);
            out.push('\n');
        }
        for line in &self.trailing {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// The edit, ready to be looked at before it is written.
    ///
    /// `None` when nothing would change, so a confirm is never shown for a write that would do
    /// nothing - the same rule the settings editor follows, and for the same reason.
    #[must_use]
    pub fn change(&self) -> Option<Change> {
        let now = self.to_text();
        if now.trim() == self.was.trim() {
            return None;
        }
        Some(Change {
            was: self.was.clone(),
            now,
            what: format!("{} entries in the startup list", self.steps.len()),
            into: crate::chain::PATH.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Boot;

    /// Exactly what a target had in the file.
    fn measured() -> &'static str {
        "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\nelfldr_v0.24.elf\n"
    }

    /// The instructions belong to the entries after them, and survive a round trip.
    #[test]
    fn the_file_reads_back_exactly_as_it_was_written() {
        let boot = Boot::parse(measured());
        assert_eq!(boot.steps.len(), 3);
        assert_eq!(boot.steps[0].payload, "kstuff-lite_v1.09.elf");
        assert_eq!(boot.steps[0].before.as_deref(), Some("!3000"));
        assert_eq!(boot.to_text(), measured());
        assert!(boot.change().is_none(), "an untouched file is not a change");
    }

    /// **Reordering moves the instruction with its entry.**
    ///
    /// They are one step. Moving a filename and leaving its wait behind would change the
    /// startup timing of a machine somebody only asked to reorder.
    #[test]
    fn a_step_takes_its_instruction_with_it() {
        let mut boot = Boot::parse(measured());
        assert!(boot.later(0));

        let text = boot.to_text();
        assert!(
            text.starts_with("!3000\nnanodns.elf\n!3000\nkstuff-lite_v1.09.elf\n"),
            "{text}"
        );
        assert_eq!(boot.steps.len(), 3, "reordering does not lose one");
    }

    /// **The ends do not wrap.** Moving the first entry up would silently make it last, which
    /// is the opposite of what somebody pressing it wants.
    #[test]
    fn the_first_cannot_go_up_and_the_last_cannot_go_down() {
        let mut boot = Boot::parse(measured());
        assert!(!boot.earlier(0));
        assert!(!boot.later(2));
        assert_eq!(boot.to_text(), measured(), "nothing should have moved");
    }

    /// A new entry copies the instruction the others use, rather than going in bare and
    /// behaving differently for no stated reason.
    #[test]
    fn a_new_entry_gets_the_same_instruction_as_the_rest() {
        let mut boot = Boot::parse(measured());
        assert!(boot.add("shsrv_v0.20.elf"));
        let last = boot.steps.last().expect("just added");
        assert_eq!(last.before.as_deref(), Some("!3000"));
        assert!(boot.to_text().ends_with("!3000\nshsrv_v0.20.elf\n"));
    }

    /// **Adding the same thing twice does nothing**, because a list that loads one payload
    /// twice is a list nobody meant to write.
    #[test]
    fn the_same_payload_is_not_added_again() {
        let mut boot = Boot::parse(measured());
        assert!(!boot.add("nanodns.elf"));
        assert!(!boot.add("   "));
        assert_eq!(boot.steps.len(), 3);
    }

    /// Removing takes the instruction with it, for the same reason moving does.
    #[test]
    fn removing_an_entry_takes_its_instruction_too() {
        let mut boot = Boot::parse(measured());
        assert!(boot.remove(1));
        let text = boot.to_text();
        assert!(!text.contains("nanodns"), "{text}");
        assert_eq!(text.matches("!3000").count(), 2, "one wait went with it");
    }

    /// **A change is shown before it is written**, and says what it would do.
    #[test]
    fn an_edit_becomes_a_change_that_can_be_read_first() {
        let mut boot = Boot::parse(measured());
        boot.remove(0);
        let change = boot.change().expect("something changed");
        assert!(change.is_real());
        assert!(change.was.contains("kstuff-lite"));
        assert!(!change.now.contains("kstuff-lite"));
    }

    /// An instruction with no entry after it is kept rather than dropped, because nothing
    /// here knows what the manager does with it and losing it is a change nobody asked for.
    #[test]
    fn an_instruction_with_nothing_after_it_is_not_lost() {
        let boot = Boot::parse("!3000\nelfldr.elf\n!5000\n");
        assert_eq!(boot.steps.len(), 1);
        assert!(boot.to_text().ends_with("!5000\n"), "{}", boot.to_text());
        assert!(boot.change().is_none(), "reading is not editing");
    }

    /// **An entry can be turned off and back on, keeping its place and its delay.**
    ///
    /// The order is the part somebody spent thought on, which is the whole reason this exists
    /// rather than removing and retyping.
    #[test]
    fn an_entry_can_be_turned_off_without_losing_where_it_was() {
        let mut boot = Boot::parse(measured());
        assert!(boot.disable(1, true));

        assert!(boot.steps[1].is_disabled());
        assert_eq!(boot.steps[1].name(), "nanodns.elf");
        assert_eq!(
            boot.steps[1].before.as_deref(),
            Some("!3000"),
            "its delay stays"
        );
        assert_eq!(boot.steps.len(), 3, "and its place stays");
        assert!(boot.to_text().contains("#nanodns.elf"));

        assert!(boot.disable(1, false));
        assert_eq!(
            boot.to_text(),
            measured(),
            "back on is back to exactly as it was"
        );
    }

    /// Turning off what is already off changes nothing, so no confirm is raised for a write
    /// that would do nothing.
    #[test]
    fn turning_off_something_already_off_is_not_a_change() {
        let mut boot = Boot::parse(measured());
        assert!(boot.disable(0, true));
        assert!(!boot.disable(0, true));
        assert!(
            !boot.disable(9, true),
            "and an entry that is not there is not a change"
        );
    }

    /// A disabled entry reads back as disabled, so a file written and read again agrees with
    /// itself.
    #[test]
    fn a_disabled_entry_survives_a_round_trip() {
        let boot = Boot::parse("!3000\n#nanodns.elf\n!3000\nelfldr_v0.24.elf\n");
        assert_eq!(boot.steps.len(), 2, "a disabled entry is still a step");
        assert!(boot.steps[0].is_disabled());
        assert_eq!(boot.steps[0].name(), "nanodns.elf");
        assert!(!boot.steps[1].is_disabled());
        assert!(boot.change().is_none());
    }
}
