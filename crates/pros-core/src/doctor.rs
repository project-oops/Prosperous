//! Health checks that say what is wrong, and what would put it right.
//!
//! A check never changes anything. It is a pure function of a [`crate::doctor::Known`]
//! snapshot the window already gathered, and returns a [`crate::doctor::Plan`]: inert data
//! naming steps, with no connection, local path or target to carry them out. The window
//! executes a plan only after a person confirms it. A check that opened its own socket would be
//! untestable without a target and could disagree with the panel beside it.
//!
//! [`crate::doctor::Verdict::Unwell`] carries its [`crate::doctor::Remedy`], so no failure is
//! reported without an answer; [`crate::doctor::Remedy::Beyond`] says why nothing can fix it.

use crate::catalogue::Catalogue;
use crate::chain::Chain;
use crate::check::Report;
use crate::manifest::Manifest;
use crate::payloads::{INTERNAL, There, Where};
use crate::recovery::{Fix, Gravity, Hazard, Kind, audit, baseline};

/// One thing somebody could do, named but not done.
///
/// A step names a payload, never a local path or a target; the window binds the name to a file,
/// so a plan cannot be executed by whoever holds it.
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
    /// machine because a copy on the target is a shell command whose failure looks like success.
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
        ///
        /// Depends on the list: an autoloader resolves entries against its own list's
        /// directory, so deploying to a stick puts the files on the stick.
        to: String,
    },
    /// Change one line of the startup list.
    ///
    /// Several of these collapse into one write of the file, so an interruption cannot leave
    /// it half edited.
    List(Fix),
    /// Replace a startup list entirely, with this.
    ///
    /// Setting up from nothing writes one whole file rather than [`Step::List`] edits against
    /// a configuration that is being discarded.
    Rebuild {
        /// Which list, in full.
        ///
        /// Owned: the path comes from the chain that declares it, and a plan outlives the
        /// chooser it was built from.
        into: String,
        /// Every entry, in the order they will run.
        entries: Vec<String>,
    },
    /// Load something the target already has, now, without moving anything.
    ///
    /// Does not survive a restart: it answers "this is not running", never "this is not in
    /// the list".
    Run {
        /// The full path on the target.
        path: String,
    },
    /// Turn autoload on in the manager's settings, so the list it was just given is read.
    ///
    /// The manager ignores its list while autoload is off, so a manager chain is deployed with
    /// this step. It is a merge: only `AUTOLOAD_ENABLED` changes.
    Enable {
        /// The settings file - the manager's `pldmgr_config.txt`.
        into: String,
    },
    /// Put a file the chain carries back on the target, verbatim.
    ///
    /// A chain exported off a target carries copies of the files beside its list, such as the
    /// manager's settings. The bytes are written whole to the path they were read from, never
    /// parsed or merged. Only exported chains carry any - see
    /// [`crate::recovery::baseline::Captured`].
    Place {
        /// The full path on the target, where the file was read and will be written back.
        into: String,
        /// The bytes to write, as they were captured.
        content: String,
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
            Self::Enable { into } => format!("turn autoload on in {into}, so the list is read"),
            Self::Place { into, content } => {
                format!(
                    "restore {into} ({} bytes) as the chain carries it",
                    content.len()
                )
            }
        }
    }

    /// Whether carrying this out changes the target. Fetching does not.
    #[must_use]
    pub const fn touches_the_target(&self) -> bool {
        match self {
            Self::Fetch { .. } => false,
            Self::Bring { .. }
            | Self::Send { .. }
            | Self::List(_)
            | Self::Rebuild { .. }
            | Self::Run { .. }
            | Self::Enable { .. }
            | Self::Place { .. } => true,
        }
    }

    /// Whether this is an edit to the startup list.
    ///
    /// `Enable` and `Place` are not: they write files beside the list, not the list.
    #[must_use]
    pub const fn is_a_list_edit(&self) -> bool {
        matches!(self, Self::List(_) | Self::Rebuild { .. })
    }
}

/// A step, and whether it has already been done.
///
/// Satisfied steps stay in the plan, so a person sees the shape of the whole job.
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
    /// Named separately in a confirmation: the list decides whether the target comes back after
    /// a restart.
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
    /// Two findings about one payload overlap ("not answering" fetches and runs it, "not in
    /// the list" fetches and lists it), so a plain concatenation would fetch it twice.
    #[must_use]
    pub fn all_of(plans: &[Self]) -> Self {
        let mut moves: Vec<Move> = Vec::new();
        for plan in plans {
            for one in &plan.moves {
                // The first mention wins, keeping the earliest position a step needed.
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
    /// Known, not permitted: a ready plan is still confirmed by a person before it runs.
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
/// Four states, not pass and fail: "nothing was measured" is not "it is broken", and "this
/// does not apply here" is not "this is fine".
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
    /// A name for this check, unique among findings and stable across runs, so a fix in flight
    /// is matched to its finding after the target is asked again.
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
    /// Something was not measured. Ranks above `Well`, so an unreachable target never looks
    /// healthy.
    Unknown,
    /// Something failed that costs visibility rather than access.
    Warning,
    /// Something failed that can leave the target unreachable.
    Unwell,
}

/// The worst of what was found.
///
/// No findings at all is [`Health::Unknown`], not [`Health::Well`].
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
    if any { worst } else { Health::Unknown }
}

/// Everything the checks are allowed to look at.
///
/// A borrowed snapshot of what the window already asked for; nothing in it can ask anything
/// else.
#[derive(Debug, Clone, Copy)]
pub struct Known<'a> {
    /// What answered when the target was last probed.
    pub report: Option<&'a Report>,
    /// Every payload file found on the target, with where it lives.
    ///
    /// `None` means not listed, which is not an empty target: only an empty listing licenses a
    /// plan that downloads the file.
    pub there: Option<&'a [There]>,
    /// Payloads already on this machine, by the name their description gives.
    pub staged: &'a [String],
    /// What this program knows about payloads it has never seen.
    pub described: &'a Manifest,
    /// The startup list being examined.
    pub chain: Option<&'a Chain>,
    /// Which kind of list that is - the rules invert between them.
    pub kind: Kind,
    /// Where that list is, when the caller is looking at one.
    ///
    /// Decides where payloads go: an autoloader resolves entries against its list's directory.
    /// `None` means the manager's directory, which is resolved by scan.
    pub list: Option<&'a str>,
    /// The chain this target is meant to be running.
    ///
    /// Decides which absences are worth reporting: a target brought up by etaHEN is not missing
    /// an FTP server, because etaHEN is one.
    pub preset: &'a baseline::Preset,
    /// What each service is and what it unlocks.
    pub known: &'a Catalogue,
}

impl Known<'_> {
    /// The directory a payload has to be in for this list to resolve it.
    ///
    /// The payload manager scans: `SCAN_DIRS` in its header is `/data/pldmgr` and
    /// `/mnt/usb0..usb7/pldmgr`, so the internal payload directory serves its own list.
    /// The autoloader joins: `snprintf(full_path, ..., "%s%s", config_dir, line)` resolves a
    /// relative entry against the directory holding `autoload.txt`, so its payloads live beside
    /// its list.
    fn payloads_go(&self) -> String {
        match (self.kind, self.list) {
            (Kind::Autoloader, Some(list)) => beside(list),
            _ => INTERNAL.to_owned(),
        }
    }

    /// Whether a payload with this name is already on this machine.
    fn is_here(&self, service: &str) -> bool {
        self.staged.iter().any(|name| named_as(name, service))
    }

    /// Where a payload with this name is on the target, preferring somewhere usable.
    ///
    /// Internal storage wins, because a startup list can resolve only that copy.
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
    /// A silent port means only that this program cannot reach it; starting a second copy
    /// crashes the target. Being in a startup list that has run is evidence of loading, and so
    /// is being the list runner itself, whatever its port says.
    fn was_already_loaded(&self, service: &str) -> bool {
        if self
            .known
            .services()
            .iter()
            .any(|one| one.runs_lists && named_as(one.name.as_ref(), service))
        {
            // A list read off the target means the list runner is running.
            return self.chain.is_some();
        }
        self.chain
            .is_some_and(|chain| chain.position(service).is_some())
    }

    /// Whether the loader is answering.
    ///
    /// Starting a payload goes through it, whether sent to 9021 or started from disk with
    /// `hbldr`: both end at `elfldr_spawn`. Sending a file does not.
    fn loader_is_up(&self) -> Option<bool> {
        let report = self.report?;
        let loader = report.about(pros_link::service::LOADER.name.as_ref())?;
        Some(loader.reachability.open)
    }

    /// Whether this program knows where to get a payload it has never seen.
    fn can_fetch(&self, service: &str) -> bool {
        self.described.payloads().iter().any(|one| {
            named_as(&one.name, service)
                && (one.url.is_some() || crate::fetch::local_build(one).is_some())
        })
    }
}

/// Whether the target holds a payload, in three answers rather than two.
///
/// "Not listed yet" and "not there" differ: only the second licenses a download.
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
/// The startup list's own rule, so a plan never adds an entry the audit does not recognise.
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
/// 1. Already where a startup list can resolve it: nothing to do.
/// 2. On this machine: one send.
/// 3. Described with an address: download, checked against the stated digest.
/// 4. On the target where a list cannot use it: copy it off and send it back. Last, because it
///    is two network trips and yields no digest to check.
fn get_it_there(what: &Known<'_>, service: &str) -> Route {
    get_it_there_into(what, service, &what.payloads_go())
}

/// As [`get_it_there`], for a list other than the one the findings are about.
fn get_it_there_into(what: &Known<'_>, service: &str, to: &str) -> Route {
    let send = || Move {
        step: Step::Send {
            payload: service.to_owned(),
            to: to.to_owned(),
        },
        already: false,
    };
    let there = what.on_target(service);

    // Not having looked is no route: the copy may already be in place.
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
    // 2. On this machine. The download is listed and marked done, keeping the plan's shape.
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
    // 4. The copy on the target that a startup list cannot resolve.
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
/// Turns a person's pick from [`Remedy::Choose`] into a plan, by the same route a check
/// proposes.
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
/// A chain names payloads (`kstuff-lite`); a startup list names files, which must resolve on
/// disk. The answer is the file already on the target, or the described filename a send will
/// write. `None` when neither is known, and the payload is then left out.
fn will_be_called(what: &Known<'_>, service: &str) -> Option<String> {
    // Already where a list can resolve it: it is not being replaced, so it keeps its name.
    if let OnTarget::At(one) = what.on_target(service)
        && one.storage == Where::Internal
    {
        return Some(one.name.clone());
    }
    // Otherwise a send writes the described filename. A description with no filename gives
    // `None`, never the bare name, which would resolve to nothing.
    what.described
        .payloads()
        .iter()
        .find(|one| named_as(&one.name, service))
        .and_then(|one| one.filename.clone())
}

/// The directory holding a file.
///
/// A list path with its last segment removed, and no trailing slash - `/mnt/usb0/ps5_autoloader`
/// from `/mnt/usb0/ps5_autoloader/autoload.txt`. A path with no separator gives the manager's
/// directory.
fn beside(path: &str) -> String {
    match path.rfind('/') {
        Some(0) | None => INTERNAL.to_owned(),
        Some(at) => path[..at].to_owned(),
    }
}

/// A plan that sets a target up from nothing, in the recommended order.
///
/// For a target that has just run the entry point, or a stick being made into a way back in.
/// The order comes from [`crate::recovery::baseline`] and nothing is added to it. The plan is
/// inert until confirmed, and the list it writes then gets the same whole-file review as any
/// other write.
///
/// Returns the plan and the payloads left out: a payload with no route is named rather than
/// listed, since an unresolvable entry fails at every boot.
#[must_use]
pub fn provision(
    what: &Known<'_>,
    into: &str,
    kind: Kind,
    preset: &baseline::Preset,
) -> (Plan, Vec<String>) {
    let mut moves: Vec<Move> = Vec::new();
    let mut entries: Vec<String> = Vec::new();
    let mut left_out: Vec<String> = Vec::new();
    // Where the list being written resolves its entries from.
    let to = match kind {
        Kind::Autoloader => beside(into),
        Kind::Manager => INTERNAL.to_owned(),
    };

    for placed in preset.in_order(kind) {
        // `None` for the loader state: a deployed list is for the next boot, where the loader
        // belongs last in the manager's list whatever 9021 is doing now. The live check is
        // `audit`'s.
        if !crate::recovery::can_work_in(&placed.name, kind, what.known, None) {
            continue;
        }
        match get_it_there_into(what, &placed.name, &to) {
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
            into: into.to_owned(),
            entries: entries.clone(),
        },
        already: false,
    });
    // The manager reads its list only with autoload on; an autoloader's list needs no switch.
    if kind == Kind::Manager {
        moves.push(Move {
            step: Step::Enable {
                into: crate::autoload::CONFIG.to_owned(),
            },
            already: false,
        });
    }
    // Carried files go last, after the switch, so a captured settings file is the last word on
    // its path. Shipped chains carry none.
    for file in &preset.files {
        moves.push(Move {
            step: Step::Place {
                into: file.path.clone(),
                content: file.content.clone(),
            },
            already: false,
        });
    }
    let carried = preset.files.len();
    (
        Plan {
            because: format!(
                "a working chain from nothing: {} payloads into {to}, in the recommended order, \
                 and {into} replaced with the list that names them{}",
                entries.len(),
                match carried {
                    0 => String::new(),
                    1 => ", and 1 file the chain carries put back".to_owned(),
                    many => format!(", and {many} files the chain carries put back"),
                }
            ),
            moves,
        },
        left_out,
    )
}

/// The checks about the startup list alone, worst first.
///
/// For the screen that edits the list; [`examine`] also reports what is answering now.
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

/// Every check, run against one snapshot, worst first.
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

    // An unlisted target passes no files, so the audit makes no claim about where one is.
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
            // Names which list: the verdict is on one file, not the whole target.
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
    // The entry is already in the list; moving the file is the whole answer.
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

/// What would get a service answering now, which is not what gets it back after a restart.
fn start_it_now(what: &Known<'_>, service: &str) -> Remedy {
    // Nothing can be started from here while the loader is not answering: sending an ELF and
    // `hbldr` both go through it. The target itself may still be running its whole chain.
    if what.loader_is_up() == Some(false) {
        return Remedy::Beyond(format!(
            "this program cannot start {service} while {} is not answering - both ways of \
             running a payload go through it. Starting it again means re-running the entry point \
             that first started it. Whatever the target is already running is unaffected.",
            pros_link::service::LOADER.name
        ));
    }
    // Never offered for something already loaded: a closed port is not evidence that nothing
    // is running, and a second copy crashes the target.
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
            // Nothing here sees processes, only ports, so the risk is stated up front.
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
        Fix, Gravity, Health, Known, Move, Step, Verdict, examine, health, named_as, provision,
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
        crate::check::Report::new("prospero", "127.0.0.1", findings)
    }

    fn described(name: &str, url: Option<&str>) -> Payload {
        Payload {
            name: name.to_owned(),
            // A list entry is this file; `unnameable` covers a description without one.
            filename: Some(format!("{name}.elf")),
            url: url.map(ToOwned::to_owned),
            ..Payload::default()
        }
    }

    /// Every payload the shipped chain names, described well enough to be planned for.
    fn every_payload_described() -> Manifest {
        Manifest::new(
            crate::recovery::baseline::first()
                .entries
                .iter()
                .map(|one| described(&one.name, Some("https://example/whatever")))
                .collect(),
        )
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

    /// A payload nobody has is fetched, sent and listed in one three-step plan.
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
            list: None,
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

    /// A payload already on this machine marks the download done and still shows it.
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
            list: None,
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

    /// A payload only on a stick is brought over and sent before it is listed.
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
            list: None,
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

    /// A target nobody has listed gets no plan; the same target listed empty does.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };
        let remedy = put_it_in_the_list(&unlisted, "klogsrv", "because".to_owned());
        let super::Remedy::Beyond(said) = &remedy else {
            panic!("nobody looked, so there is no route to propose: {remedy:?}");
        };
        assert!(said.contains("have not been listed"), "{said}");

        let listed = Known {
            there: Some(&[]),
            ..unlisted
        };
        assert!(matches!(
            put_it_in_the_list(&listed, "klogsrv", "because".to_owned()),
            super::Remedy::Ready(_)
        ));
    }

    /// An unread scan is unknown, not absent.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        assert_eq!(what.on_target("klogsrv"), super::OnTarget::Unknown);
    }

    /// A listing that found the payload returns its path.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        let super::OnTarget::At(one) = what.on_target("klogsrv") else {
            panic!("it is on the target");
        };
        assert_eq!(one.path, "/data/pldmgr/payloads/klogsrv/klogsrv_v0.9.elf");
    }

    /// A listing that ran and found nothing is an absence.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        assert_eq!(what.on_target("klogsrv"), super::OnTarget::Absent);
    }

    /// A copy on internal storage beats a copy on a stick, whatever the listing order.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &catalogue,
        };
        let super::OnTarget::At(one) = what.on_target("ftpsrv") else {
            panic!("it is on the target twice");
        };
        assert_eq!(one.storage, Where::Internal, "{}", one.path);
    }

    /// Two findings about one payload combine into a plan that fetches it once.
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
                to: crate::payloads::INTERNAL.to_owned(),
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
                        to: crate::payloads::INTERNAL.to_owned(),
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

    /// A copy on this machine beats a copy on the target's stick.
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
            list: None,
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

    /// A verified download beats copying off the target.
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
            list: None,
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

    /// A copy on the target is still used when it is the only route.
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
            list: None,
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

    /// A provisioned autoloader list holds only the manager.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = provision(
            &what,
            "/mnt/usb0/ps5_autoloader/autoload.txt",
            Kind::Autoloader,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, into }) = plan
            .moves
            .iter()
            .rev()
            .map(|one| one.step.clone())
            .find(|step| matches!(step, Step::Rebuild { .. }))
        else {
            panic!("the last step writes the file");
        };
        assert_eq!(into, "/mnt/usb0/ps5_autoloader/autoload.txt");
        // pldmgr loads the rest of the chain from its own list.
        assert_eq!(entries, ["pldmgr.elf"], "{entries:?}");
        assert!(plan.rewrites_the_list());
    }

    /// The manager's own list includes the loader whatever 9021 is doing, and never the manager.
    #[test]
    fn setting_up_the_managers_own_list_includes_the_loader_whatever_9021_is_doing() {
        let manifest = Manifest::new(vec![
            described("elfldr", Some("https://example/elfldr")),
            described("pldmgr", Some("https://example/pldmgr")),
            described("ftpsrv", Some("https://example/ftpsrv")),
        ]);
        let known = Catalogue::builtin();
        for answering in [true, false] {
            let report = with_loader(answering);
            let what = Known {
                report: Some(&report),
                there: Some(&[]),
                staged: &[],
                described: &manifest,
                chain: None,
                kind: Kind::Manager,
                list: None,
                preset: &crate::recovery::baseline::first(),
                known: &known,
            };
            let (plan, _) = provision(
                &what,
                crate::chain::PATH,
                Kind::Manager,
                &crate::recovery::baseline::first(),
            );
            let Some(Step::Rebuild { entries, .. }) = plan
                .moves
                .iter()
                .rev()
                .map(|one| one.step.clone())
                .find(|step| matches!(step, Step::Rebuild { .. }))
            else {
                panic!("the last step writes the file");
            };
            assert!(
                entries.iter().any(|one| one == "elfldr.elf"),
                "the loader is in the manager's own list, 9021 answering={answering}: {entries:?}"
            );
            assert!(
                !entries.iter().any(|one| one == "pldmgr.elf"),
                "the manager is not in the list it reads: {entries:?}"
            );
            assert!(entries.iter().any(|one| one == "ftpsrv.elf"), "{entries:?}");
        }
    }

    /// Deploying a manager chain turns autoload on; deploying an autoloader list does not.
    #[test]
    fn deploying_the_managers_list_also_enables_autoload() {
        let manifest = Manifest::new(vec![described("ftpsrv", Some("https://example/ftpsrv"))]);
        let known = Catalogue::builtin();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };
        let (manager, _) = provision(
            &what,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        assert!(
            manager
                .moves
                .iter()
                .any(|one| matches!(one.step, Step::Enable { .. })),
            "deploying the manager's list turns autoload on: {:?}",
            manager.moves
        );
        let (autoloader, _) = provision(
            &what,
            "/mnt/usb0/ps5_autoloader/autoload.txt",
            Kind::Autoloader,
            &crate::recovery::baseline::first(),
        );
        assert!(
            !autoloader
                .moves
                .iter()
                .any(|one| matches!(one.step, Step::Enable { .. })),
            "an autoloader's list is read regardless and needs no switch: {:?}",
            autoloader.moves
        );
    }

    /// A chain that carries files puts each back verbatim, after the list and the switch.
    #[test]
    fn a_chain_that_carries_files_has_them_restored_on_deploy() {
        let manifest = Manifest::new(vec![described("ftpsrv", Some("https://example/ftpsrv"))]);
        let known = Catalogue::builtin();
        let mut preset = crate::recovery::baseline::first();
        preset.files = vec![crate::recovery::baseline::Captured {
            label: "payload manager settings".to_owned(),
            path: "/data/pldmgr/pldmgr_config.txt".to_owned(),
            content: "AUTOLOAD_ENABLED=1\nAUTOLOAD_DELAY=5\n".to_owned(),
        }];
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            list: None,
            preset: &preset,
            known: &known,
        };
        let (plan, _) = provision(&what, crate::chain::PATH, Kind::Manager, &preset);

        let place_at = plan
            .moves
            .iter()
            .position(|one| {
                matches!(&one.step, Step::Place { into, content }
                    if into == "/data/pldmgr/pldmgr_config.txt"
                        && content == "AUTOLOAD_ENABLED=1\nAUTOLOAD_DELAY=5\n")
            })
            .expect("the carried settings file is put back verbatim");
        let rebuild_at = plan
            .moves
            .iter()
            .position(|one| matches!(one.step, Step::Rebuild { .. }))
            .expect("the list is written");
        let enable_at = plan
            .moves
            .iter()
            .position(|one| matches!(one.step, Step::Enable { .. }))
            .expect("the switch is flipped");
        assert!(
            place_at > rebuild_at && place_at > enable_at,
            "the carried file is put back after the list and the switch: {:?}",
            plan.moves
        );
    }

    /// A shipped chain carries no files, so deploying it places none.
    #[test]
    fn a_chain_that_carries_no_files_places_nothing() {
        let manifest = Manifest::new(vec![described("ftpsrv", Some("https://example/ftpsrv"))]);
        let known = Catalogue::builtin();
        let preset = crate::recovery::baseline::first();
        assert!(
            preset.files.is_empty(),
            "a shipped preset ships no captured files"
        );
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            list: None,
            preset: &preset,
            known: &known,
        };
        let (plan, _) = provision(&what, crate::chain::PATH, Kind::Manager, &preset);
        assert!(
            !plan
                .moves
                .iter()
                .any(|one| matches!(one.step, Step::Place { .. })),
            "nothing to put back: {:?}",
            plan.moves
        );
    }

    /// A payload with no route is left out and named, never listed unresolvable.
    #[test]
    fn what_cannot_be_got_is_named_rather_than_listed() {
        let manifest = Manifest::new(vec![described("ftpsrv", Some("https://example/ftpsrv"))]);
        let known = Catalogue::builtin();
        // The manager's own list, because the autoloader's holds only the manager.
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, left_out) = provision(
            &what,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, .. }) = plan
            .moves
            .iter()
            .rev()
            .map(|one| one.step.clone())
            .find(|step| matches!(step, Step::Rebuild { .. }))
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
            left_out.iter().any(|one| one.starts_with("kstuff-lite")),
            "{left_out:?}"
        );
    }

    /// Nothing is offered to be started while the loader is down; `hbldr` needs it too.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let remedy = super::start_it_now(&what, "klogsrv");
        let super::Remedy::Beyond(said) = &remedy else {
            panic!("there is nothing that can start it: {remedy:?}");
        };
        assert!(said.contains("re-run"), "{said}");
    }

    /// With the loader answering, a payload on the disk is started in one step.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let super::Remedy::Ready(plan) = super::start_it_now(&what, "klogsrv") else {
            panic!("it is there and the loader is up");
        };
        assert_eq!(plan.outstanding().len(), 1);
    }

    /// The loader last in the manager's own list is not a hazard: nothing after it can pay.
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
            list: None,
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

    /// The loader with entries after it is still a hazard.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };
        assert!(
            examine(&what).iter().any(|one| one.id == "loader-in-list"),
            "two entries after it are two that pay"
        );
    }

    /// The list runner is never offered a second copy when its list was read off the target.
    #[test]
    fn what_runs_the_list_is_never_offered_a_second_copy() {
        let known = Catalogue::builtin();
        let manifest = Manifest::new(vec![]);
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
            list: None,
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

    /// A payload the startup list already loaded is never offered a second copy.
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
            list: None,
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

    /// A payload no list loaded is offered, with the second-copy risk stated.
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
            list: None,
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

    /// The shipped manager chain pins its exact order: `elfldr` last, no `pldmgr`.
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
                "pltauth-patch",
                "sandbox-daemon",
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

    /// The manager chain holds one kernel patch and no `etaHEN`, which has its own preset.
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

    /// A deployed list names files, not chain entries.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = provision(
            &what,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        let Some(Step::Rebuild { entries, .. }) = plan
            .moves
            .iter()
            .rev()
            .map(|one| one.step.clone())
            .find(|step| matches!(step, Step::Rebuild { .. }))
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
        // The fixture gives ftpsrv no filename, so it is left out rather than written bare.
        assert!(
            !entries.iter().any(|one| one == "ftpsrv"),
            "unnameable entries are left out: {entries:?}"
        );
        for entry in &entries {
            assert!(entry.contains('.'), "{entry} is a chain entry, not a file");
        }
    }

    /// A chain deployed to a stick sends its payloads beside the stick's `autoload.txt`.
    #[test]
    fn deploying_to_a_stick_sends_the_payloads_to_the_stick() {
        let known = Catalogue::builtin();
        let manifest = every_payload_described();
        let staged: Vec<String> = known
            .services()
            .iter()
            .map(|one| one.name.to_string())
            .collect();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &staged,
            described: &manifest,
            chain: None,
            kind: Kind::Autoloader,
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = provision(
            &what,
            "/mnt/usb0/ps5_autoloader/autoload.txt",
            Kind::Autoloader,
            &crate::recovery::baseline::first(),
        );
        let sends: Vec<String> = plan
            .moves
            .iter()
            .filter_map(|one| match &one.step {
                Step::Send { to, .. } => Some(to.clone()),
                _ => None,
            })
            .collect();
        assert!(!sends.is_empty(), "nothing was sent: {plan:?}");
        for to in &sends {
            assert_eq!(to, "/mnt/usb0/ps5_autoloader", "{sends:?}");
        }
    }

    /// The manager's own list sends its payloads to the internal directory the manager scans.
    #[test]
    fn deploying_the_managers_own_list_sends_to_where_it_scans() {
        let known = Catalogue::builtin();
        let manifest = every_payload_described();
        let staged: Vec<String> = known
            .services()
            .iter()
            .map(|one| one.name.to_string())
            .collect();
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &staged,
            described: &manifest,
            chain: None,
            kind: Kind::Manager,
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let (plan, _) = provision(
            &what,
            crate::chain::PATH,
            Kind::Manager,
            &crate::recovery::baseline::first(),
        );
        for one in &plan.moves {
            if let Step::Send { to, .. } = &one.step {
                assert_eq!(to, crate::payloads::INTERNAL, "{plan:?}");
            }
        }
    }

    /// The loader is never in an autoloader's list, and is last in the manager's.
    ///
    /// In y2jb's source, `autoload.js` waits for the loader on 9021 before reading any list.
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

        let mgr: Vec<String> = preset
            .in_order(Kind::Manager)
            .iter()
            .map(|one| one.name.clone())
            .collect();
        assert_eq!(mgr.last().map(String::as_str), Some("elfldr"), "{mgr:?}");
    }

    /// A payload with no route gets a `Beyond`, not steps.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let remedy = put_it_in_the_list(&what, "klogsrv", "because".to_owned());
        let super::Remedy::Beyond(said) = remedy else {
            panic!("there is no route, so there is no plan: {remedy:?}");
        };
        assert!(said.contains("nothing describes where"), "{said}");
    }

    /// Removing the loader from the manager's own list is one edit and no transfers.
    #[test]
    fn removing_the_loader_is_a_list_edit_alone() {
        let known = Catalogue::builtin();
        let chain = Chain::parse("kstuff.elf\nelfldr_v0.24.elf\nftpsrv.elf\npldmgr.elf");
        let manifest = Manifest::new(vec![]);
        // The hazard exists only while the loader is answering.
        let up = with_loader(true);
        let what = Known {
            report: Some(&up),
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            list: None,
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

    /// Every failure carries a non-empty remedy, over a list with several hazards.
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
            list: None,
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

    /// A target nobody asked is unknown, not well.
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
            list: None,
            preset: &crate::recovery::baseline::first(),
            known: &known,
        };

        let findings = examine(&what);
        assert_eq!(health(&findings), Health::Unknown);
        assert!(findings.iter().all(|one| !one.verdict.is_unwell()));
    }

    /// No findings at all is unknown.
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

    /// Findings are ordered worst first.
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
            list: None,
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

    /// A versioned filename matches the service it names.
    #[test]
    fn a_versioned_file_is_the_service_it_names() {
        assert!(named_as("klogsrv_v0.6.elf", "klogsrv"));
        assert!(named_as("klogsrv", "klogsrv"));
        assert!(!named_as("klogsrv", "ftpsrv"));
    }

    /// pltauth-patch missing from manager autoload.txt is demanded by `doctor::examine`.
    #[test]
    fn pltauth_patch_missing_from_startup_is_demanded_by_doctor() {
        let known = Catalogue::builtin();
        let chain = Chain::parse(
            "!3000\nkstuff-lite_v1.09.elf\n!3000\nnanodns.elf\n!3000\nShadowMountPlus_1.6beta16.elf\n\
             !3000\nps5upload-4.1.2.elf\n!3000\nftpsrv_v0.21.elf\n!3000\nklogsrv_v0.9.elf\n\
             !3000\nshsrv_v0.20.elf\n!3000\nelfldr_v0.24.elf\n",
        );
        let manifest = Manifest::new(vec![]);
        let preset =
            crate::recovery::baseline::named("payload-manager").expect("payload-manager preset");
        let what = Known {
            report: None,
            there: Some(&[]),
            staged: &[],
            described: &manifest,
            chain: Some(&chain),
            kind: Kind::Manager,
            list: Some(crate::chain::PATH),
            preset: &preset,
            known: &known,
        };

        let findings = examine(&what);
        let pltauth = findings
            .iter()
            .find(|f| f.id == "in-list:pltauth-patch")
            .expect("doctor::examine must demand pltauth-patch");
        assert_eq!(pltauth.label, "pltauth-patch comes back after a restart");
        assert_eq!(pltauth.gravity, Gravity::Warning);
    }

    /// A plan that only fetches does not touch the target.
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
