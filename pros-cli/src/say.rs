//! What a person reads.
//!
//! # Why this is the only thing in the shim
//!
//! Every decision this program makes is made in a crate. What is left is how a finding is
//! worded and where the columns line up, and that is the one job a library should not be
//! doing on somebody's behalf - a library that prints has chosen the interface of every
//! tool that uses it.
//!
//! # The rule the wording follows
//!
//! A reader is told **what is possible**, not which ports are open, and when something is
//! wrong they are told **what to do about it** rather than left to work it out from a
//! table. Those are the same two rules the check itself is built on; this is where they
//! become sentences.

use pros_core::check::{Remedy, Report, Verdict};
use pros_core::library::{Item, Kind as LibraryKind};
use pros_core::payloads::{Boot, Presence, Row, Trust};
use pros_link::files::Entry;

/// Prints a check as a table and then as a sentence.
pub(crate) fn report(report: &Report) {
    println!("{} ({})", report.name, report.address);
    for finding in &report.findings {
        let mark = if finding.reachability.open {
            "up  "
        } else if finding.service.required {
            "DOWN"
        } else {
            "--  "
        };
        // A slow answer is said out loud, because a port that refuses instantly and one
        // that takes a second and a half mean different things and look identical in a
        // column of up and down.
        let slow = if finding.was_slow() {
            format!("  ({}ms)", finding.reachability.took.as_millis())
        } else {
            String::new()
        };
        println!(
            "  {mark} {:<9} :{:<5} {}{slow}",
            finding.service.name, finding.service.port, finding.service.unlocks
        );
    }
    println!();
    println!("{}", verdict(&report.verdict()));
}

/// One sentence saying what the check concluded and what to do.
#[must_use]
pub(crate) fn verdict(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Ready => "ready".to_owned(),
        Verdict::Dimmed { names } => format!(
            "usable, but {} {} not loaded, so something will be invisible if a run goes wrong",
            names.join(" and "),
            were(names.len())
        ),
        Verdict::Blocked {
            remedy: Remedy::RerunTheJailbreak,
        } => "the loader is not answering, so nothing can be sent or started from here. \
              This says nothing about the target: a console can run its whole chain with \
              9021 unreachable. Getting it back means loading elfldr through the exploit's \
              own loader, which means re-running the exploit"
            .to_owned(),
        Verdict::Blocked {
            remedy: Remedy::LoadThese { names },
        } => format!(
            "blocked: {} {} not loaded. The loader is up, so {} can be sent again",
            names.join(" and "),
            were(names.len()),
            if names.len() == 1 { "it" } else { "they" }
        ),
    }
}

/// Agreement, because a list of two that says "is" reads as a tool that has never had two.
const fn were(count: usize) -> &'static str {
    if count == 1 { "is" } else { "are" }
}

/// Prints a directory listing.
pub(crate) fn listing(entries: &[Entry]) {
    for entry in entries {
        if entry.is_usable() {
            let size = entry.size.map_or_else(|| "-".to_owned(), |n| n.to_string());
            println!("  {size:>10}  {}", entry.name);
        } else {
            // Shown rather than dropped: a listing that hides the lines it could not read
            // says a directory is emptier than it is.
            println!("  {:>10}  ? {}", "", entry.raw);
        }
    }
}

/// Prints what is described, what can be trusted, and what is on the target.
///
/// `probed` says whether a target was asked. **Without it every row is unknown**, and the
/// difference between *nobody looked* and *it is not there* is the whole reason the presence
/// column has three states rather than two.
pub(crate) fn payloads(rows: &[Row<'_>], probed: bool) {
    for row in rows {
        let presence = match row.presence {
            Presence::Loaded => "on ",
            Presence::NotLoaded => "off",
            Presence::Unknown => "?  ",
        };
        // **A second column, because they are different questions.** A service can be
        // answering now and absent from the boot list, which means it is there until
        // somebody turns the target off - usually the finding that was actually wanted.
        let boot = match row.boot {
            Boot::At(position) => format!("{position:>2}"),
            Boot::NotInList => " -".to_owned(),
            Boot::Unknown => " ?".to_owned(),
        };
        let mark = if row.trust.is_verifiable() { " " } else { "!" };
        let staged = if pros_core::staging::is_staged(row.payload) {
            "here"
        } else {
            "    "
        };
        println!(
            "{presence} {boot} {staged} {mark} {:<16} {:<10} {}",
            row.payload.name,
            row.payload.version.as_deref().unwrap_or("-"),
            row.payload.description.as_deref().unwrap_or("")
        );
    }

    println!();
    println!("columns: running / boot-list position / staged here / verifiable");
    if !probed {
        println!("nothing was asked of a target - add --check for the first two");
    } else if rows.iter().any(|row| row.presence == Presence::Unknown) {
        println!();
        println!("? means no port this project knows, so nothing here can tell - it does");
        println!("  not mean absent");
    }

    let doubtful: Vec<&Row<'_>> = rows
        .iter()
        .filter(|row| !row.trust.is_verifiable())
        .collect();
    if doubtful.is_empty() {
        return;
    }
    // **Said at the end, not left to be discovered one payload at a time.** An entry that
    // cannot be verified is one that cannot be sent, and finding that out half way through
    // a job is finding it out too late.
    println!();
    println!("{} of these cannot be verified:", doubtful.len());
    for row in doubtful {
        if let Trust::Doubtful(why) = &row.trust {
            println!("  {:<16} {why}", row.payload.name);
        }
    }
}

/// Prints what is on the target's storage.
pub(crate) fn library(items: &[&Item]) {
    for item in items {
        let kind = match item.kind {
            LibraryKind::Title => "title",
            LibraryKind::Package => "pkg  ",
            LibraryKind::Folder => "dir  ",
            LibraryKind::File => "file ",
        };
        println!(
            "{kind} {:>12}  {:<12} {}",
            item.size.map_or_else(String::new, size),
            item.id.as_deref().unwrap_or(""),
            item.name
        );
    }

    let (total, counted) = pros_core::library::total_size(
        &items
            .iter()
            .map(|item| (*item).clone())
            .collect::<Vec<Item>>(),
    );
    println!();
    if counted == items.len() {
        println!("{} {}, {}", items.len(), things(items.len()), size(total));
    } else {
        // A total over a listing where some sizes were missing looks complete and is not.
        println!(
            "{} {}, {} over {counted} of them - the rest stated no size",
            items.len(),
            things(items.len()),
            size(total)
        );
    }
}

/// One item is an item. Same reason as `were`: a tool that says "1 items" has never had one.
const fn things(count: usize) -> &'static str {
    if count == 1 { "item" } else { "items" }
}

/// A byte count somebody can read at a glance.
///
/// Integer arithmetic throughout: a size can exceed what a float represents exactly, and a
/// library listing is one of the few places where numbers that large turn up.
fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    const STEP: u64 = 1024;

    let mut amount = bytes;
    let mut remainder = 0;
    let mut unit = 0;
    while amount >= STEP && unit + 1 < UNITS.len() {
        remainder = amount % STEP;
        amount /= STEP;
        unit += 1;
    }
    let name = UNITS.get(unit).copied().unwrap_or("B");
    if unit == 0 {
        format!("{amount} {name}")
    } else {
        format!("{amount}.{} {name}", remainder * 10 / STEP)
    }
}

/// Prints what a folder copy did, and what it did not.
///
/// **The incomplete case is not a footnote.** A backup that quietly missed a file will be
/// trusted at the moment it matters, so a copy with anything skipped says so first, lists
/// every one, and exits non-zero.
pub(crate) fn copied(
    summary: &pros_core::transfer::Summary,
    where_to: &str,
) -> std::process::ExitCode {
    println!(
        "{} {}, {} -> {where_to}",
        summary.files,
        if summary.files == 1 { "file" } else { "files" },
        size(summary.bytes)
    );
    if summary.is_complete() {
        return std::process::ExitCode::SUCCESS;
    }
    println!();
    println!(
        "{} {} NOT copied:",
        summary.skipped.len(),
        things(summary.skipped.len())
    );
    for skipped in &summary.skipped {
        println!("  {}", skipped.path);
        println!("      {}", skipped.why);
    }
    println!();
    println!("this copy is incomplete - do not treat it as a backup");
    std::process::ExitCode::FAILURE
}
