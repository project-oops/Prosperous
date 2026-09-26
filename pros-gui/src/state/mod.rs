//! What the window is showing, and the rules about changing it.
//!
//! Every rule rather than pixel lives here, where tests reach it; `app` only draws.
//!
//! 1. One job at a time, so two answers never interleave.
//! 2. A failed job clears what it would have replaced, so nothing stale is shown as current.
//! 3. Waiting is visible and timed, so working and hung look different.

use std::time::{Duration, Instant};

use pros_core::check::Report;
use pros_core::target::Target;

mod job;
mod panels;
mod section;

use job::Panel;
pub(crate) use job::{Disturbs, Done, Job};
pub(crate) use panels::{
    AutoloadState, ControllersState, DoctorState, Exporting, FilesState, LogState, PayloadsState,
    Pending, Probing, RegisterState, Showing, StreamState,
};
pub(crate) use section::{Place, Section};

/// What is currently being waited for.
#[derive(Debug, Clone)]
pub(crate) struct Waiting {
    /// What was asked.
    pub(crate) job: Job,
    /// When it was asked.
    pub(crate) since: Instant,
}

impl Waiting {
    /// How long this has been running.
    #[must_use]
    pub(crate) fn elapsed(&self) -> Duration {
        self.since.elapsed()
    }
}

/// Everything the window is showing.
///
/// What only one panel reads is grouped under that panel; what every panel reads is here.
#[derive(Debug, Default)]
pub(crate) struct State {
    /// Targets this machine knows about.
    pub(crate) targets: Vec<Target>,
    /// Which one is selected, by position in `targets`.
    pub(crate) chosen: Option<usize>,
    /// Windows that are open.
    pub(crate) showing: Showing,
    /// Which section is on screen.
    pub(crate) section: Section,
    /// What is running, and since when.
    pub(crate) waiting: Option<Waiting>,
    /// A job that has begun and not yet been handed to the worker.
    pub(crate) pending: Option<Job>,
    /// How far a long copy has got.
    pub(crate) progress: Option<pros_core::transfer::Progress>,
    /// The last check, when one has been run.
    pub(crate) report: Option<Report>,
    /// The boot list, when it could be read.
    pub(crate) chain: Option<pros_core::chain::Chain>,
    /// Whether everything should be asked again about the target already selected.
    ///
    /// Distinct from a change of target: a refresh keeps what is on screen until the new
    /// answers arrive, a new target clears it.
    pub(crate) resurvey: bool,
    /// Which target the last check was about.
    pub(crate) checked_for: Option<String>,
    /// The last command's output.
    pub(crate) said: String,
    /// The last thing that went wrong.
    pub(crate) trouble: Option<String>,
    /// What this tool has done this session.
    ///
    /// Not the log: this is this program's account of what it asked.
    pub(crate) journal: crate::journal::Journal,
    /// What the target is, once asked.
    pub(crate) system: Option<pros_core::system::Report>,
    /// Where the two panes are split, as the left one share of the usable width.
    ///
    /// A fraction rather than pixels, so it survives a resize.
    pub(crate) split: f32,
    /// Jobs behind the one that is running.
    ///
    /// Not a second scheduler: one job still runs at a time, and [`Self::finish`] starts the
    /// next when the previous ends.
    pub(crate) queued: std::collections::VecDeque<Job>,
    /// What the last finished job may have made untrue.
    ///
    /// Set here and acted on by the window, so the worker never reaches into the display.
    pub(crate) disturbed: Vec<Disturbs>,
    /// What a person has typed into the command box.
    pub(crate) command: String,
    /// The doctor's plan and what it leaves until its transfers land.
    pub(crate) doctor: DoctorState,
    /// The startup lists, the manager's settings, and the configurator.
    pub(crate) autoload: AutoloadState,
    /// The two-sided storage sections.
    pub(crate) files: FilesState,
    /// The payloads screen.
    pub(crate) payloads: PayloadsState,
    /// The log screen.
    pub(crate) log: LogState,
    /// The controllers screen.
    pub(crate) controllers: ControllersState,
    /// The stream screen.
    pub(crate) stream: StreamState,
    /// The probe screen: what it can launch, and what the last run captured.
    pub(crate) probing: Probing,
    /// The register dialog.
    pub(crate) register: RegisterState,
}

impl State {
    /// A window that has just opened, with whatever is registered.
    #[must_use]
    pub(crate) fn new(targets: Vec<Target>) -> Self {
        let first_address = targets
            .first()
            .map_or_else(String::new, |t| t.address.clone());
        Self {
            chosen: (!targets.is_empty()).then_some(0),
            targets,
            files: FilesState {
                // Conventional and unmeasured; the window says so beside the editable box.
                library_path: "/user/app".to_owned(),
                ..FilesState::default()
            },
            autoload: AutoloadState {
                preset: pros_core::recovery::baseline::first().name,
                // Every startup list comes from the chains, shipped and user-added.
                lists: pros_core::chain::lists(),
                ..AutoloadState::default()
            },
            payloads: PayloadsState {
                // One of the two directories `payload_mgr_resolve_path` searches, as the
                // doctor's plans use.
                install_dir: pros_core::payloads::INTERNAL.to_owned(),
                ..PayloadsState::default()
            },
            register: RegisterState {
                name: "ps5".to_owned(),
                ..RegisterState::default()
            },
            controllers: ControllersState {
                // Chosen, not measured; filled in so the box is editable rather than guessed.
                feed_port: pros_link::feed::PORT.to_string(),
                ..ControllersState::default()
            },
            stream: StreamState {
                watching: pros_core::watch::Watching::idle(),
                target_ip: first_address,
                watch_port: pros_core::watch::PORT.to_string(),
                installing_player: false,
                watch_after_install: false,
            },
            split: 0.5,
            // The command line's own default for `pros probe --seconds`.
            probing: Probing {
                seconds: 120,
                ..Probing::default()
            },
            ..Self::default()
        }
    }

    /// The target being acted on.
    #[must_use]
    pub(crate) fn target(&self) -> Option<&Target> {
        self.chosen.and_then(|which| self.targets.get(which))
    }

    /// The startup list currently being shown.
    pub(crate) fn list(&self) -> pros_core::chain::Held {
        self.autoload
            .lists
            .get(self.autoload.list_at)
            .or_else(|| self.autoload.lists.first())
            .cloned()
            // Only before the lists are read; `chain::lists` returns at least one entry.
            .unwrap_or_else(|| pros_core::chain::Held {
                label: "no chain declares a list".to_owned(),
                path: String::new(),
                editable: false,
                autoloader: false,
            })
    }

    /// Whether anything may be started right now.
    #[must_use]
    pub(crate) const fn is_idle(&self) -> bool {
        self.waiting.is_none()
    }

    /// Starts a job, if nothing else is running.
    ///
    /// Answers whether it was started; refused while busy so two answers never interleave.
    pub(crate) fn begin(&mut self, job: Job) -> bool {
        if self.waiting.is_some() {
            return false;
        }
        self.trouble = None;
        self.progress = None;
        self.files.guard_refusal = None;
        // Recorded at the start, so a job that never returns still appears.
        self.journal
            .began(job.describe(), Self::target_in(&job).map(str::to_owned));
        self.pending = Some(job.clone());
        self.waiting = Some(Waiting {
            job,
            since: Instant::now(),
        });
        true
    }

    /// Starts a job, or puts it behind whatever is running.
    ///
    /// For a multi-select: where [`Self::begin`] refuses while busy, this queues.
    pub(crate) fn queue(&mut self, job: Job) {
        if self.waiting.is_some() {
            self.queued.push_back(job);
        } else {
            self.begin(job);
        }
    }

    /// How many are waiting their turn.
    #[must_use]
    pub(crate) fn queued(&self) -> usize {
        self.queued.len()
    }

    /// Whether a screen is waiting on the answer it cannot be drawn without.
    ///
    /// True only when there is nothing to show yet and an answer is on its way. A re-read
    /// leaves what is shown in place ([`Self::re_reading`]); nothing shown and nothing coming
    /// is a screen nobody has asked about.
    pub(crate) fn still_arriving(&self, section: Section) -> bool {
        self.nothing_yet(section) && self.expecting(section)
    }

    /// Whether a screen has anything at all to draw.
    fn nothing_yet(&self, section: Section) -> bool {
        match section {
            Section::Check => self.report.is_none(),
            Section::Autoload => self.autoload.boot.is_none(),
            Section::System => self.system.is_none(),
            Section::Payloads => self.payloads.there.is_none(),
            // Finding saves is a navigation into the shared listing.
            Section::Saves
            | Section::Filesystem
            | Section::Titles
            | Section::Cheats
            | Section::Packages => self.files.library.is_empty(),
            Section::Log | Section::Shell | Section::Stream | Section::Controllers => false,
            Section::Probe => self.probing.titles.is_none(),
        }
    }

    /// Whether an answer this screen needs is running or waiting its turn.
    ///
    /// Queued jobs count too: the survey on arrival queues several, and a screen whose answer
    /// is behind others must not say nobody asked.
    fn expecting(&self, section: Section) -> bool {
        let wanted = |job: &Job| job.fills().contains(&section);
        self.waiting
            .as_ref()
            .is_some_and(|running| wanted(&running.job))
            || self.queued.iter().any(wanted)
    }

    /// Whether a screen already showing something is being read again.
    ///
    /// Shown beside what is on screen, not instead of it.
    pub(crate) fn re_reading(&self, section: Section) -> bool {
        !self.nothing_yet(section) && self.expecting(section)
    }

    /// Forgets everything not yet started.
    ///
    /// The running job is not touched.
    pub(crate) fn drop_queued(&mut self) -> usize {
        std::mem::take(&mut self.queued).len()
    }

    /// Which target a job is about, when it is about one.
    fn target_in(job: &Job) -> Option<&str> {
        match job {
            Job::Check(target)
            | Job::Shell(target, _)
            | Job::Pull(target, ..)
            | Job::Browse(target, _)
            | Job::Push(target, ..)
            | Job::Install(target, ..)
            | Job::Backup(target, ..)
            | Job::Restore(target, ..)
            | Job::Send(target, ..)
            | Job::Names(target, _)
            | Job::Titles(target)
            | Job::FindSaves(target)
            | Job::Locate(target, _)
            | Job::Launch(target, _)
            | Job::RunThere(target, _)
            | Job::ReadList(target, _)
            | Job::ReadAutoload(target)
            | Job::ReadSystem(target)
            | Job::RestartUi(target)
            | Job::CloseTitle(target, _)
            | Job::EndProcess(target, _)
            | Job::InstallPackage(target, _)
            | Job::FindPayloads(target, _)
            | Job::DeleteThere(target, _)
            | Job::WriteAutoload(target, ..)
            | Job::EnableAutoload(target)
            | Job::CaptureConfig(target)
            | Job::PlaceFile(target, ..) => Some(&target.name),
            // No target involved.
            Job::Fetch(..) | Job::Relist(..) | Job::DeleteHere(..) => None,
        }
    }

    /// How a result should read in the record.
    fn how_it_went(done: &Done) -> crate::journal::Ending {
        use crate::journal::Ending;
        match done {
            Done::Failed(why) => Ending::Failed(why.clone()),
            Done::Refused(needs) => Ending::Refused(match needs {
                pros_core::origin::Needs::Resigning { wrote, .. } => {
                    format!("written by another account ({wrote})")
                }
                pros_core::origin::Needs::Unknown(why) => why.clone(),
                pros_core::origin::Needs::Nothing => String::new(),
            }),
            Done::GuardRefused(refusal) => Ending::Refused(refusal.explanation.clone()),
            // A stopped copy is recorded as stopped, not done.
            Done::Copied(summary, _)
                if summary
                    .skipped
                    .iter()
                    .any(|one| one.why.contains("stopped")) =>
            {
                Ending::Stopped
            }
            Done::Copied(summary, into) => Ending::Done(format!(
                "{} files, {} bytes to {into}{}{}",
                summary.files,
                summary.bytes,
                if summary.unchanged > 0 {
                    format!(", {} unchanged", summary.unchanged)
                } else {
                    String::new()
                },
                if summary.is_complete() {
                    String::new()
                } else {
                    format!(" - {} not copied", summary.skipped.len())
                }
            )),
            Done::Fetched(name, into) => {
                Ending::Done(format!("{name} verified into {}", into.display()))
            }
            Done::Relisted(payload, _) => Ending::Done(format!(
                "{} now describes {}",
                payload.name,
                payload.version.as_deref().unwrap_or("a new release")
            )),
            Done::Installed(said) => {
                if said.is_a_known_failure() {
                    Ending::Failed(said.describe())
                } else {
                    Ending::Done(said.describe())
                }
            }
            Done::Checked(report, _) => Ending::Done(format!("{:?}", report.verdict())),
            Done::Browsed(items) => Ending::Done(format!("{} entries", items.len())),
            Done::Named(found) => Ending::Done(format!("{} names", found.len())),
            Done::Titles(found) => Ending::Done(format!("{} titles", found.len())),
            Done::Pulled { into, bytes } => {
                Ending::Done(format!("{bytes} bytes to {}", into.display()))
            }
            Done::Located(found) => Ending::Done(match found.path() {
                Some(path) => path.to_owned(),
                None => "none of them".to_owned(),
            }),
            Done::Payloads(found) => Ending::Done(format!("{} payload files", found.len())),
            Done::Launched(said) => match said {
                pros_core::launch::Said::NotAnId | pros_core::launch::Said::Refused(_) => {
                    Ending::Failed(said.describe())
                }
                pros_core::launch::Said::Asked(_) => Ending::Done(said.describe()),
            },
            Done::RanThere(said) => match said {
                pros_core::hbldr::Said::NotFound(_) | pros_core::hbldr::Said::NoArgument => {
                    Ending::Failed(said.describe())
                }
                pros_core::hbldr::Said::Ran(_) => Ending::Done(said.describe()),
            },
            Done::List(boot) => Ending::Done(format!("{} entries", boot.steps.len())),
            Done::Autoload(settings, boot) => Ending::Done(format!(
                "{} settings, {} startup entries",
                settings.all().len(),
                boot.steps.len()
            )),
            Done::Captured(files, _) => Ending::Done(format!(
                "{} file{} the chain will carry",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            )),
            Done::System(report) => Ending::Done(format!("{} facts", report.facts.len())),
            Done::Signalled { note, .. } => Ending::Done(note.clone()),
            Done::Said(_) | Done::FoundSaves(_) => Ending::Done(String::new()),
        }
    }

    /// Whether a begun job is waiting to be handed to the worker.
    #[cfg(test)]
    fn pending_after_begin(&self) -> bool {
        self.pending.is_some()
    }

    /// Takes the result of the job that was running.
    ///
    /// A failure clears the panel the job would have filled (rule 2).
    pub(crate) fn finish(&mut self, done: Done) {
        let Some(waiting) = self.waiting.take() else {
            return;
        };
        self.journal.ended(Self::how_it_went(&done));
        // Whatever the outcome: a copy that failed part way still moved files.
        self.disturbed = waiting.job.disturbs().to_vec();
        match done {
            Done::Checked(report, chain) => {
                self.report = Some(*report);
                self.chain = chain;
            }
            // Handed up: the payload list belongs to the window.
            Done::Relisted(payload, found) => self.doctor.relisted = Some((*payload, *found)),
            Done::Browsed(items) => {
                if let Job::Browse(_, where_) = &waiting.job {
                    self.files.seen.insert(where_.clone(), items.clone());
                }
                self.files.library = items;
            }
            Done::FoundSaves(found) => self.carry_saves(found),
            Done::Named(found) => {
                for about in found {
                    if let Some(name) = about.name {
                        self.files.names.insert(about.id, name);
                    }
                }
            }
            Done::Titles(found) => self.carry_titles(found),
            Done::Copied(summary, where_to) => self.carry_copied(&summary, &where_to),
            Done::Said(text) => self.said = text,
            Done::Refused(needs) => {
                self.files.refused = Some(needs.clone());
            }
            Done::GuardRefused(refusal) => {
                self.files.guard_refusal = Some(refusal.clone());
            }
            Done::Payloads(found) => self.payloads.there = Some(found),
            Done::Launched(said) => self.said = said.describe(),
            Done::RanThere(said) => self.said = said.describe(),
            Done::List(boot) => {
                // The list only; this may not be the manager's list.
                self.autoload.boot = Some(*boot);
                self.autoload.boot_at = None;
            }
            Done::Autoload(settings, boot) => {
                self.autoload.settings = Some(*settings);
                self.autoload.boot = Some(*boot);
                self.autoload.boot_at = None;
            }
            Done::Captured(files, notes) => self.carry_captured(files, notes),
            Done::System(report) | Done::Signalled { report, .. } => {
                self.system = Some(*report);
            }
            Done::Installed(said) => {
                // Only a recognised failure is trouble; an unrecognised answer is shown as said.
                if said.is_a_known_failure() {
                    self.trouble = Some(said.describe());
                } else {
                    self.said = said.describe();
                }
            }
            Done::Located(found) => self.carry_located(&found),
            Done::Fetched(name, into) => {
                self.said = format!("{name} kept and verified: {}", into.display());
            }
            Done::Pulled { into, bytes } => {
                self.said = format!("{bytes} bytes written to {}", into.display());
            }
            Done::Failed(why) => {
                self.trouble = Some(why);
                self.clear(waiting.job.replaces());
            }
        }
        self.next_in_line();
    }

    /// What a copy moved; an incomplete copy is trouble, with a count, not a quiet summary.
    fn carry_copied(&mut self, summary: &pros_core::transfer::Summary, where_to: &str) {
        self.said = format!(
            "{} files, {} bytes -> {where_to}{}",
            summary.files,
            summary.bytes,
            if summary.unchanged > 0 {
                format!(" ({} unchanged, not re-sent)", summary.unchanged)
            } else {
                String::new()
            }
        );
        if !summary.is_complete() {
            self.trouble = Some(format!(
                "{} not copied - this is not a backup. First: {}",
                summary.skipped.len(),
                summary
                    .skipped
                    .first()
                    .map_or_else(String::new, |one| format!("{} ({})", one.path, one.why))
            ));
        }
    }

    /// Where the target said this section's things live, kept for the section that asked.
    fn carry_located(&mut self, found: &pros_core::locate::Where) {
        self.files.located = Some((self.section, found.clone()));
        // Not moved to an absent directory, whose empty listing would look like an
        // installed tool with nothing in it.
        if let Some(path) = found.path() {
            self.files.go_to = Some(path.to_owned());
        }
    }

    /// What is installed, for the probe screen to choose from.
    fn carry_titles(&mut self, found: Vec<pros_core::titles::Metadata>) {
        // Other screens show these names in place of identifiers too.
        for about in &found {
            if let Some(name) = &about.name {
                self.files.names.insert(about.id.clone(), name.clone());
            }
        }
        // Keep the choice when it is still installed; otherwise start on the first.
        if self
            .probing
            .id
            .as_ref()
            .is_none_or(|id| !found.iter().any(|about| &about.id == id))
        {
            self.probing.id = found.first().map(|about| about.id.clone());
        }
        self.probing.titles = Some(found);
    }

    /// Where the target said its saves are, or why it could not say.
    ///
    /// Several accounts are named as trouble rather than one being picked.
    fn carry_saves(&mut self, found: pros_core::saves::Found) {
        match found {
            pros_core::saves::Found::Here(path) => self.files.go_to = Some(path),
            pros_core::saves::Found::Several(users) => {
                self.trouble = Some(format!(
                    "several users, so this does not choose: {}",
                    users.join(", ")
                ));
            }
            pros_core::saves::Found::None => {
                self.trouble = Some(format!("no user folders under {}", pros_core::saves::HOME));
            }
        }
    }

    /// Folds a capture into the export waiting for it; discarded if the panel was closed.
    fn carry_captured(
        &mut self,
        files: Vec<pros_core::recovery::baseline::Captured>,
        notes: Vec<String>,
    ) {
        if let Some(export) = self.autoload.exporting.as_mut() {
            export.preset.files = files;
            export.notes.extend(notes);
            export.capturing = false;
        }
    }

    /// Starts whatever is waiting, unless the last one gave somebody something to read.
    ///
    /// Trouble stops the rest of the queue, with a count: [`Self::begin`] clears `trouble`, so
    /// the next job would otherwise erase the message. Pressing the button again carries on.
    fn next_in_line(&mut self) {
        if self.trouble.is_some() {
            let dropped = self.drop_queued();
            if dropped > 0 {
                let why = self.trouble.take().unwrap_or_default();
                self.trouble = Some(format!(
                    "{why}\n{dropped} more were not started - the rest of the selection is \
                     still ticked"
                ));
            }
            return;
        }
        if let Some(next) = self.queued.pop_front() {
            self.begin(next);
        }
    }

    /// Empties a panel, because what was in it is no longer known to be true.
    fn clear(&mut self, panel: Panel) {
        match panel {
            Panel::Report => self.report = None,
            Panel::Said => self.said.clear(),
            Panel::Library => self.files.library.clear(),
            Panel::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use pros_core::check::{Finding, Report};
    use pros_core::target::Target;
    use pros_link::service::{Reachability, SERVICES};

    use super::{Disturbs, Section};

    use super::{Done, Job, State};

    fn target(name: &str) -> Target {
        Target {
            name: name.to_owned(),
            address: "127.0.0.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }
    }

    fn a_report() -> Report {
        let findings = SERVICES
            .iter()
            .map(|service| Finding {
                service: service.clone(),
                reachability: Reachability {
                    open: true,
                    took: Duration::from_millis(5),
                },
            })
            .collect();
        Report::new("ps5", "127.0.0.1", findings)
    }

    /// A begun job is left pending for the worker to start.
    #[test]
    fn beginning_a_job_leaves_it_for_the_worker_to_start() {
        let mut state = State::new(vec![target("ps5")]);
        assert!(
            !state.pending_after_begin(),
            "nothing begun, nothing pending"
        );
        assert!(state.begin(Job::Check(target("ps5"))));
        assert!(
            state.pending_after_begin(),
            "a begun job was not left anywhere the window would find it"
        );
    }

    /// A second job is refused while one runs.
    #[test]
    fn only_one_job_runs_at_a_time() {
        let mut state = State::new(vec![target("ps5")]);
        assert!(state.begin(Job::Check(target("ps5"))));
        assert!(
            !state.begin(Job::Browse(target("ps5"), "/data".to_owned())),
            "a second job started while the first was still running"
        );
        state.finish(Done::Checked(Box::new(a_report()), None));
        assert!(state.is_idle());
        assert!(state.begin(Job::Browse(target("ps5"), "/data".to_owned())));
    }

    /// A failed refresh clears the previous answer and says why.
    #[test]
    fn a_failed_job_clears_what_it_would_have_replaced() {
        let mut state = State::new(vec![target("ps5")]);
        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Checked(Box::new(a_report()), None));
        assert!(state.report.is_some());

        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Failed("the target stopped answering".to_owned()));

        assert!(
            state.report.is_none(),
            "a stale report survived a failed refresh"
        );
        assert!(state.trouble.is_some(), "and nothing said why");
    }

    /// A failure clears only its own panel.
    #[test]
    fn a_failure_clears_only_its_own_panel() {
        let mut state = State::new(vec![target("ps5")]);
        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Checked(Box::new(a_report()), None));

        state.begin(Job::Browse(target("ps5"), "/data".to_owned()));
        state.finish(Done::Failed("no such directory".to_owned()));

        assert!(state.report.is_some(), "an unrelated panel was cleared");
        assert!(state.files.library.is_empty());
    }

    /// Starting a job clears the previous trouble.
    #[test]
    fn starting_a_job_clears_the_previous_trouble() {
        let mut state = State::new(vec![target("ps5")]);
        state.begin(Job::Check(target("ps5")));
        state.finish(Done::Failed("nothing answered".to_owned()));
        assert!(state.trouble.is_some());

        state.begin(Job::Check(target("ps5")));
        assert!(
            state.trouble.is_none(),
            "the previous failure is still on screen while the next attempt runs"
        );
    }

    /// An answer arriving when nothing was asked is dropped rather than displayed.
    #[test]
    fn an_answer_with_nothing_waiting_for_it_is_ignored() {
        let mut state = State::new(vec![target("ps5")]);
        state.finish(Done::Checked(Box::new(a_report()), None));
        assert!(state.report.is_none(), "an unasked-for answer was shown");
    }

    /// The waiting text names what is being waited for.
    #[test]
    fn waiting_says_which_thing_it_is_waiting_for() {
        assert_eq!(Job::Check(target("desk")).describe(), "checking desk");
        assert_eq!(
            Job::Browse(target("desk"), "/data/pldmgr".to_owned()).describe(),
            "opening /data/pldmgr"
        );
    }

    /// With nothing registered, nothing is selected.
    #[test]
    fn nothing_registered_means_nothing_chosen() {
        let state = State::new(Vec::new());
        assert!(state.target().is_none());
        assert_eq!(State::new(vec![target("only")]).chosen, Some(0));
    }

    /// A refusal is kept to show, is not trouble, and clears no panel.
    #[test]
    fn a_refusal_says_what_was_needed_without_clearing_anything() {
        let mut state = State::new(vec![Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }]);
        state.files.library = vec![item("PPSA01650")];

        state.begin(Job::Restore(
            state.target().cloned().expect("one target"),
            std::path::PathBuf::from("."),
            "/user/home/beefcafe/savedata_prospero".to_owned(),
            false,
        ));
        state.finish(Done::Refused(pros_core::origin::Needs::Resigning {
            wrote: "769f77716958d37e".to_owned(),
            going_to: "00112233445566aa".to_owned(),
        }));

        assert!(
            state.files.refused.is_some(),
            "the reason should be kept to show"
        );
        assert!(
            state.trouble.is_none(),
            "a refusal is not trouble - nothing went wrong"
        );
        assert_eq!(
            state.files.library.len(),
            1,
            "the listing should survive a copy that was declined"
        );
        assert!(state.is_idle(), "and the job is over");
    }

    /// One listing entry, named.
    fn item(name: &str) -> pros_core::library::Item {
        pros_core::library::Item {
            name: name.to_owned(),
            id: None,
            kind: pros_core::library::Kind::Folder,
            size: None,
        }
    }

    /// A locate answer belongs to the section that asked, not the one on screen.
    #[test]
    fn a_locate_answer_belongs_to_the_section_that_asked() {
        let mut state = State::new(vec![Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }]);
        state.section = Section::Cheats;
        state.begin(Job::Locate(
            state.target().cloned().expect("one target"),
            Section::Cheats.candidates(),
        ));
        state.finish(Done::Located(pros_core::locate::Where::NoneOfThem(vec![
            "/data/cheatrunner/cheats".to_owned(),
        ])));

        let (asked, _) = state.files.located.as_ref().expect("an answer was kept");
        assert_eq!(*asked, Section::Cheats, "it should remember who asked");

        state.section = Section::Titles;
        let still_ours = state
            .files
            .located
            .as_ref()
            .is_some_and(|(asked, _)| *asked == state.section);
        assert!(
            !still_ours,
            "the cheats answer should not read as an answer about titles"
        );
    }

    /// Sending a payload marks the check report stale.
    #[test]
    fn running_a_payload_makes_what_the_target_can_do_stale() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        let job = Job::Send(
            target,
            "klogsrv".to_owned(),
            std::path::PathBuf::from("klogsrv.elf"),
        );
        assert_eq!(job.disturbs(), [Disturbs::Report]);
    }

    /// Read-only jobs disturb nothing, so a check does not trigger itself.
    #[test]
    fn asking_what_is_true_does_not_make_anything_untrue() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        assert!(Job::Check(target.clone()).disturbs().is_empty());
        assert!(Job::ReadSystem(target.clone()).disturbs().is_empty());
        assert!(
            Job::Browse(target, "/data".to_owned())
                .disturbs()
                .is_empty()
        );
    }

    /// What a job disturbs is recorded even when it failed.
    #[test]
    fn a_job_that_failed_still_leaves_the_world_changed() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        let mut state = State::new(vec![target.clone()]);
        state.begin(Job::Push(
            target,
            std::path::PathBuf::from("a.elf"),
            "/data/a.elf".to_owned(),
        ));
        state.finish(Done::Failed("refused".to_owned()));

        assert_eq!(
            state.disturbed,
            [Disturbs::There],
            "a failed copy may still have written something"
        );
    }

    /// A shell command is assumed to have changed the target.
    #[test]
    fn an_arbitrary_command_is_assumed_to_have_changed_things() {
        let target = Target {
            name: "ps5".to_owned(),
            address: "192.0.2.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        };
        let job = Job::Shell(target, "rm /data/thing".to_owned());
        assert!(job.disturbs().contains(&Disturbs::Report));
        assert!(job.disturbs().contains(&Disturbs::There));
    }
}

#[cfg(test)]
mod queue_tests {
    use std::path::PathBuf;

    use pros_core::target::Target;

    use super::{Done, Job, State};

    fn target() -> Target {
        Target {
            name: "ps5".to_owned(),
            address: "127.0.0.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }
    }

    fn state() -> State {
        State::new(vec![target()])
    }

    fn push(which: u8) -> Job {
        Job::Push(
            target(),
            PathBuf::from(format!("{which}.pkg")),
            format!("/data/{which}.pkg"),
        )
    }

    /// Queuing a selection starts the first and keeps the rest waiting.
    #[test]
    fn a_selection_of_four_asks_for_four() {
        let mut state = state();
        for which in 0..4 {
            state.queue(push(which));
        }
        assert!(!state.is_idle(), "the first one runs");
        assert_eq!(state.queued(), 3, "the rest are waiting, not gone");
    }

    /// Each finish starts the next, until the queue is empty.
    #[test]
    fn finishing_one_starts_the_next() {
        let mut state = state();
        for which in 0..3 {
            state.queue(push(which));
        }
        for left in [1, 0] {
            state.finish(Done::Said("ok".to_owned()));
            assert_eq!(state.queued(), left);
            assert!(!state.is_idle(), "{left} left, so something is running");
        }
        state.finish(Done::Said("ok".to_owned()));
        assert!(state.is_idle(), "the line is empty and nothing is running");
    }

    /// A failure stops the queue and says how many were not started.
    #[test]
    fn a_failure_stops_the_rest_and_says_so() {
        let mut state = state();
        for which in 0..4 {
            state.queue(push(which));
        }
        state.finish(Done::Failed("the target refused STOR".to_owned()));

        assert!(state.is_idle(), "nothing carried on past the failure");
        assert_eq!(state.queued(), 0, "the line was dropped, not left dangling");
        let trouble = state.trouble.expect("a failure leaves something to read");
        assert!(
            trouble.contains("the target refused STOR"),
            "the reason survives: {trouble}"
        );
        assert!(
            trouble.contains('3'),
            "and says how many did not start: {trouble}"
        );
    }

    /// A copy that finished with files missing also stops the queue.
    #[test]
    fn an_incomplete_copy_also_stops_the_rest() {
        let mut state = state();
        for which in 0..3 {
            state.queue(push(which));
        }
        let mut summary = pros_core::transfer::Summary::default();
        summary.skipped.push(pros_core::transfer::Skipped {
            path: "one.pkg".to_owned(),
            why: "refused".to_owned(),
        });
        state.finish(Done::Copied(Box::new(summary), "/data".to_owned()));

        assert!(state.is_idle());
        assert_eq!(state.queued(), 0);
        assert!(state.trouble.is_some_and(|why| why.contains('2')));
    }

    /// Clearing the queue leaves the running job alone.
    #[test]
    fn clearing_the_queue_does_not_touch_what_is_running() {
        let mut state = state();
        for which in 0..3 {
            state.queue(push(which));
        }
        assert_eq!(state.drop_queued(), 2);
        assert!(!state.is_idle(), "the running one is untouched");
        assert_eq!(state.queued(), 0);
    }
}

#[cfg(test)]
mod retention_tests {
    use pros_core::library::{Item, Kind};
    use pros_core::target::Target;

    use super::{Done, Job, State};

    fn target() -> Target {
        Target {
            name: "ps5".to_owned(),
            address: "127.0.0.1".to_owned(),
            ports: std::collections::BTreeMap::new(),
            chain: None,
        }
    }

    fn listing() -> Vec<Item> {
        vec![Item {
            name: "thing.elf".to_owned(),
            kind: Kind::File,
            size: Some(1),
            id: None,
        }]
    }

    /// A listing is kept by its path.
    #[test]
    fn a_listing_is_remembered_by_its_path() {
        let mut state = State::new(vec![target()]);
        state.begin(Job::Browse(target(), "/data/pkg".to_owned()));
        state.finish(Done::Browsed(listing()));
        assert_eq!(state.files.seen.get("/data/pkg").map(Vec::len), Some(1));
    }

    /// A push announces that it disturbed the target, the signal that clears kept listings.
    #[test]
    fn what_changed_the_target_is_not_remembered_from_before() {
        let mut state = State::new(vec![target()]);
        state.begin(Job::Browse(target(), "/data/pkg".to_owned()));
        state.finish(Done::Browsed(listing()));
        assert!(!state.files.seen.is_empty());

        state.begin(Job::Push(
            target(),
            std::path::PathBuf::from("x"),
            "/data/pkg/x".to_owned(),
        ));
        assert!(
            state
                .pending
                .as_ref()
                .is_some_and(|job| job.disturbs().contains(&super::Disturbs::There)),
            "a push must announce that it changed the target"
        );
    }
}
