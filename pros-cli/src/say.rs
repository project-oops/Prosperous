//! What a person reads: the wording and column layout of every report.
//!
//! Decisions are made in the library crates; printing lives here because a library that
//! prints chooses the interface of every tool that uses it. A reader is told what is
//! possible rather than which ports are open, and when something is wrong, what to do
//! about it.

use pros_core::check::Report;
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
        // A slow answer is shown: an instant refusal and a slow one mean different things
        // and look identical in a column of up and down.
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
    println!("{}", report.verdict());
}

/// Prints a directory listing.
pub(crate) fn listing(entries: &[Entry]) {
    for entry in entries {
        if entry.is_usable() {
            let size = entry.size.map_or_else(|| "-".to_owned(), |n| n.to_string());
            println!("  {size:>10}  {}", entry.name);
        } else {
            // Shown rather than dropped, so the listing is not emptier than the directory.
            println!("  {:>10}  ? {}", "", entry.raw);
        }
    }
}

/// Prints what is described, what can be trusted, and what is on the target.
///
/// `probed` says whether a target was asked. Without it every row is unknown: the presence
/// column has three states so that "nobody looked" differs from "it is not there".
pub(crate) fn payloads(rows: &[Row<'_>], probed: bool) {
    for row in rows {
        let presence = match row.presence {
            Presence::Loaded => "on ",
            Presence::NotLoaded => "off",
            Presence::Unknown => "?  ",
        };
        // A separate column: a service can be answering now and absent from the boot list,
        // so it is gone after the next reboot.
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
    // Listed together at the end: an entry that cannot be verified cannot be sent.
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
        // Some sizes are missing, so the total says what it covers.
        println!(
            "{} {}, {} over {counted} of them - the rest stated no size",
            items.len(),
            things(items.len()),
            size(total)
        );
    }
}

/// The noun agreeing with a count of items.
const fn things(count: usize) -> &'static str {
    if count == 1 { "item" } else { "items" }
}

/// A byte count somebody can read at a glance.
///
/// Integer arithmetic throughout: a library size can exceed what a float represents exactly.
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
/// A copy with anything skipped says so, lists every skipped file and exits non-zero, so an
/// incomplete copy is never trusted as a backup.
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
    // Files already on the target unchanged are neither copied nor skipped; they are counted so
    // a restore that moved little does not read as one that did nothing.
    if summary.unchanged > 0 {
        println!(
            "{} {} already there, unchanged - not re-sent",
            summary.unchanged,
            things(summary.unchanged)
        );
    }
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

/// Prints a process listing as a table: pid, state, memory, title, command.
///
/// Shared by `ps` and `top` so their columns cannot drift. Memory is the current figure in MiB;
/// a row with no memory figure shows `-` rather than an unmeasured zero. Columns are sized to
/// the widest value present.
pub(crate) fn processes(processes: &[pros_core::system::Process]) {
    let pid_w = processes
        .iter()
        .map(|p| p.pid.len())
        .max()
        .unwrap_or(3)
        .max(3);
    let mem_w = processes
        .iter()
        .map(|p| p.memory.as_ref().map_or(1, |m| m.current.len()))
        .max()
        .unwrap_or(3)
        .max(3);
    println!(
        "{:>pid_w$}  {:6}  {:>mem_w$}  {:9}  COMMAND",
        "PID", "STATE", "MEM", "TITLE"
    );
    for one in processes {
        let mem = one.memory.as_ref().map_or("-", |m| m.current.as_str());
        let title = if one.title.is_empty() {
            "-"
        } else {
            &one.title
        };
        println!(
            "{:>pid_w$}  {:6}  {:>mem_w$}  {:9}  {}",
            one.pid, one.state, mem, title, one.command
        );
    }
}
