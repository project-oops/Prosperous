//! Health checks that say what is wrong, and exactly what would put it right.
//!
//! # The one rule this module is built around
//!
//! **A check can never change anything.** It reads a snapshot and returns a
//! [`crate::doctor::Plan`], and a plan is inert data - a list of things somebody could do, in
//! order, with no way at all to do them.
//! Nothing here holds a connection, a path on this machine, or a target. Carrying a plan out
//! is the window's job, after somebody has read it and said yes.
//!
//! That is not politeness, it is the whole safety property: this program configures a machine
//! whose recovery costs a walk across the room with a USB stick, and it has already cost that
//! twice. So the boundary is drawn in the type system rather than in a habit - a check that
//! wanted to act would have nothing to act *with*.
//!
//! # Why a failure cannot exist without a remedy
//!
//! [`crate::doctor::Verdict::Unwell`] carries its [`crate::doctor::Remedy`] rather than having
//! one alongside. A finding with no remedy is a sentence telling somebody their target is
//! broken and then leaving - which is what this program used to do, in two places, with two
//! unrelated notions of a fix:
//!
//! - the check screen could download a payload, send it, or run it, one press per stage, and
//!   none of the three put it in a startup list, so none of them survived a restart;
//! - the startup-list screen could add an entry, but only if the file was **already** on
//!   internal storage - and otherwise printed *copy it there first* and stopped.
//!
//! Between those two sat the actual answer - fetch it, send it, list it - which nothing could
//! express, so it was left to a person to assemble across two screens.
//! [`crate::doctor::Remedy::Beyond`] exists for the cases where there genuinely is nothing to
//! do, and it has to say why.
//!
//! # Why the checks do no probing
//!
//! Everything here is a pure function of [`crate::doctor::Known`], which the window has
//! already gathered when the target was selected. A check that opened its own socket would
//! double the traffic to a
//! jailbroken console, would be untestable without one, and would be able to disagree with the
//! panel drawn beside it about what is running. Re-checking is re-gathering, which is one
//! mechanism instead of two.

use crate::catalogue::Catalogue;
use crate::chain::Chain;
use crate::check::Report;
use crate::manifest::Manifest;
use crate::payloads::{INTERNAL, There, Where};
use crate::recovery::{Fix, Gravity, Hazard, Kind, audit, baseline};

/// One thing somebody could do, named but not done.
///
/// **No paths on this machine and no target.** A step names a payload; binding that name to a
/// file is the window's job, and keeping it out of here is what stops a plan being executable
/// by whoever happens to be holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Get it onto this machine from where its description says it lives.
    Fetch {
        /// Which payload, as the manifest names it.
        payload: String,
    },
    /// Copy it off the target, from a place a startup list cannot rely on.
    ///
    /// The first half of moving a payload from a stick to internal storage. It goes via this
    /// machine because that is a copy this program has measured; a copy on the target itself
    /// would be a shell command whose failure looks like its success.
    Bring {
        /// Which payload.
        payload: String,
        /// Where it is now, in full.
        from: String,
    },
    /// Put it on the target, where a startup list can resolve it.
    Send {
        /// Which payload.
        payload: String,
        /// The directory it lands in.
        to: &'static str,
    },
    /// Change one line of the startup list.
    ///
    /// **Several of these collapse into one write.** The list is one file, and a plan that
    /// wrote it three times would give a person three chances to be interrupted half way.
    List(Fix),
    /// Replace a startup list entirely, with this.
    ///
    /// **Not several [`crate::doctor::Step::List`] edits.** Setting up a target from nothing
    /// is not a sequence of adds against whatever happened to be there - it is one file, and
    /// describing it as edits would show somebody a diff against a configuration they are
    /// throwing away, which is the least useful way to look at it.
    Rebuild {
        /// Which list, in full.
        into: &'static str,
        /// Every entry, in the order they will run.
        entries: Vec<String>,
    },
    /// Load something the target already has, now, without moving anything.
    ///
    /// Does not survive a restart, and says so - it is the answer to *this is not running*,
    /// never to *this is not in the list*.
    Run {
        /// The full path on the target.
        path: String,
    },
}

impl Step {
    /// How to put it to somebody, in the order they would do it.
    #[must_use]
    pub fn describe(&self) -> String {
        match self {
            Self::Fetch { payload } => format!("download {payload} to this machine"),
            Self::Bring { payload, from } => format!("copy {payload} off the target, from {from}"),
            Self::Send { payload, to } => format!("send {payload} to {to}"),
            Self::List(Fix::Add(name)) => format!("add {name} to the startup list"),
            Self::List(Fix::Remove(name)) => format!("take {name} out of the startup list"),
            Self::Rebuild { into, entries } => {
                format!("replace {into} with these {} entries", entries.len())
            }
            Self::Run { path } => format!("load {path} now, without sending anything"),
        }
    }

    /// Whether carrying this out changes the target.
    ///
    /// **Fetching does not.** A plan whose only unsatisfied step is a download can be run
    /// without touching the console at all, and saying so is the difference between somebody
    /// pressing the button and somebody putting it off.
    #[must_use]
    pub const fn touches_the_target(&self) -> bool {
        match self {
            Self::Fetch { .. } => false,
            Self::Bring { .. }
            | Self::Send { .. }
            | Self::List(_)
            | Self::Rebuild { .. }
            | Self::Run { .. } => true,
        }
    }

    /// Whether this is an edit to the startup list.
    #[must_use]
    pub const fn is_a_list_edit(&self) -> bool {
        matches!(self, Self::List(_) | Self::Rebuild { .. })
    }
}

/// A step, and whether it has already been done.
///
/// **Satisfied steps stay in the plan.** Hiding them would show somebody two steps where the
/// work is four, and the shape of the whole job is what makes it obvious that *download* and
/// *add to the list* are one action rather than two unrelated buttons on two screens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move {
    /// What to do.
    pub step: Step,
    /// Whether it is already the case, and so will be skipped.
    pub already: bool,
}

/// An ordered set of steps that answers one finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// What this achieves, in one sentence, for the confirmation.
    pub because: String,
    /// The steps, in the order they must happen.
    pub moves: Vec<Move>,
}

impl Plan {
    /// The steps that are not already done.
    #[must_use]
    pub fn outstanding(&self) -> Vec<&Step> {
        self.moves
            .iter()
            .filter(|one| !one.already)
            .map(|one| &one.step)
            .collect()
    }

    /// Whether there is anything left to do.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.moves.iter().all(|one| one.already)
    }

    /// Whether carrying this out would change the target at all.
    #[must_use]
    pub fn touches_the_target(&self) -> bool {
        self.moves
            .iter()
            .any(|one| !one.already && one.step.touches_the_target())
    }

    /// Whether the startup list would be rewritten.
    ///
    /// The one consequence worth naming separately in a confirmation: everything else here can
    /// be undone by doing it again, and this is what decides whether the target comes back.
    #[must_use]
    pub fn rewrites_the_list(&self) -> bool {
        self.moves
            .iter()
            .any(|one| !one.already && one.step.is_a_list_edit())
    }
}

impl Plan {
    /// One plan out of several, in order, with a step that appears twice done once.
    ///
    /// # Why duplicates have to go
    ///
    /// Two findings about the same payload propose overlapping work - *it is not answering*
    /// wants it fetched and run, *it is not in the startup list* wants it fetched and listed.
    /// Concatenating them would fetch it twice and send it twice, which is a plan that spends
    /// somebody's time doing something it already did, in front of them, having just shown
    /// them the list.
    #[must_use]
    pub fn all_of(plans: &[Self]) -> Self {
        let mut moves: Vec<Move> = Vec::new();
        for plan in plans {
            for one in &plan.moves {
                // The first mention wins, which keeps the earliest position a step needed.
                if !moves.iter().any(|kept| kept.step == one.step) {
                    moves.push(one.clone());
                }
            }
        }
        Self {
            because: format!(
                "{} findings, answered together - {} steps once anything repeated is done once",
                plans.len(),
                moves.iter().filter(|one| !one.already).count()
            ),
            moves,
        }
    }
}

/// What could be done about a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remedy {
    /// Every step is known, and nothing further needs asking.
    ///
    /// **Known, not permitted.** This is the *"no questions"* case, never the *"no consent"*
    /// case: a settled plan still goes in front of somebody before any of it happens.
    Ready(Plan),
    /// More than one thing would answer this, and choosing is not this program's to make.
    Choose {
        /// The options: what each is called, and what it buys.
        between: Vec<(String, String)>,
        /// Why this is being asked rather than decided.
        why: String,
    },
    /// Nothing here can put it right, and this is why.
    Beyond(String),
}

/// What one check concluded.
///
/// # Four states, not two
///
/// The tools this borrows its shape from report pass or fail. That is one state short in both
/// directions for a program that talks to a machine over a network it does not control:
/// *nothing was measured* is not *it is broken*, and *this does not apply here* is not *this
/// is fine*. Collapsing either one produces the failure this whole project is organised
/// against - a report that reads identically whether or not it found anything out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It is as it should be.
    Well(String),
    /// It is not, and this is what would put it right.
    Unwell {
        /// What is wrong, in a person's words.
        why: String,
        /// What to do about it.
        remedy: Remedy,
    },
    /// Nothing was measured, so nothing is claimed.
    Unknown(String),
    /// It does not apply to this target, so it is not being asked.
    Aside(String),
}

impl Verdict {
    /// What to show beside the finding.
    #[must_use]
    pub fn describe(&self) -> &str {
        match self {
            Self::Well(said) | Self::Unknown(said) | Self::Aside(said) => said,
            Self::Unwell { why, .. } => why,
        }
    }

    /// Whether this is a failure.
    #[must_use]
    pub const fn is_unwell(&self) -> bool {
        matches!(self, Self::Unwell { .. })
    }
}

/// One check, and what it found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// A stable name for this check, unique among findings.
    ///
    /// **Stable across runs**, because it is how a fix in flight is matched to the finding it
    /// was meant to answer once the target has been asked again.
    pub id: String,
    /// What the check is, for somebody reading a list of them.
    pub label: String,
    /// How much a failure of this one matters.
    pub gravity: Gravity,
    /// What it found.
    pub verdict: Verdict,
}

/// The traffic light, worst wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    /// Every check that applies passed.
    Well,
    /// Something was not measured. **Above `Well` on purpose**: a check nobody could run is
    /// not a check that passed, and showing the two the same colour is how an unreachable
    /// target comes to look like a healthy one.
    Unknown,
    /// Something failed that costs visibility rather than access.
    Warning,
    /// Something failed that can leave the target unreachable.
    Unwell,
}

/// The worst of what was found.
///
/// Nothing at all is [`Health::Unknown`] rather than [`Health::Well`], for the same reason: an
/// empty list of findings is a check that has not happened.
#[must_use]
pub fn health(findings: &[Finding]) -> Health {
    let mut worst = Health::Well;
    let mut any = false;
    for finding in findings {
        let one = match (&finding.verdict, finding.gravity) {
            (Verdict::Aside(_), _) => continue,
            (Verdict::Well(_), _) => Health::Well,
            (Verdict::Unknown(_), _) => Health::Unknown,
            (Verdict::Unwell { .. }, Gravity::Warning) => Health::Warning,
            (Verdict::Unwell { .. }, Gravity::Critical) => Health::Unwell,
        };
        any = true;
        worst = worst.max(one);
    }
    // **Nothing to report is not a clean bill of health.** Starting at `Well` and taking the
    // worst is right once anything has been looked at; with an empty list it would claim a
    // target is fine on the strength of never having asked it anything.
    if any { worst } else { Health::Unknown }
}

/// Everything the checks are allowed to look at.
///
/// A borrowed snapshot of what the window already asked for. **Nothing here can be used to ask
/// anything else**, which is what makes a check a function rather than an actor.
#[derive(Debug, Clone, Copy)]
pub struct Known<'a> {
    /// What answered when the target was last probed.
    pub report: Option<&'a Report>,
    /// Every payload file found on the target, with where it lives.
    ///
    /// **`None` is not an empty target.** A target nobody has listed and a target with no
    /// payloads on it are the same slice and opposite facts, and the second one licenses a
    /// plan that begins *download it* while the file may be sitting there already. So the
    /// difference is kept, and a route that cannot be worked out without it says so.
    pub there: Option<&'a [There]>,
    /// Payloads already on this machine, by the name their description gives.
    pub staged: &'a [String],
    /// What this program knows about payloads it has never seen.
    pub described: &'a Manifest,
    /// The startup list being examined.
    pub chain: Option<&'a Chain>,
    /// Which kind of list that is - the rules invert between them.
    pub kind: Kind,
    /// The chain this target is meant to be running.
    ///
    /// **What somebody decided, not what this program knows about.** It decides which absences
    /// are worth reporting: a console brought up by etaHEN is not missing an FTP server,
    /// because etaHEN is one.
    pub preset: &'a baseline::Preset,
    /// What each service is and what it unlocks.
    pub known: &'a Catalogue,
}

impl Known<'_> {
    /// Whether a payload with this name is already on this machine.
    fn is_here(&self, service: &str) -> bool {
        self.staged.iter().any(|name| named_as(name, service))
    }

    /// Where a payload with this name is on the target, preferring somewhere usable.
    ///
    /// **Internal storage wins.** A copy on a stick and a copy on the drive are not
    /// interchangeable - a startup list can only resolve the second - so a search that
    /// returned whichever came first would report a payload as present and then build a list
    /// that fails at every boot.
    fn on_target(&self, service: &str) -> OnTarget<'_> {
        let Some(there) = self.there else {
            return OnTarget::Unknown;
        };
        let mut fallback = None;
        for one in there {
            if !named_as(&one.name, service) {
                continue;
            }
            if one.storage == Where::Internal {
                return OnTarget::At(one);
            }
            fallback.get_or_insert(one);
        }
        fallback.map_or(OnTarget::Absent, OnTarget::At)
    }

    /// Whether this payload was already loaded, as far as anything here can tell.
    ///
    /// # Why a silent port is not an answer
    ///
    /// **It cost a console an hour to learn this twice.** A payload that a startup list loaded
    /// is running; a port that does not answer says only that *this program cannot reach it*.
    /// The two were treated as one, and *load it now* was offered for something already up -
    /// which starts a second copy, and a second copy crashed the machine.
    ///
    /// So this asks the only question with a real answer: **did something already load it?**
    /// Being in a startup list that has run is evidence. So is being the thing that runs the
    /// list at all - if the list ran, its runner is up, whatever 8084 says about it.
    fn was_already_loaded(&self, service: &str) -> bool {
        if self
            .known
            .services()
            .iter()
            .any(|one| one.runs_lists && named_as(one.name.as_ref(), service))
        {
            // The list this is auditing came off the target, which means something read it,
            // which means the thing that reads lists is running.
            return self.chain.is_some();
        }
        self.chain
            .is_some_and(|chain| chain.position(service).is_some())
    }

    /// Whether the loader is answering.
    ///
    /// **Everything that runs a payload goes through it.** Sending a file does not - that is
    /// the file server - but starting one does, whether it is sent to 9021 or already on the
    /// disk and started with `hbldr`: both end at `elfldr_spawn`. So this decides whether
    /// *run it* is an offer or a fiction.
    fn loader_is_up(&self) -> Option<bool> {
        let report = self.report?;
        let loader = report.about(pros_link::service::LOADER.name.as_ref())?;
        Some(loader.reachability.open)
    }

    /// Whether this program knows where to get a payload it has never seen.
    fn can_fetch(&self, service: &str) -> bool {
        self.described
            .payloads()
            .iter()
            .any(|one| named_as(&one.name, service) && one.url.is_some())
    }
}

/// Whether the target holds a payload, in three answers rather than two.
///
/// **The third is the one that matters.** *Not listed yet* and *not there* differ by exactly
/// the fact that would make a plan wrong: one of them licenses *download it first*, and the
/// other is a target whose copy nobody has looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnTarget<'a> {
    /// Nobody has listed the target's payloads.
    Unknown,
    /// They were listed, and it is not among them.
    Absent,
    /// It is there, at this file.
    At(&'a There),
}

/// How a payload could be got onto internal storage.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    /// These steps, in this order.
    Steps(Vec<Move>),
    /// It cannot be worked out yet, because nobody has looked.
    NotYet,
    /// There is no route: not on the target, not here, and nothing says where to get it.
    Nowhere,
}

/// Whether a file or entry is this service, by the same rule as everything else.
///
/// One rule, in one place: a second way of matching a name here could disagree with the one
/// the startup list uses, and then a plan would add an entry the audit does not recognise.
fn named_as(candidate: &str, service: &str) -> bool {
    Chain::parse(candidate).position(service).is_some()
}

/// What to say when the route cannot be worked out because nobody has looked.
fn not_looked_yet(service: &str) -> String {
    format!(
        "the target's payloads have not been listed, so whether {service} is already there is \
         unknown - checking again answers it"
    )
}

/// The steps that get a payload onto internal storage, cheapest and surest first.
///
/// # The order, and why it is this order
///
/// It used to ask *where is it on the target* first, and so a copy sitting on a USB stick beat
/// both the copy already on this machine and the one the description says where to download.
/// The advice that produced was **"copy pldmgr off the target, from
/// `/mnt/usb0/ps5_autoloader/pldmgr_v0.5.1.elf`"** - two network trips to drag a file off the
/// console and hand it straight back, when the file was on this disk and a verified download
/// was a click away. It read as nonsense because it was.
///
/// So:
///
/// 1. **Already where a startup list can resolve it** - nothing to do, and it says so.
/// 2. **On this machine** - one copy, no download, and it is the build this list describes.
/// 3. **Described with an address** - download it, checked against the digest the list states.
///    A verified download of the described build beats an unverified copy of some other build.
/// 4. **Somewhere on the target a list cannot use** - drag it over and hand it back. Last,
///    because it is the slowest, the only one that produces no digest to check, and the one
///    that leans on a USB stick that is somebody's way back in when everything else is broken.
fn get_it_there(what: &Known<'_>, service: &str) -> Route {
    let send = || Move {
        step: Step::Send {
            payload: service.to_owned(),
            to: INTERNAL,
        },
        already: false,
    };
    let there = what.on_target(service);

    // **Not knowing is its own answer.** Falling through here would propose moving a file
    // about on the strength of never having looked for the copy that may already be in place.
    if there == OnTarget::Unknown {
        return Route::NotYet;
    }
    // 1. Already where a startup list can resolve it: named, marked done, shown anyway.
    if let OnTarget::At(one) = there
        && one.storage == Where::Internal
    {
        return Route::Steps(vec![Move {
            already: true,
            ..send()
        }]);
    }
    // 2. On this machine. The download is listed and marked done, so the shape of the job is
    //    the same one somebody sees when it is not.
    if what.is_here(service) {
        return Route::Steps(vec![
            Move {
                step: Step::Fetch {
                    payload: service.to_owned(),
                },
                already: true,
            },
            send(),
        ]);
    }
    // 3. Described, so it can be fetched and checked against the digest the list states.
    if what.can_fetch(service) {
        return Route::Steps(vec![
            Move {
                step: Step::Fetch {
                    payload: service.to_owned(),
                },
                already: false,
            },
            send(),
        ]);
    }
    // 4. Last: the copy on the target that a startup list cannot resolve. Two network trips
    //    and no digest at the end of them, which is why nothing above it settles for this.
    if let OnTarget::At(one) = there {
        return Route::Steps(vec![
            Move {
                step: Step::Bring {
                    payload: service.to_owned(),
                    from: one.path.clone(),
                },
                already: false,
            },
            send(),
        ]);
    }
    Route::Nowhere
}

/// The plan that puts a service into the startup list, getting it there first if it is not.
fn put_it_in_the_list(what: &Known<'_>, service: &str, because: String) -> Remedy {
    let mut moves = match get_it_there(what, service) {
        Route::Steps(moves) => moves,
        Route::NotYet => return Remedy::Beyond(not_looked_yet(service)),
        Route::Nowhere => {
            return Remedy::Beyond(format!(
                "{service} is not on the target, not on this machine, and nothing describes \
                 where to get it - add it to the payload list first"
            ));
        }
    };
    moves.push(Move {
        step: Step::List(Fix::Add(service.to_owned())),
        already: false,
    });
    Remedy::Ready(Plan { because, moves })
}

/// The plan that puts one named service into the startup list.
///
/// **Public because [`Remedy::Choose`] hands a decision to a person**, and whatever they pick
/// has to become a plan somewhere. Building it here rather than in the window is what stops
/// the chosen route differing from the one the check would have proposed itself.
#[must_use]
pub fn plan_for(what: &Known<'_>, service: &str) -> Remedy {
    put_it_in_the_list(
        what,
        service,
        format!("so {service} is running after a restart, and there is a way back in"),
    )
}

/// What a startup list should call this payload, so the loader can find it.
///
/// # Why a chain's name is not a filename
///
/// A chain names payloads - `kstuff-lite`, `nanoDNS` - because an ordering is about what a
/// thing *is*, and versions change without the ordering changing. A startup list names
/// **files**, because something has to resolve them on disk.
///
/// Deploying a chain wrote the first where the second was needed. The list it produced was
/// well-formed, correctly ordered, and named six things that do not exist - every row reporting
/// *not on the target*, which is exactly what it was: a configuration that cannot load
/// anything, written by the feature whose whole job is producing one that can.
///
/// So: what is on the target already, or failing that what the list describes and a send is
/// about to put there. The bare name only when nothing knows any better, which is the case
/// where the payload has no route and is being left out anyway.
fn will_be_called(what: &Known<'_>, service: &str) -> Option<String> {
    // Already where a list can resolve it: it is not being replaced, so it keeps its name.
    if let OnTarget::At(one) = what.on_target(service)
        && one.storage == Where::Internal
    {
        return Some(one.name.clone());
    }
    // Otherwise a send is in the plan, and it writes the described filename.
    //
    // **`None` rather than the bare name.** A description with no filename cannot say what
    // will be on the disk, and writing the chain's own word for it produces exactly the line
    // this function exists to stop producing - one that resolves to nothing. A payload that
    // cannot be named is left out and said, like one that cannot be got.
    what.described
        .payloads()
        .iter()
        .find(|one| named_as(&one.name, service))
        .and_then(|one| one.filename.clone())
}

/// A plan that sets a target up from nothing, in the recommended order.
///
/// # What this is for
///
/// A target that has just been exploited, or a stick being made into a way back in. Both are
/// the same job - get the payloads somewhere the loader can reach and write a list that runs
/// them in an order that works - and both are otherwise a dozen presses across three screens
/// with the ordering held in somebody's head.
///
/// # Where the order comes from
///
/// [`crate::recovery::baseline`], which is a tracked file in this repository rather than a
/// constant, so the order and the reason for it are reviewable like anything else here. This
/// adds nothing to it and invents no order of its own.
///
/// # What it will not do
///
/// **It plans; it does not write.** Like every other plan here it is inert until somebody
/// agrees to it, and the list it produces then goes through the same whole-file review as any
/// other write. Two confirmations for the most destructive thing this program can do.
///
/// Payloads with no route are left out and named, rather than listed and unresolvable: an
/// entry naming a file the loader cannot find fails at every boot with only a log line to say
/// so, which is the exact failure this whole module exists to prevent.
#[must_use]
pub fn provision(
    what: &Known<'_>,
    into: &'static str,
    kind: Kind,
    preset: &baseline::Preset,
) -> (Plan, Vec<String>) {
    let mut moves: Vec<Move> = Vec::new();
    let mut entries: Vec<String> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();

    for placed in preset.in_order(kind) {
        // The loader is required in an autoloader's list and impossible in the manager's own.
        // The audit knows this; taking the same decision twice in two places is how the two
        // come to disagree.
        if !crate::recovery::can_work_in(&placed.name, kind, what.known, what.loader_is_up()) {
            continue;
        }
        match get_it_there(what, &placed.name) {
            Route::Steps(steps) => {
                for one in steps {
                    if !moves.iter().any(|kept| kept.step == one.step) {
                        moves.push(one);
                    }
                }
                match will_be_called(what, &placed.name) {
                    Some(file) => entries.push(file),
                    None => left_out.push(format!(
                        "{} - nothing says what file it arrives as, so a list entry for it                          would resolve to nothing",
                        placed.name
                    )),
                }
            }
            Route::NotYet => left_out.push(format!(
                "{} - the target's payloads have not been listed yet",
                placed.name
            )),
            Route::Nowhere => left_out.push(format!(
                "{} - not on the target, not here, and nothing says where to get it",
                placed.name
            )),
        }
    }

    moves.push(Move {
        step: Step::Rebuild {
            into,
            entries: entries.clone(),
        },
        already: false,
    });
    (
        Plan {
            because: format!(
                "a working chain from nothing: {} payloads in the recommended order, and {into} \
                 replaced with it",
                entries.len()
            ),
            moves,
        },
        left_out,
    )
}

/// The checks about the startup list alone, worst first.
///
/// **For the screen that edits it.** Everything [`crate::doctor::examine`] reports is worth
/// knowing, but half of it is about what is answering right now, which is a different question
/// with a different screen. A list being edited should be told what is wrong with *it*, where
/// it is being edited, rather than only on another screen that audits a different list.
#[must_use]
pub fn examine_list(what: &Known<'_>) -> Vec<Finding> {
    let mut findings = about_the_list(what);
    findings.sort_by(|left, right| {
        rank(&right.verdict, right.gravity)
            .cmp(&rank(&left.verdict, left.gravity))
            .then_with(|| left.id.cmp(&right.id))
    });
    findings
}

/// Every check, run against one snapshot.
///
/// Ordered worst first, which is the order somebody should read them in and therefore the
/// order they are drawn in.
#[must_use]
pub fn examine(what: &Known<'_>) -> Vec<Finding> {
    let mut findings = Vec::new();
    findings.extend(about_the_list(what));
    findings.extend(about_what_is_running(what));
    findings.sort_by(|left, right| {
        rank(&right.verdict, right.gravity)
            .cmp(&rank(&left.verdict, left.gravity))
            .then_with(|| left.id.cmp(&right.id))
    });
    findings
}

/// How far up the list a finding belongs.
const fn rank(verdict: &Verdict, gravity: Gravity) -> u8 {
    match (verdict, gravity) {
        (Verdict::Unwell { .. }, Gravity::Critical) => 4,
        (Verdict::Unwell { .. }, Gravity::Warning) => 3,
        (Verdict::Unknown(_), _) => 2,
        (Verdict::Well(_), _) => 1,
        (Verdict::Aside(_), _) => 0,
    }
}

/// What the startup list would leave standing, as findings.
fn about_the_list(what: &Known<'_>) -> Vec<Finding> {
    let Some(chain) = what.chain else {
        return vec![Finding {
            id: "startup-list".to_owned(),
            label: "the startup list".to_owned(),
            gravity: Gravity::Critical,
            verdict: Verdict::Unknown(
                "not read, so what a restart brings back is unknown".to_owned(),
            ),
        }];
    };

    // An unlisted target passes nothing here rather than a guess: the audit then makes no
    // claim about where a file is, which under-reports instead of asserting something unmeasured.
    let hazards = audit(
        chain,
        what.known,
        what.there.unwrap_or_default(),
        what.kind,
        what.preset,
        what.loader_is_up(),
    );
    let mut findings: Vec<Finding> = hazards.iter().map(|one| from_hazard(what, one)).collect();
    if findings.is_empty() {
        findings.push(Finding {
            id: "startup-list".to_owned(),
            label: "the startup list".to_owned(),
            gravity: Gravity::Critical,
            // **Says which list, because it is not the only one.** *This list brings back a way
            // in* read as a verdict on the target while being a verdict on one file, and the
            // file that decides whether the loader comes back is usually a different one.
            verdict: Verdict::Well(format!(
                "after a restart, {} brings back a way in",
                what.kind.describe()
            )),
        });
    }
    findings
}

/// One hazard, as a finding with everything it would take to answer it.
fn from_hazard(what: &Known<'_>, hazard: &Hazard) -> Finding {
    let gravity = hazard.gravity();
    let why = hazard.describe();
    let (id, label, remedy) = match hazard {
        Hazard::ReloadsTheLoader { loader, .. } => (
            "loader-in-list".to_owned(),
            "the loader is not in its own list".to_owned(),
            Remedy::Ready(Plan {
                because: format!(
                    "{loader} cannot be loaded through itself, and everything listed after it \
                     is what pays"
                ),
                moves: vec![Move {
                    step: Step::List(Fix::Remove(loader.clone())),
                    already: false,
                }],
            }),
        ),
        Hazard::ChainNeverRuns { runner } => (
            "runs-lists".to_owned(),
            "something starts the payload manager".to_owned(),
            put_it_in_the_list(
                what,
                runner,
                format!(
                    "without {runner} in this list it never starts, and its own list never runs"
                ),
            ),
        ),
        Hazard::Missing {
            service, unlocks, ..
        } => (
            format!("in-list:{service}"),
            format!("{service} comes back after a restart"),
            put_it_in_the_list(
                what,
                service,
                format!("so {unlocks} is there after a restart, not only until the next one"),
            ),
        ),
        Hazard::OnRemovable { entry, storage } => (
            format!("resolvable:{entry}"),
            format!("{entry} is somewhere the list can reach"),
            on_removable(what, entry, *storage),
        ),
        Hazard::NoWayBack { candidates } => (
            "way-back".to_owned(),
            "something can still accept a payload".to_owned(),
            Remedy::Choose {
                between: candidates.clone(),
                why: "any one of these is a way back in, and which one is a choice about how \
                      this target boots"
                    .to_owned(),
            },
        ),
    };
    Finding {
        id,
        label,
        gravity,
        verdict: Verdict::Unwell { why, remedy },
    }
}

/// What to do about an entry whose file is not where a list can resolve it.
fn on_removable(what: &Known<'_>, entry: &str, storage: Where) -> Remedy {
    if storage.can_autoload() {
        // A stick's own manager folder resolves while that stick is in. Somebody may mean it.
        return Remedy::Beyond(format!(
            "{entry} resolves only while that storage is attached - deliberate, if the stick \
             stays in. Copying it to {INTERNAL} is what makes it unconditional."
        ));
    }
    let mut moves = match get_it_there(what, entry) {
        Route::Steps(moves) => moves,
        Route::NotYet => return Remedy::Beyond(not_looked_yet(entry)),
        Route::Nowhere => {
            return Remedy::Beyond(format!(
                "{entry} is listed but its file cannot be found anywhere this program can reach"
            ));
        }
    };
    // The entry is already in the list; moving the file is the whole of the answer, so no
    // list edit is added. An entry that reads the same before and after is the point.
    moves.retain(|one| !one.step.is_a_list_edit());
    Remedy::Ready(Plan {
        because: format!("so the manager can resolve {entry} at every boot, not just this one"),
        moves,
    })
}

/// What is answering now, as findings - a different question from what comes back.
fn about_what_is_running(what: &Known<'_>) -> Vec<Finding> {
    let Some(report) = what.report else {
        return vec![Finding {
            id: "answering".to_owned(),
            label: "services are answering".to_owned(),
            gravity: Gravity::Critical,
            verdict: Verdict::Unknown("the target has not been asked yet".to_owned()),
        }];
    };

    report
        .findings
        .iter()
        .map(|finding| {
            let name = finding.service.name.as_ref();
            let gravity = if finding.service.required {
                Gravity::Critical
            } else {
                Gravity::Warning
            };
            let verdict = if finding.reachability.open {
                Verdict::Well(format!(
                    "answering on {} - {}",
                    finding.service.port, finding.service.unlocks
                ))
            } else {
                Verdict::Unwell {
                    why: format!(
                        "not answering on {}, so {} is unavailable",
                        finding.service.port, finding.service.unlocks
                    ),
                    remedy: start_it_now(what, name),
                }
            };
            Finding {
                id: format!("answering:{name}"),
                label: format!("{name} is running"),
                gravity,
                verdict,
            }
        })
        .collect()
}

/// What would get a service answering **now**, which is not what gets it back after a restart.
fn start_it_now(what: &Known<'_>, service: &str) -> Remedy {
    // **This program cannot start anything while the loader is not answering.**
    //
    // Which is not the same as *the target is broken*, and the difference matters: a console
    // can run its whole chain with 9021 unreachable, and one measured here does. What is lost
    // is the ability to start something from this machine - both ways of doing it, sending an
    // ELF to the loader and asking `hbldr` to run one already on the disk, end at the same
    // place.
    //
    // It offered exactly that on a target where the loader was not answering, four lines above
    // a paragraph saying so.
    if what.loader_is_up() == Some(false) {
        return Remedy::Beyond(format!(
            "this program cannot start {service} while {} is not answering - both ways of \
             running a payload go through it. Loading it needs the exploit's own loader, which \
             means re-running the exploit. Whatever the target is already running is unaffected.",
            pros_link::service::LOADER.name
        ));
    }
    // **Never offered for something that was already loaded.**
    //
    // This is the one that has cost real time. `pldmgr` was not answering on 8084, so *load it
    // now* was offered - on a console whose startup list had demonstrably run, which means
    // `pldmgr` was the thing that ran it and was up the whole time. Starting it again started a
    // second copy and crashed the machine.
    //
    // A closed port means this program cannot reach it. It is not evidence that nothing is
    // there, and the difference is a reboot.
    if what.was_already_loaded(service) {
        return Remedy::Beyond(format!(
            "{service} was already loaded on this target, so a second copy is what starting it \
             again would produce - and that has crashed a console. A port that does not answer \
             means this program cannot reach it, not that nothing is running: it may be bound \
             to loopback, busy, or wedged. The log says which, and a restart is the way back \
             from wedged."
        ));
    }
    if let OnTarget::At(one) = what.on_target(service) {
        return Remedy::Ready(Plan {
            // **Said before the button, not after the crash.** Nothing here can see processes;
            // it sees ports, and a silent port is not an empty one.
            because: "it is already on the target, so this starts it without moving anything - \
                      it does not put it in a startup list. Nothing here can see whether a copy \
                      is already running: if one is, this makes two."
                .to_owned(),
            moves: vec![Move {
                step: Step::Run {
                    path: one.path.clone(),
                },
                already: false,
            }],
        });
    }
    let mut moves = match get_it_there(what, service) {
        Route::Steps(moves) => moves,
        Route::NotYet => return Remedy::Beyond(not_looked_yet(service)),
        Route::Nowhere => {
            return Remedy::Beyond(format!(
                "{service} is not on the target, not on this machine, and nothing describes \
                 where to get it"
            ));
        }
    };
    moves.push(Move {
        step: Step::Run {
            path: format!("{INTERNAL}/{service}"),
        },
        already: false,
    });
    Remedy::Ready(Plan {
        because: format!("so {service} is answering - a restart is a separate question"),
        moves,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        Fix, Gravity, Health, Known, Move, Step, Verdict, examine, health, named_as,
        put_it_in_the_list,
    };
    use crate::catalogue::Catalogue;
    use crate::chain::Chain;
    use crate::manifest::{Manifest, Payload};
    use crate::payloads::{There, Where};
    use crate::recovery::Kind;

    /// A report in which the loader is answering, or is not.
    fn with_loader(up: bool) -> crate::check::Report {
        let findings = pros_link::service::SERVICES
            .iter()
            .map(|service| crate::check::Finding {
                service: service.clone(),
                reachability: pros_link::service::Reachability {
                    open: if service.name == pros_link::service::LOADER.name {
                        up
                    } else {
                        true
                    },
                    took: std::time::Duration::from_millis(5),
                },
            })
            .collect();
        crate::check::Report::new("ps5", "127.0.0.1", findings)
    }

    fn described(name: &str, url: Option<&str>) -> Payload {
        Payload {
            name: name.to_owned(),
            // **A description says what file it arrives as**, and a list entry is that file.
            // A fixture without one is a payload nothing can write into a list, which is a
            // real case with its own test rather than the default for every other one.
            filename: Some(format!("{name}.elf")),
            url: url.map(ToOwned::to_owned),
            ..Payload::default()
        }
    }

    /// A description that never says what file it arrives as.
    fn unnameable(name: &str) -> Payload {
        Payload {
            name: name.to_owned(),
            url: Some("https://example/whatever".to_owned()),
            ..Payload::default()
        }
    }

    fn on(name: &str, path: &str, storage: Where) -> There {
        There {
            name: name.to_owned(),
            path: path.to_owned(),
            storage,
            about: None,
        }
    }

    /// **The gap this module exists to close**, stated as a test.
    ///
    /// A payload nobody has, that a list needs, used to produce two dead ends: a download
    /// button on one screen that never touched the list, and an add button on another that
    /// refused because the file was not on the target. One finding, one plan, three steps.
    #[test]
    fn a_payload_nobody_has_yet_is_fetched_sent_and_listed_in_one_plan() {
        let manifest = Manifest::new(vec![described("klogsrv", Some("https://example/klogsrv"))]);
        let known = Catalogue::builtin();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = put_it_in_the_list(&what, "klogsrv", "because".to_owned())
        else {
            panic!("a described payload has a route");
        };
        let steps: Vec<String> = plan.moves.iter().map(|one| one.step.describe()).collect();
        assert_eq!(steps.len(), 3, "{steps:?}");
        assert!(steps[0].starts_with("download"), "{steps:?}");
        assert!(steps[1].starts_with("send"), "{steps:?}");
        assert!(steps[2].starts_with("add"), "{steps:?}");
        assert!(plan.rewrites_the_list());
    }

    /// A payload already on this machine skips the download - and still shows it.
    #[test]
    fn what_is_already_here_is_marked_done_rather_than_hidden() {
        let manifest = Manifest::new(vec![described("klogsrv", Some("https://example/klogsrv"))]);
        let known = Catalogue::builtin();
        let staged = vec!["klogsrv_v0.6.elf".to_owned()];
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &staged,
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = put_it_in_the_list(&what, "klogsrv", "because".to_owned())
        else {
            panic!("it is here, so there is a route");
        };
        assert_eq!(plan.moves.len(), 3, "the shape of the job does not change");
        assert!(plan.moves[0].already, "the download is done");
        assert_eq!(plan.outstanding().len(), 2);
    }

    /// **A payload on a stick is two copies away, not one add away.**
    ///
    /// This is the case that used to print *copy it to /data/pldmgr/payloads first* and stop.
    #[test]
    fn a_payload_on_removable_storage_is_brought_over_before_it_is_listed() {
        let manifest = Manifest::new(vec![]);
        let known = Catalogue::builtin();
        let there = vec![on(
            "ftpsrv_v0.21.elf",
            "/mnt/usb0/ftpsrv_v0.21.elf",
            Where::Removable,
        )];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = put_it_in_the_list(&what, "ftpsrv", "because".to_owned())
        else {
            panic!("a copy on a stick is still a copy");
        };
        let steps: Vec<String> = plan.moves.iter().map(|one| one.step.describe()).collect();
        assert!(steps[0].contains("copy ftpsrv off the target"), "{steps:?}");
        assert!(steps[1].starts_with("send"), "{steps:?}");
        assert!(steps[2].starts_with("add"), "{steps:?}");
    }

    /// **A target nobody has listed does not license a download.**
    ///
    /// The payload may be sitting on the drive already. Proposing to fetch it because the
    /// listing has not been read is a plan built on an absence of evidence, and the person
    /// carrying it out has no way to tell that from a plan built on a measurement.
    #[test]
    fn an_unlisted_target_is_not_treated_as_an_empty_one() {
        let manifest = Manifest::new(vec![described("klogsrv", Some("https://example/klogsrv"))]);
        let known = Catalogue::builtin();
        let unlisted = Known {
            report: None,
            there: None,
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };
        let remedy = put_it_in_the_list(&unlisted, "klogsrv", "because".to_owned());
        let super::Remedy::Beyond(said) = &remedy else {
            panic!("nobody looked, so there is no route to propose: {remedy:?}");
        };
        assert!(said.contains("have not been listed"), "{said}");

        // The same snapshot, having actually looked, does propose one.
        let listed = Known {
            there: Some(&[]),
            ..unlisted
        };
        assert!(matches!(
            put_it_in_the_list(&listed, "klogsrv", "because".to_owned()),
            super::Remedy::Ready(_)
        ));
    }

    /// **An unread scan is not an absence.**
    ///
    /// Carried over from the check screen, where this was a real bug: the payload scan only
    /// ran when somebody opened the startup list, so the screen giving the advice reasoned
    /// from nothing and drew it as *not there*.
    #[test]
    fn nothing_listed_is_unknown_rather_than_absent() {
        let manifest = Manifest::new(vec![]);
        let catalogue = Catalogue::builtin();
        let what = Known {
            report: None,
            there: None,
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        assert_eq!(what.on_target("klogsrv"), super::OnTarget::Unknown);
    }

    /// A listing that found it says where, so something can be done about it.
    #[test]
    fn a_payload_on_the_target_comes_back_with_its_path() {
        let manifest = Manifest::new(vec![]);
        let catalogue = Catalogue::builtin();
        let there = vec![on(
            "klogsrv_v0.9.elf",
            "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf",
            Where::Internal,
        )];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        let super::OnTarget::At(one) = what.on_target("klogsrv") else {
            panic!("it is on the target");
        };
        assert_eq!(one.path, "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf");
    }

    /// A listing that ran and found nothing is a real absence, and fetching is then right.
    #[test]
    fn a_listing_that_found_nothing_is_an_absence() {
        let manifest = Manifest::new(vec![]);
        let catalogue = Catalogue::builtin();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        assert_eq!(what.on_target("klogsrv"), super::OnTarget::Absent);
    }

    /// **A copy on the drive beats a copy on a stick**, whichever came first in the listing.
    ///
    /// Only one of the two can be resolved by a startup list, so returning whichever was
    /// found first would report the payload as present and then build a list that fails at
    /// every boot.
    #[test]
    fn internal_storage_wins_over_a_stick() {
        let manifest = Manifest::new(vec![]);
        let catalogue = Catalogue::builtin();
        let there = vec![
            on(
                "ftpsrv_v0.21.elf",
                "/mnt/usb0/ftpsrv_v0.21.elf",
                Where::Removable,
            ),
            on(
                "ftpsrv_v0.21.elf",
                "/data/pldmgr/payloads/ftpsrv_v0.21.elf",
                Where::Internal,
            ),
        ];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        let super::OnTarget::At(one) = what.on_target("ftpsrv") else {
            panic!("it is on the target twice");
        };
        assert_eq!(one.storage, Where::Internal, "{}", one.path);
    }

    /// **Two findings about one payload do not fetch it twice.**
    ///
    /// The overlap is the normal case: *it is not answering* and *it is not in the startup
    /// list* both want the same file on the target, and a combined plan that listed the
    /// download twice would be showing somebody work it had already decided to skip.
    #[test]
    fn a_combined_plan_does_repeated_work_once() {
        let fetch = Move {
            step: Step::Fetch {
                payload: "klogsrv".to_owned(),
            },
            already: false,
        };
        let send = Move {
            step: Step::Send {
                payload: "klogsrv".to_owned(),
                to: crate::payloads::INTERNAL,
            },
            already: false,
        };
        let running = super::Plan {
            because: "so it answers".to_owned(),
            moves: vec![
                fetch.clone(),
                send.clone(),
                Move {
                    step: Step::Run {
                        path: "/data/pldmgr/payloads/klogsrv".to_owned(),
                    },
                    already: false,
                },
            ],
        };
        let listed = super::Plan {
            because: "so it comes back".to_owned(),
            moves: vec![
                fetch,
                send,
                Move {
                    step: Step::List(Fix::Add("klogsrv".to_owned())),
                    already: false,
                },
            ],
        };

        let both = super::Plan::all_of(&[running, listed]);
        assert_eq!(both.moves.len(), 4, "{:?}", both.moves);
        let fetches = both
            .moves
            .iter()
            .filter(|one| matches!(one.step, Step::Fetch { .. }))
            .count();
        assert_eq!(fetches, 1, "it is downloaded once");
        assert!(both.rewrites_the_list());
        assert!(both.because.contains("2 findings"), "{}", both.because);
    }

    /// The order each plan needed is the order the combined one keeps.
    #[test]
    fn a_combined_plan_keeps_the_order_its_steps_needed() {
        let plan = super::Plan {
            because: String::new(),
            moves: vec![
                Move {
                    step: Step::Fetch {
                        payload: "a".to_owned(),
                    },
                    already: false,
                },
                Move {
                    step: Step::Send {
                        payload: "a".to_owned(),
                        to: crate::payloads::INTERNAL,
                    },
                    already: false,
                },
                Move {
                    step: Step::List(Fix::Add("a".to_owned())),
                    already: false,
                },
            ],
        };
        let both = super::Plan::all_of(std::slice::from_ref(&plan));
        assert_eq!(both.moves, plan.moves, "one plan combined is itself");
    }

    /// **A copy on the console's USB never beats a copy on this machine.**
    ///
    /// The exact advice this replaced: pldmgr was on the target's stick and also staged here,
    /// and the plan said *copy pldmgr off the target, from `/mnt/usb0/...`* - two network trips
    /// to drag a file off the console and hand it straight back. One send was the whole job.
    #[test]
    fn a_payload_already_here_is_sent_rather_than_dragged_off_the_console() {
        let manifest = Manifest::new(vec![described("pldmgr", Some("https://example/pldmgr"))]);
        let known = Catalogue::builtin();
        let there = vec![on(
            "pldmgr_v0.5.1.elf",
            "/mnt/usb0/ps5_autoloader/pldmgr_v0.5.1.elf",
            Where::Unreachable,
        )];
        let staged = vec!["pldmgr_v0.5.1.elf".to_owned()];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &staged,
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = put_it_in_the_list(&what, "pldmgr", "because".to_owned())
        else {
            panic!("it is on this machine, so there is a route");
        };
        let steps: Vec<String> = plan.moves.iter().map(|one| one.step.describe()).collect();
        assert!(
            !steps.iter().any(|step| step.contains("off the target")),
            "nothing should be dragged off the console: {steps:?}"
        );
        assert_eq!(plan.outstanding().len(), 2, "{steps:?}");
        assert!(
            plan.moves[0].already,
            "the download is already done: {steps:?}"
        );
    }

    /// **A verified download beats scavenging from the console**, when there is one to be had.
    #[test]
    fn a_described_payload_is_downloaded_rather_than_dragged_off_the_console() {
        let manifest = Manifest::new(vec![described("pldmgr", Some("https://example/pldmgr"))]);
        let known = Catalogue::builtin();
        let there = vec![on(
            "pldmgr_v0.5.1.elf",
            "/mnt/usb0/ps5_autoloader/pldmgr_v0.5.1.elf",
            Where::Unreachable,
        )];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = put_it_in_the_list(&what, "pldmgr", "because".to_owned())
        else {
            panic!("it is described, so there is a route");
        };
        let steps: Vec<String> = plan.moves.iter().map(|one| one.step.describe()).collect();
        assert!(steps[0].starts_with("download"), "{steps:?}");
        assert!(
            !steps.iter().any(|step| step.contains("off the target")),
            "{steps:?}"
        );
    }

    /// **And it is still the answer when it is the only one.** A copy on the console is a
    /// worse route, not a forbidden one - dropping it would leave nothing to offer for a
    /// payload nobody has and nothing describes.
    #[test]
    fn a_copy_on_the_console_is_used_when_there_is_no_other_route() {
        let manifest = Manifest::new(vec![]);
        let known = Catalogue::builtin();
        let there = vec![on(
            "pldmgr_v0.5.1.elf",
            "/mnt/usb0/ps5_autoloader/pldmgr_v0.5.1.elf",
            Where::Unreachable,
        )];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = put_it_in_the_list(&what, "pldmgr", "because".to_owned())
        else {
            panic!("the only copy is still a copy");
        };
        let steps: Vec<String> = plan.moves.iter().map(|one| one.step.describe()).collect();
        assert!(steps[0].contains("off the target"), "{steps:?}");
    }

    /// **A configurator's list is in the tracked order, not the order anything was found.**
    #[test]
    fn setting_up_produces_the_recommended_order() {
        let manifest = Manifest::new(vec![
            described("pldmgr", Some("https://example/pldmgr")),
            described("kstuff-lite", Some("https://example/kstuff-lite")),
            described("ftpsrv", Some("https://example/ftpsrv")),
        ]);
        let known = Catalogue::builtin();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Autoloader,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = super::provision(
            &what,
            "/mnt/usb0/ps5_autoloader/autoload.txt",
            Kind::Autoloader,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, into }) = plan.moves.last().map(|one| one.step.clone())
        else {
            panic!("the last step writes the file");
        };
        assert_eq!(into, "/mnt/usb0/ps5_autoloader/autoload.txt");
        // The kernel patch goes first; pldmgr runs a list of its own and goes last.
        assert_eq!(
            entries.first().map(String::as_str),
            Some("kstuff-lite.elf"),
            "{entries:?}"
        );
        assert_eq!(
            entries.last().map(String::as_str),
            Some("pldmgr.elf"),
            "{entries:?}"
        );
        assert!(plan.rewrites_the_list());
    }

    /// **The manager's own list never names the loader or the manager.**
    ///
    /// Both are already up by the time it is read - one is what read it, the other is what it
    /// was loaded through - so listing either is at best pointless and at worst the end of the
    /// chain.
    #[test]
    fn setting_up_the_managers_own_list_leaves_out_what_cannot_work_in_it() {
        let manifest = Manifest::new(vec![
            described("elfldr", Some("https://example/elfldr")),
            described("pldmgr", Some("https://example/pldmgr")),
            described("ftpsrv", Some("https://example/ftpsrv")),
        ]);
        let known = Catalogue::builtin();
        // With the loader answering, both are left out; with it silent only the manager is.
        let up = with_loader(true);
        let what = Known {
            report: Some(&up),
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = super::provision(
            &what,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, .. }) = plan.moves.last().map(|one| one.step.clone())
        else {
            panic!("the last step writes the file");
        };
        assert!(
            !entries.iter().any(|one| one == "elfldr.elf"),
            "{entries:?}"
        );
        assert!(
            !entries.iter().any(|one| one == "pldmgr.elf"),
            "{entries:?}"
        );
        assert!(entries.iter().any(|one| one == "ftpsrv.elf"), "{entries:?}");

        // The manager stays out whatever happens; the loader comes in once it is silent.
        let down = with_loader(false);
        let quiet = Known {
            report: Some(&down),
            ..what
        };
        let (plan, _) = super::provision(
            &quiet,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, .. }) = plan.moves.last().map(|one| one.step.clone())
        else {
            panic!("the last step writes the file");
        };
        assert!(entries.iter().any(|one| one == "elfldr.elf"), "{entries:?}");
        assert!(
            !entries.iter().any(|one| one == "pldmgr.elf"),
            "{entries:?}"
        );
    }

    /// **A payload with no route is left out and named**, never listed unresolvable.
    #[test]
    fn what_cannot_be_got_is_named_rather_than_listed() {
        let manifest = Manifest::new(vec![described("ftpsrv", Some("https://example/ftpsrv"))]);
        let known = Catalogue::builtin();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Autoloader,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, left_out) = super::provision(
            &what,
            crate::chain::PATH,
            Kind::Autoloader,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, .. }) = plan.moves.last().map(|one| one.step.clone())
        else {
            panic!("the last step writes the file");
        };
        assert_eq!(
            entries,
            vec!["ftpsrv.elf".to_owned()],
            "only the one with a route"
        );
        assert!(!left_out.is_empty(), "and the rest are named");
        assert!(
            left_out.iter().any(|one| one.starts_with("pldmgr")),
            "{left_out:?}"
        );
    }

    /// **A loader that is down cannot be started by anything, including itself.**
    ///
    /// This offered *load it now, without sending anything* on exactly the target whose check
    /// screen said, four lines below, that the loader is down and reloading a payload will not
    /// help. Both were on screen at once and one of them was a button - and `hbldr` reaches the
    /// disk through the same `elfldr_spawn` the loader port uses, so it fails precisely when it
    /// looks most useful.
    #[test]
    fn nothing_is_offered_to_be_started_while_the_loader_is_down() {
        let manifest = Manifest::new(vec![]);
        let known = Catalogue::builtin();
        let there = vec![on(
            "klogsrv_v0.9.elf",
            "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf",
            Where::Internal,
        )];
        let report = with_loader(false);
        let what = Known {
            report: Some(&report),
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let remedy = super::start_it_now(&what, "klogsrv");
        let super::Remedy::Beyond(said) = &remedy else {
            panic!("there is nothing that can start it: {remedy:?}");
        };
        assert!(said.contains("re-run"), "{said}");
    }

    /// With the loader answering, the same payload on the same disk is one press.
    #[test]
    fn a_payload_on_the_disk_is_started_when_the_loader_is_up() {
        let manifest = Manifest::new(vec![]);
        let known = Catalogue::builtin();
        let there = vec![on(
            "klogsrv_v0.9.elf",
            "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf",
            Where::Internal,
        )];
        let report = with_loader(true);
        let what = Known {
            report: Some(&report),
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = super::start_it_now(&what, "klogsrv") else {
            panic!("it is there and the loader is up");
        };
        assert_eq!(plan.outstanding().len(), 1);
    }

    /// **The loader last in the manager's own list is the deliberate way to run it.**
    ///
    /// Everything else has loaded by the time it starts, and what it is there for is to still
    /// be running afterwards holding 9021 open. This was reported as a hazard, with the words
    /// *the 0 after it depend on it surviving being sent to itself* - a sentence the hazard's
    /// own carried count disproves.
    #[test]
    fn the_loader_last_in_the_managers_list_is_not_a_hazard() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
        let up = with_loader(true);
        let chain = Chain::parse(
            "kstuff-lite_v1.09.elf
nanodns.elf
ShadowMountPlus_1.6beta16.elf
             ps5upload-4.1.2.elf
ftpsrv_v0.21.elf
klogsrv_v0.9.elf
shsrv_v0.20.elf
             elfldr_v0.24.elf
",
        );
        let what = Known {
            report: Some(&up),
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let findings = examine(&what);
        assert!(
            !findings.iter().any(|one| one.id == "loader-in-list"),
            "nothing comes after it, so nothing can pay: {:?}",
            findings.iter().map(|one| &one.id).collect::<Vec<_>>()
        );
    }

    /// **With entries after it, it is still a hazard** - that is what the finding is for.
    #[test]
    fn the_loader_with_things_after_it_is_still_a_hazard() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
        let up = with_loader(true);
        let chain = Chain::parse(
            "elfldr_v0.24.elf
ftpsrv_v0.21.elf
shsrv_v0.20.elf
",
        );
        let what = Known {
            report: Some(&up),
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };
        assert!(
            examine(&what).iter().any(|one| one.id == "loader-in-list"),
            "two entries after it are two that pay"
        );
    }

    /// **The one that cost an hour, twice.**
    ///
    /// `pldmgr` was not answering on 8084, so *load it now* was offered - on a console whose
    /// startup list had demonstrably run, which means `pldmgr` was the thing that ran it and was
    /// up the whole time. Starting it again started a second copy and crashed the machine.
    #[test]
    fn what_runs_the_list_is_never_offered_a_second_copy() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
        // The list came off the target, so something read it - and the thing that reads lists
        // is the thing being asked about.
        let chain = Chain::parse(
            "ftpsrv_v0.21.elf
klogsrv_v0.9.elf
shsrv_v0.20.elf
",
        );
        let there = vec![on(
            "pldmgr_v0.5.1.elf",
            "/data/pldmgr/payloads/pldmgr/pldmgr_v0.5.1.elf",
            Where::Internal,
        )];
        let up = with_loader(true);
        let what = Known {
            report: Some(&up),
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let remedy = super::start_it_now(&what, "pldmgr");
        let super::Remedy::Beyond(said) = &remedy else {
            panic!("a second copy is not a fix: {remedy:?}");
        };
        assert!(said.contains("second copy"), "{said}");
        assert!(said.contains("cannot reach it"), "{said}");
    }

    /// **And nor is anything the startup list already loaded.**
    ///
    /// Being in a list that has run is the same evidence by a different route.
    #[test]
    fn a_payload_the_startup_list_loaded_is_never_offered_a_second_copy() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
        let chain = Chain::parse(
            "ftpsrv_v0.21.elf
klogsrv_v0.9.elf
shsrv_v0.20.elf
",
        );
        let there = vec![on(
            "klogsrv_v0.9.elf",
            "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf",
            Where::Internal,
        )];
        let up = with_loader(true);
        let what = Known {
            report: Some(&up),
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        assert!(
            matches!(
                super::start_it_now(&what, "klogsrv"),
                super::Remedy::Beyond(_)
            ),
            "it is in the list that ran, so it was loaded"
        );
    }

    /// Something the list never loaded is still offered, and still says what it cannot see.
    #[test]
    fn a_payload_no_list_loaded_is_offered_with_the_risk_stated() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
        let chain = Chain::parse(
            "ftpsrv_v0.21.elf
",
        );
        let there = vec![on(
            "ps5debug-NG.elf",
            "/data/pldmgr/payloads/ps5debug-NG/ps5debug-NG.elf",
            Where::Internal,
        )];
        let up = with_loader(true);
        let what = Known {
            report: Some(&up),
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = super::start_it_now(&what, "ps5debug-NG") else {
            panic!("nothing loaded it, so starting it is a real offer");
        };
        assert!(
            plan.because.contains("makes two"),
            "the risk is stated before the button: {}",
            plan.because
        );
    }

    /// **The manager's own list, deployed, is the list a working console runs.**
    ///
    /// The chain is taken from one rather than assembled, so this pins the result exactly: the
    /// eight entries in that order, `elfldr` last so it outlives the chain that loaded
    /// everything else, and no `pldmgr` because the list is what `pldmgr` reads.
    #[test]
    fn the_managers_chain_is_the_one_a_console_runs() {
        let preset = crate::recovery::baseline::first();
        let listed: Vec<&str> = preset
            .in_order(Kind::Manager)
            .iter()
            .filter(|one| {
                crate::recovery::can_work_in(&one.name, Kind::Manager, &Catalogue::builtin(), None)
            })
            .map(|one| one.name.clone())
            .collect::<Vec<String>>()
            .leak()
            .iter()
            .map(String::as_str)
            .collect();
        assert_eq!(
            listed,
            [
                "kstuff-lite",
                "nanoDNS",
                "ShadowMountPlus",
                "ps5upload",
                "ftpsrv",
                "klogsrv",
                "shsrv",
                "elfldr",
            ]
        );
    }

    /// **One kernel patch, and no rival chain inside this one.**
    ///
    /// It held both `kstuff` and `kstuff-lite`, which the chain file's own text says never to
    /// do, and `etaHEN` - which is a different way of bringing a console up, with a preset of
    /// its own, and which starts several of the payloads listed beside it.
    #[test]
    fn the_managers_chain_holds_no_rival_and_one_patch() {
        let names: Vec<String> = crate::recovery::baseline::first()
            .entries
            .iter()
            .map(|one| one.name.clone())
            .collect();
        assert!(names.iter().any(|one| one == "kstuff-lite"), "{names:?}");
        assert!(!names.iter().any(|one| one == "kstuff"), "{names:?}");
        assert!(!names.iter().any(|one| one == "etaHEN"), "{names:?}");
    }

    /// **A deployed list names files, not chain entries.**
    ///
    /// Deploying wrote `kstuff-lite` where `kstuff-lite_v1.09.elf` was needed. The list was
    /// well-formed, correctly ordered, and named six things that do not exist - every row
    /// reporting *not on the target*, from the one feature whose job is producing a list that
    /// loads.
    #[test]
    fn a_deployed_list_names_files_that_can_be_resolved() {
        let manifest = Manifest::new(vec![
            described("kstuff-lite", Some("https://example/kstuff-lite")),
            unnameable("ftpsrv"),
        ]);
        let known = Catalogue::builtin();
        // One already on the drive under its own name; one about to be sent under the
        // description's.
        let there = vec![on(
            "kstuff-lite_v1.09.elf",
            "/data/pldmgr/payloads/kstuff-lite/kstuff-lite_v1.09.elf",
            Where::Internal,
        )];
        let what = Known {
            report: None,
            there: Some(&there),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = super::provision(
            &what,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, .. }) = plan.moves.last().map(|one| one.step.clone())
        else {
            panic!("the last step writes the file");
        };
        assert!(
            entries.iter().any(|one| one == "kstuff-lite_v1.09.elf"),
            "it keeps the name of what is already there: {entries:?}"
        );
        assert!(
            !entries.iter().any(|one| one == "kstuff-lite"),
            "and never the chain's own name for it: {entries:?}"
        );
        // **A description with no filename is left out, not written as a bare name.** The
        // fixture gives ftpsrv no filename, which is the case that used to produce an entry
        // resolving to nothing.
        assert!(
            !entries.iter().any(|one| one == "ftpsrv"),
            "unnameable entries are left out: {entries:?}"
        );
        for entry in &entries {
            assert!(entry.contains('.'), "{entry} is a chain entry, not a file");
        }
    }

    /// **The loader is never in an autoloader's list**, because that autoloader loads it.
    ///
    /// From y2jb's own README: *"Do NOT include the kernel exploit or the `elf_loader` in
    /// autoload.txt; they are loaded automatically."* This file said to put it early, on the
    /// reasoning that everything after it loads through it - true, and the autoloader's job.
    #[test]
    fn the_loader_is_left_out_of_an_autoloaders_list_entirely() {
        let preset = crate::recovery::baseline::first();
        let auto: Vec<String> = preset
            .in_order(Kind::Autoloader)
            .iter()
            .map(|one| one.name.clone())
            .collect();
        assert!(!auto.iter().any(|one| one == "elfldr"), "{auto:?}");
        assert!(
            auto.iter().any(|one| one == "pldmgr"),
            "and the manager still is: {auto:?}"
        );

        // The manager's own list is the one place it belongs, and it goes last.
        let mgr: Vec<String> = preset
            .in_order(Kind::Manager)
            .iter()
            .map(|one| one.name.clone())
            .collect();
        assert_eq!(mgr.last().map(String::as_str), Some("elfldr"), "{mgr:?}");
    }

    /// Nowhere to get it from is said plainly rather than planned around.
    #[test]
    fn a_payload_with_no_route_says_so_instead_of_offering_steps() {
        let manifest = Manifest::new(vec![described("klogsrv", None)]);
        let known = Catalogue::builtin();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let remedy = put_it_in_the_list(&what, "klogsrv", "because".to_owned());
        let super::Remedy::Beyond(said) = remedy else {
            panic!("there is no route, so there is no plan: {remedy:?}");
        };
        assert!(said.contains("nothing describes where"), "{said}");
    }

    /// **The loader in the manager's own list is one edit and no transfers.**
    #[test]
    fn removing_the_loader_is_a_list_edit_alone() {
        let known = Catalogue::builtin();
        let chain = Chain::parse("kstuff.elf\nelfldr_v0.24.elf\nftpsrv.elf\npldmgr.elf");
        let manifest = Manifest::new(vec![]);
        // **While it is answering.** That is the whole of the hazard: a second copy finds 9021
        // already bound. After a boot chain has finished the first one has exited, and then
        // listing it is how somebody gets it back - see the test below.
        let up = with_loader(true);
        let what = Known {
            report: Some(&up),
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let findings = examine(&what);
        let one = findings
            .iter()
            .find(|finding| finding.id == "loader-in-list")
            .expect("the loader is in a manager list");
        let Verdict::Unwell {
            remedy: super::Remedy::Ready(plan),
            ..
        } = &one.verdict
        else {
            panic!("it has one edit that answers it");
        };
        assert_eq!(plan.moves.len(), 1);
        assert_eq!(plan.moves[0].step, Step::List(Fix::Remove("elfldr".into())));
        assert!(!plan.touches_the_target() || plan.rewrites_the_list());
    }

    /// **A failure always carries its remedy.** The type says so; this says it out loud, over
    /// a list that produces several different hazards at once.
    #[test]
    fn nothing_reports_a_failure_without_saying_what_would_answer_it() {
        let known = Catalogue::builtin();
        let chain = Chain::parse("elfldr_v0.24.elf\nnanoDNS.elf");
        let manifest = Manifest::new(vec![]);
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let findings = examine(&what);
        assert!(!findings.is_empty(), "this list has several things wrong");
        for finding in &findings {
            if let Verdict::Unwell { remedy, why } = &finding.verdict {
                assert!(!why.is_empty(), "{}", finding.id);
                match remedy {
                    super::Remedy::Ready(plan) => {
                        assert!(!plan.moves.is_empty(), "{}", finding.id);
                        assert!(!plan.because.is_empty(), "{}", finding.id);
                    }
                    super::Remedy::Choose { between, why } => {
                        assert!(!between.is_empty(), "{}", finding.id);
                        assert!(!why.is_empty(), "{}", finding.id);
                    }
                    super::Remedy::Beyond(said) => assert!(!said.is_empty(), "{}", finding.id),
                }
            }
        }
    }

    /// **Nothing measured is not a clean bill of health.**
    #[test]
    fn a_target_nobody_asked_is_unknown_rather_than_well() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let findings = examine(&what);
        assert_eq!(health(&findings), Health::Unknown);
        assert!(findings.iter().all(|one| !one.verdict.is_unwell()));
    }

    /// Nothing at all is unknown too - an empty report is not a passing one.
    #[test]
    fn no_findings_at_all_is_unknown() {
        assert_eq!(health(&[]), Health::Unknown);
    }

    /// A critical failure outranks a warning, and both outrank not knowing.
    #[test]
    fn the_worst_finding_decides_the_light() {
        let well = super::Finding {
            id: "a".to_owned(),
            label: String::new(),
            gravity: Gravity::Critical,
            verdict: Verdict::Well(String::new()),
        };
        let warned = super::Finding {
            gravity: Gravity::Warning,
            verdict: Verdict::Unwell {
                why: String::new(),
                remedy: super::Remedy::Beyond(String::new()),
            },
            ..well.clone()
        };
        let bad = super::Finding {
            gravity: Gravity::Critical,
            ..warned.clone()
        };
        assert_eq!(health(std::slice::from_ref(&well)), Health::Well);
        assert_eq!(health(&[well.clone(), warned.clone()]), Health::Warning);
        assert_eq!(health(&[well, warned, bad]), Health::Unwell);
    }

    /// The worst findings are drawn first, because that is the order to read them in.
    #[test]
    fn findings_are_ordered_worst_first() {
        let known = Catalogue::builtin();
        let chain = Chain::parse("elfldr_v0.24.elf\npldmgr.elf");
        let manifest = Manifest::new(vec![]);
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let findings = examine(&what);
        let ranks: Vec<u8> = findings
            .iter()
            .map(|one| super::rank(&one.verdict, one.gravity))
            .collect();
        assert!(ranks.windows(2).all(|pair| pair[0] >= pair[1]), "{ranks:?}");
    }

    /// A version on the end does not make it a different payload - the one rule, everywhere.
    #[test]
    fn a_versioned_file_is_the_service_it_names() {
        assert!(named_as("klogsrv_v0.6.elf", "klogsrv"));
        assert!(named_as("klogsrv", "klogsrv"));
        assert!(!named_as("klogsrv", "ftpsrv"));
    }

    /// Fetching changes nothing on the target, and a plan that only fetches says so.
    #[test]
    fn a_plan_that_only_downloads_does_not_touch_the_target() {
        let plan = super::Plan {
            because: String::new(),
            moves: vec![Move {
                step: Step::Fetch {
                    payload: "klogsrv".to_owned(),
                },
                already: false,
            }],
        };
        assert!(!plan.touches_the_target());
        assert!(!plan.rewrites_the_list());
        assert!(!plan.is_settled());
    }
}
