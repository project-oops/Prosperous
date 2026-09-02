//! The payload manager's own settings, and changing them.
//!
//! # What this is
//!
//! Beside the boot list the manager keeps a settings file: `AUTOLOAD_ENABLED=1`,
//! `AUTOLOAD_DELAY=5`, and a handful more. Measured on a target rather than taken from
//! anywhere - the file was read and its keys are what it actually contained.
//!
//! # Why editing it needs more care than reading it
//!
//! This is the first thing in this project that would **write** to a target. Everything
//! until now has asked questions.
//!
//! The boot list decides what loads at startup, so a file written wrongly is a target that
//! comes up without its file service, its shell, or its loader - which is exactly the state
//! where a tool that talks over those services can no longer fix anything. The recovery is
//! re-running the jailbreak by hand.
//!
//! So three rules, all of them about the same fear:
//!
//! 1. **Nothing is written that was not read first.** An edit is applied to the text that
//!    came off the target, in memory, and what goes back is that text - not something
//!    regenerated from a parsed model. Comments, ordering and directives a person put there
//!    survive because they were never taken apart.
//! 2. **What would change is shown before it changes.** [`crate::autoload::Change::diff`]
//!    renders it line by line, so somebody confirms a specific edit rather than an intention.
//! 3. **The old text comes back with the change.** Keeping it is what makes an undo possible
//!    at all, and a tool that can put a target into this state and not out of it again is
//!    worse than one that refuses.

use std::collections::BTreeMap;

/// Where the manager keeps its settings.
///
/// Measured on a target on 2026-08-26, beside the boot list.
pub const CONFIG: &str = "/data/pldmgr/pldmgr_config.txt";

/// The settings, as read.
///
/// **The original text is kept whole.** Settings are edited by rewriting the one line that
/// changed, so anything the file carries that this does not understand - a comment, a key
/// added by a later version - goes back exactly as it arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// The file as it came off the target.
    text: String,
    /// What could be read out of it.
    values: BTreeMap<String, String>,
}

impl Settings {
    /// Reads the settings out of the file's text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut values = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            // A comment or a blank is kept in the text and is not a setting.
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                values.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }
        Self {
            text: text.to_owned(),
            values,
        }
    }

    /// Every setting, in name order.
    #[must_use]
    pub fn all(&self) -> &BTreeMap<String, String> {
        &self.values
    }

    /// One setting, as text.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Whether a setting reads as on.
    ///
    /// `1` is on and anything else is off, which is what the file uses. `None` when the key is
    /// absent - **not `false`**, because a setting this version of the manager does not have
    /// is a different thing from one it has turned off.
    #[must_use]
    pub fn is_on(&self, key: &str) -> Option<bool> {
        self.values.get(key).map(|value| value.trim() == "1")
    }

    /// The file as it stands.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// A change to one setting, ready to be looked at before it is applied.
    ///
    /// Returns `None` when the value is already that, so a confirm dialog is never shown for
    /// a write that would do nothing - which is the same class of dishonesty as a progress
    /// bar for work that is not happening.
    #[must_use]
    pub fn set(&self, key: &str, value: &str) -> Option<Change> {
        if self.get(key) == Some(value) {
            return None;
        }
        let mut written = false;
        let mut lines: Vec<String> = Vec::new();
        for line in self.text.lines() {
            let names_it = line
                .split_once('=')
                .is_some_and(|(name, _)| name.trim() == key);
            if names_it && !written {
                lines.push(format!("{key}={value}"));
                written = true;
            } else {
                lines.push(line.to_owned());
            }
        }
        // A key the file did not have is appended rather than dropped silently.
        if !written {
            lines.push(format!("{key}={value}"));
        }
        Some(Change {
            was: self.text.clone(),
            now: lines.join("\n") + "\n",
            what: format!("{key} = {value}"),
            into: CONFIG.to_owned(),
        })
    }
}

/// A pending edit to a file on the target.
///
/// **Carries both texts.** The new one is what would be written; the old one is what makes
/// putting it back possible, and a change that could not be undone is one nobody should be
/// asked to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The file as it is now on the target.
    pub was: String,
    /// The file as it would be.
    pub now: String,
    /// What the edit was, in one line.
    pub what: String,
    /// Which file it would be written to.
    ///
    /// **Carried with the change**, because there are now two editable files here and a diff
    /// that did not say which one it belonged to could be confirmed against the wrong one.
    ///
    /// Owned: a list's path comes from the chain that declares it, read from a file, rather
    /// than from a constant in this program.
    pub into: String,
}

impl Change {
    /// The change, line by line.
    ///
    /// **Shown before anything is written**, because confirming *"change the autoload delay"*
    /// and confirming *these two lines* are different acts, and only the second one catches a
    /// tool that is about to do something else as well.
    ///
    /// # Why this is a real diff and not a walk down two lists
    ///
    /// It used to compare line 1 with line 1, line 2 with line 2, and so on. That is correct
    /// only while nothing is inserted or deleted: **one removal shifts everything below it**,
    /// and every following line then differs from the one it is paired with. Removing a single
    /// entry from a startup list of six rendered as four removals and four additions, none of
    /// which were happening.
    ///
    /// That is not a cosmetic problem. This panel is the last thing between somebody and a
    /// write to their target, and it was describing a different change from the one about to
    /// be made - which is this project's defect exactly, in the one place built to catch it.
    ///
    /// So the longest common subsequence is found first, and everything not in it is an
    /// addition or a removal. The lists here are tens of lines, so the quadratic table costs
    /// nothing worth measuring.
    #[must_use]
    pub fn diff(&self) -> Vec<Line> {
        let before: Vec<&str> = self.was.lines().collect();
        let after: Vec<&str> = self.now.lines().collect();
        let (rows, columns) = (before.len(), after.len());

        // `common[i][j]` is the length of the longest common subsequence of what is left of
        // each side from `i` and `j` onwards. Built from the end so the walk below can read it
        // forwards and keep the output in file order.
        let mut common = vec![vec![0_usize; columns + 1]; rows + 1];
        for i in (0..rows).rev() {
            for j in (0..columns).rev() {
                common[i][j] = if before[i] == after[j] {
                    common[i + 1][j + 1] + 1
                } else {
                    common[i + 1][j].max(common[i][j + 1])
                };
            }
        }

        let mut out = Vec::new();
        let (mut i, mut j) = (0, 0);
        while i < rows && j < columns {
            if before[i] == after[j] {
                out.push(Line::Same(before[i].to_owned()));
                i += 1;
                j += 1;
            } else if common[i + 1][j] >= common[i][j + 1] {
                out.push(Line::Gone(before[i].to_owned()));
                i += 1;
            } else {
                out.push(Line::Added(after[j].to_owned()));
                j += 1;
            }
        }
        out.extend(
            before[i..]
                .iter()
                .map(|line| Line::Gone((*line).to_owned())),
        );
        out.extend(
            after[j..]
                .iter()
                .map(|line| Line::Added((*line).to_owned())),
        );
        out
    }

    /// Whether this would actually change the file.
    #[must_use]
    pub fn is_real(&self) -> bool {
        self.was.trim() != self.now.trim()
    }
}

/// One line of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// Unchanged.
    Same(String),
    /// Would be removed.
    Gone(String),
    /// Would be added.
    Added(String),
}

#[cfg(test)]
mod tests {
    use super::{Line, Settings};

    /// The file as a target actually had it.
    fn measured() -> &'static str {
        "AUTOLOAD_ENABLED=1\n\
         LAST_REPOSITORY_UPDATE=1787678611\n\
         AUTO_BROWSER_OPEN=1\n\
         AUTOLOAD_DELAY=5\n\
         KILL_DISC_PLAYER_ON_STARTUP=1\n\
         SCAN_USB_PAYLOADS=1\n"
    }

    /// Every setting the measured file carried is read.
    #[test]
    fn the_settings_a_target_had_are_read() {
        let settings = Settings::parse(measured());
        assert_eq!(settings.all().len(), 6);
        assert_eq!(settings.get("AUTOLOAD_DELAY"), Some("5"));
        assert_eq!(settings.is_on("AUTOLOAD_ENABLED"), Some(true));
        assert_eq!(settings.is_on("SCAN_USB_PAYLOADS"), Some(true));
    }

    /// **A setting the file does not have is unknown, not off.**
    ///
    /// A manager that never had the key and one that has it turned off are different
    /// targets, and showing both as an unticked box would make them look the same.
    #[test]
    fn a_setting_that_is_not_there_is_not_off() {
        let settings = Settings::parse(measured());
        assert_eq!(settings.is_on("SOMETHING_ELSE"), None);
    }

    /// **Only the line that changed changes.** Everything else goes back byte for byte,
    /// including keys this does not understand.
    #[test]
    fn changing_one_setting_leaves_every_other_line_alone() {
        let settings = Settings::parse(measured());
        let change = settings.set("AUTOLOAD_DELAY", "10").expect("it differs");

        let diff = change.diff();
        let gone: Vec<&Line> = diff
            .iter()
            .filter(|line| matches!(line, Line::Gone(_)))
            .collect();
        let added: Vec<&Line> = diff
            .iter()
            .filter(|line| matches!(line, Line::Added(_)))
            .collect();
        assert_eq!(gone.len(), 1, "one line should go: {diff:?}");
        assert_eq!(added.len(), 1, "and one should arrive: {diff:?}");
        assert_eq!(gone[0], &Line::Gone("AUTOLOAD_DELAY=5".to_owned()));
        assert_eq!(added[0], &Line::Added("AUTOLOAD_DELAY=10".to_owned()));
        assert!(change.now.contains("LAST_REPOSITORY_UPDATE=1787678611"));
    }

    /// **Setting a value to what it already is produces no change at all.**
    ///
    /// Not an empty one: none. A confirm dialog for a write that would do nothing teaches
    /// somebody to click through confirms, which is precisely the habit that makes the real
    /// one dangerous.
    #[test]
    fn setting_a_value_to_itself_is_not_a_change() {
        let settings = Settings::parse(measured());
        assert!(settings.set("AUTOLOAD_DELAY", "5").is_none());
    }

    /// A key the file never had is appended rather than lost.
    #[test]
    fn a_new_key_is_added_rather_than_dropped() {
        let settings = Settings::parse(measured());
        let change = settings.set("NEW_THING", "1").expect("it is new");
        assert!(change.now.ends_with("NEW_THING=1\n"));
        assert!(change.is_real());
    }

    /// Comments and blank lines are not settings, and survive an edit.
    #[test]
    fn what_is_not_a_setting_is_carried_through_untouched() {
        let text = "# written by hand\n\nAUTOLOAD_DELAY=5\n";
        let settings = Settings::parse(text);
        assert_eq!(settings.all().len(), 1);

        let change = settings.set("AUTOLOAD_DELAY", "9").expect("it differs");
        assert!(change.now.starts_with("# written by hand\n\n"));
    }
}

#[cfg(test)]
mod diffing {
    use super::{Change, Line};

    fn change(was: &str, now: &str) -> Change {
        Change {
            what: "the startup list".to_owned(),
            was: was.to_owned(),
            now: now.to_owned(),
            into: "/data/pldmgr/autoload.txt".to_owned(),
        }
    }

    fn gone(lines: &[Line]) -> Vec<&str> {
        lines
            .iter()
            .filter_map(|line| match line {
                Line::Gone(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    fn added(lines: &[Line]) -> Vec<&str> {
        lines
            .iter()
            .filter_map(|line| match line {
                Line::Added(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// **One removal renders as one removal.**
    ///
    /// The real list, and the real fix applied to it. Comparing line by line, this showed four
    /// removals and four additions - describing a change nobody had asked for, in the panel
    /// that exists to be checked before a write.
    #[test]
    fn removing_one_entry_does_not_look_like_moving_four() {
        let was = "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\nelfldr_v0.24.elf\n\
                   !3000\nShadowMountPlus_1.6beta16.elf\n!3000\nps5upload-4.1.2.elf\n";
        let now = "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\n\
                   ShadowMountPlus_1.6beta16.elf\n!3000\nps5upload-4.1.2.elf\n";
        let lines = change(was, now).diff();
        // Two lines, because removing an entry takes its `!3000` instruction with it - which
        // is what the editor does and what the file therefore looks like afterwards.
        assert_eq!(gone(&lines).len(), 2, "{lines:?}");
        assert!(
            gone(&lines).contains(&"elfldr_v0.24.elf"),
            "the entry itself: {lines:?}"
        );
        assert!(added(&lines).is_empty(), "nothing was added: {lines:?}");
    }

    /// Appending shows only what was appended, however much came before it.
    #[test]
    fn appending_shows_only_the_new_lines() {
        let was = "ftpsrv_v0.21.elf\nshsrv_v0.20.elf\n";
        let now = "ftpsrv_v0.21.elf\nshsrv_v0.20.elf\n!3000\nklogsrv_v0.9.elf\n";
        let lines = change(was, now).diff();
        assert!(gone(&lines).is_empty(), "{lines:?}");
        assert_eq!(added(&lines), ["!3000", "klogsrv_v0.9.elf"]);
    }

    /// A line genuinely replaced is still one out and one in.
    #[test]
    fn a_replaced_line_is_one_out_and_one_in() {
        let lines = change("AUTOLOAD_DELAY=5\n", "AUTOLOAD_DELAY=9\n").diff();
        assert_eq!(gone(&lines), ["AUTOLOAD_DELAY=5"]);
        assert_eq!(added(&lines), ["AUTOLOAD_DELAY=9"]);
    }

    /// **What the diff says is added, applied to what was there, is what will be written.**
    ///
    /// The property that makes the panel worth reading at all: reconstructing the file from
    /// the diff has to give back exactly the text about to be sent.
    #[test]
    fn the_diff_reconstructs_the_file_about_to_be_written() {
        let was = "!3000\na.elf\n!3000\nb.elf\n!3000\nc.elf\n";
        let now = "!3000\na.elf\n!3000\nc.elf\n!3000\nd.elf\n";
        let rebuilt: Vec<String> = change(was, now)
            .diff()
            .into_iter()
            .filter_map(|line| match line {
                Line::Same(text) | Line::Added(text) => Some(text),
                Line::Gone(_) => None,
            })
            .collect();
        assert_eq!(rebuilt.join("\n"), now.trim_end());
    }
}

/// One entry of a startup list, as it stands and as it would be.
///
/// # Why both positions rather than one list or the other
///
/// A panel that showed only the **pending** list could not show a removal: the entry is not in
/// it, so there is no row, and an account of it had to go somewhere else - which read as
/// *removing something that is not there*, because that is what it said.
///
/// A panel that showed only the **current** list could not show an addition.
///
/// So a row carries where it is now and where it would be, and either can be absent. Nothing
/// about a change then needs explaining outside the table it happened in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// Its position in the list on the target, if it is in that list.
    pub was_at: Option<usize>,
    /// Its position after saving, if it would still be in the list.
    pub now_at: Option<usize>,
    /// The entry, as the file spells it.
    pub payload: String,
}

impl Shown {
    /// Whether saving would take this out of the list.
    #[must_use]
    pub const fn removed(&self) -> bool {
        self.now_at.is_none()
    }

    /// Whether saving would put this into the list.
    #[must_use]
    pub const fn added(&self) -> bool {
        self.was_at.is_none()
    }

    /// Whether it stays but loads at a different point.
    ///
    /// **Order is not decoration here.** The manager loads the list top to bottom, so moving an
    /// entry changes what is running by the time the next one starts.
    #[must_use]
    pub fn moved(&self) -> bool {
        match (self.was_at, self.now_at) {
            (Some(was), Some(now)) => was != now,
            _ => false,
        }
    }
}

impl Change {
    /// The list as it stands, with what would change marked in place.
    ///
    /// Instructions - the `!` lines - are left out: they belong to the entry below them and
    /// are shown beside it, not as rows of their own.
    #[must_use]
    pub fn shown(&self) -> Vec<Shown> {
        let mut rows = Vec::new();
        let (mut was_at, mut now_at) = (0, 0);
        for line in self.diff() {
            let (text, in_old, in_new) = match line {
                Line::Same(text) => (text, true, true),
                Line::Gone(text) => (text, true, false),
                Line::Added(text) => (text, false, true),
            };
            if text.trim_start().starts_with('!') || text.trim().is_empty() {
                continue;
            }
            rows.push(Shown {
                was_at: in_old.then_some(was_at),
                now_at: in_new.then_some(now_at),
                payload: text,
            });
            if in_old {
                was_at += 1;
            }
            if in_new {
                now_at += 1;
            }
        }
        rows
    }
}

#[cfg(test)]
mod showing {
    use super::{Change, Shown};

    fn change(was: &str, now: &str) -> Change {
        Change {
            what: "the startup list".to_owned(),
            was: was.to_owned(),
            now: now.to_owned(),
            into: "/data/pldmgr/autoload.txt".to_owned(),
        }
    }

    /// **A removed entry keeps its row and its number.**
    ///
    /// This is the whole point: it is in the list on the target, so it appears in the list,
    /// marked. Dropping the row and accounting for it underneath read as *removing something
    /// that is not there* - which was a fair description of what it said.
    #[test]
    fn a_removed_entry_stays_in_the_table_with_its_position() {
        let rows = change(
            "!3000\na.elf\n!3000\nb.elf\n!3000\nc.elf\n",
            "!3000\na.elf\n!3000\nc.elf\n",
        )
        .shown();
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert_eq!(rows[1].payload, "b.elf");
        assert_eq!(rows[1].was_at, Some(1), "where it is now");
        assert!(rows[1].removed());
        assert!(!rows[1].added());
    }

    /// An added entry has no current position and does have a new one.
    #[test]
    fn an_added_entry_has_only_a_new_position() {
        let rows = change("a.elf\n", "a.elf\nb.elf\n").shown();
        assert_eq!(rows[1].payload, "b.elf");
        assert_eq!(rows[1].was_at, None);
        assert_eq!(rows[1].now_at, Some(1));
        assert!(rows[1].added());
    }

    /// **What follows a removal keeps its old number and shows a new one.** The manager loads
    /// the list in order, so moving an entry changes what is up by the time the next starts.
    #[test]
    fn what_shifts_carries_both_positions() {
        let rows = change("a.elf\nb.elf\nc.elf\n", "b.elf\nc.elf\n").shown();
        let c = rows.iter().find(|row| row.payload == "c.elf").expect("c");
        assert_eq!(c.was_at, Some(2));
        assert_eq!(c.now_at, Some(1));
        assert!(c.moved());
    }

    /// Instructions are not rows: they belong to the entry below them.
    #[test]
    fn a_delay_is_not_an_entry() {
        let rows = change("!3000\na.elf\n", "!5000\na.elf\n").shown();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].payload, "a.elf");
    }

    /// An unchanged list is every entry, unmarked, in its own position.
    #[test]
    fn an_unchanged_list_marks_nothing() {
        let text = "!3000\na.elf\n!3000\nb.elf\n";
        let rows: Vec<Shown> = change(text, text).shown();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| !row.added() && !row.removed()));
        assert!(rows.iter().all(|row| !row.moved()));
    }
}

#[cfg(test)]
mod toggling {
    use super::Settings;

    const FILE: &str = "AUTOLOAD_ENABLED=1\nKILL_DISC_PLAYER_ON_STARTUP=1\nAUTOLOAD_DELAY=5\n";

    /// **A setting turned back to what the target has produces the original file.**
    ///
    /// The window compares the two to decide there is nothing left to write. Without it, a
    /// box unticked and re-ticked left a pending change that said nothing had changed, and the
    /// only way out was discarding every edit.
    #[test]
    fn setting_a_value_back_gives_the_original_file() {
        let settings = Settings::parse(FILE);
        let off = settings
            .set("KILL_DISC_PLAYER_ON_STARTUP", "0")
            .expect("that is a change");
        let back = Settings::parse(&off.now)
            .set("KILL_DISC_PLAYER_ON_STARTUP", "1")
            .expect("and back again");
        assert_eq!(back.now.trim(), FILE.trim());
    }

    /// An edit applied to a pending edit keeps the earlier one.
    #[test]
    fn a_second_edit_does_not_lose_the_first() {
        let settings = Settings::parse(FILE);
        let first = settings.set("AUTOLOAD_DELAY", "9").expect("a change");
        let second = Settings::parse(&first.now)
            .set("KILL_DISC_PLAYER_ON_STARTUP", "0")
            .expect("another");
        let both = Settings::parse(&second.now);
        assert_eq!(both.get("AUTOLOAD_DELAY"), Some("9"));
        assert_eq!(both.get("KILL_DISC_PLAYER_ON_STARTUP"), Some("0"));
        assert_eq!(both.get("AUTOLOAD_ENABLED"), Some("1"), "untouched");
    }

    /// Setting a value it already has is not a change, so no panel appears for a write that
    /// would do nothing.
    #[test]
    fn setting_what_is_already_there_is_not_a_change() {
        assert!(Settings::parse(FILE).set("AUTOLOAD_ENABLED", "1").is_none());
    }
}
