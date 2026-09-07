
## A quiet socket ended the watch on Windows, and the pump now knows both names for a timeout

Reviewing the Porthole wire contract against `pros-core::watch` found the pump reading with a
half-second timeout and treating only `WouldBlock` as the pause between frames. That is the
name Unix gives a timed-out read. Windows gives it `TimedOut`, so on Windows the first
half-second with nothing arriving ended the stream - and said so with a line about the
connected party failing to respond, which reads as a fault on the target's side. The
transport's own log reader had already learned this (`wire::is_quiet` matches both names); the
pump had not.

Two tests pin it. The unit test feeds a source that answers every read with one name and then
the other, with no socket, and requires the pump to go round rather than settle. The standin
test opens a silent fake with a short timeout and lets the platform choose the name, which is
the reading that failed here before.

**The surprise worth keeping:** nothing in the suite had ever exercised the timeout arm. Every
socket test read with a five-second timeout against a fake that answered within milliseconds,
and the one test that did wait on silence read the socket directly and matched both names in
its own assertion - the knowledge was in the test file and not in the code beside it.

The same review found the target side of the same seam blocking on its input socket, so that
the video stalled whenever a pad was at rest; that is fixed in Porthole, in oops-apps, and
recorded there.

## Process control moved in from obSCEne, first-class in both programs

Restarting the user interface and closing a title were built inside `obscene-tool`, each
hand-parsing `ps`/`procstat` and encoding the signal policy inline. The transport was never
the duplication - both already ran over `pros_link::shell` - but the *capability* had leaked
into a consumer: which process is the interface, that the system respawns it, that a stopped
title needs a wake signal before a kill or it leaves locked vnodes behind. That is
target-management knowledge, so it came here. (D027)

It sits in `pros_core::system` the way `launch` does: pure builders and selectors - `shell_ui`,
`of_title`, `kill`, `end`, a `Signal` enum - with the effect left to the shim. `end` is where
the wake-then-kill order lives, as data rather than a branch at a call site, so it is tested
against a `ps` fixture with no target in the room.

First-class in the CLI as `pros restart-ui` and `pros close <id>`, and in the window as a
*restart UI* button and a per-title *close*, both leaving the process list showing what is
running now. obSCEne's two functions became thin calls into `pros-core`, so the recipe it used
to carry is gone and the knowledge has one home.

**The surprise worth keeping:** obSCEne found the interface with `procstat -a`; this uses `ps`,
because `ps` is what `system::processes` already parses. `ps` lists every process so the UI is
in it, but that its command column reads exactly `SceShellUI` under `ps` is an expectation, not
a measured fact - flagged in D027 rather than asserted, and the one thing a hardware run should
confirm.
## contents(), which was made testable and then not tested

8 tests in `crates/pros-core/tests/walking.rs`. `transfer.rs` went from **71.60% to 80.17%**
of regions covered.

`transfer::contents` carries a doc comment saying it is *"separated from the sending so it can
be tested, and so a caller can show what is about to go before any of it does"* - and nothing
tested it. A seam introduced for testability and left untested is the cost of the seam without
the benefit.

It is also the half of a restore that decides what a restore *is*. `upload` takes a live
session and cannot be exercised without a target; `contents` decides the file list that session
is handed, so a folder missed here is a file that never goes back.

What is pinned: paths come back relative (an absolute one would carry this machine's directory
onto the target), the order is sorted rather than the filesystem's, directories are walked
rather than listed, an unreadable folder is an error and **not** an empty list, and the depth
bound stops a deep tree while still reporting what was inside it. The last one is asserted at
the boundary in both directions - twelve levels found, thirty not - because an off-by-one
there either loses a legitimate file or walks a level further than the rule says, and neither
shows up on a shallow folder.

`upload`, `configured` and `temporary` stay uncovered: they need a live session or the user's
own config directory, and a test that reached for either would be testing this machine.

## Why `run` was greyed on every payload, and the two folders behind it

Reported from the window: a payload ticked in the payloads pane, and `run` refused. The refusal
itself was correct - `Offer::Run` sends bytes *from here*, and `Entry::here` was `None` - but
four things around it made that unreadable, and one of them was a second directory nobody had
noticed was second.

**`open folder` opened a different folder from the one being judged.** It revealed
`manifest::staging()`, which is `cache_directory()/payloads`, under a comment stating that is
"where the payloads table sends from". It is not, and had not been for a while: the row action
sends from `local_path`, which is `data_root()/payloads`, and so does the toolbar, and so is
where a download is written. Somebody opening the folder to find out why `run` was greyed was
shown a directory that has no bearing on the answer. Now it opens the one the pane uses.

**The table had twelve cells per row and eleven headings.** There is no `"run"` heading, so
every label sat one column left of its data - `name` over the run buttons, `running` over the
sizes, `version` over the on/off marks. It reads as a table that is simply wrong about itself,
and it is invisible until somebody compares a value with the word above it.

**An empty local folder presented as thirty dead buttons.** `library::here` reports a folder
that does not exist as an empty one, which is right - a machine where nothing has been
downloaded is not a failure - but nothing said so, and the only explanation was on the hover of
a control that looked broken. It now says it once, above the table.

**`run a file...` opened wherever the last dialog had been.** It passes `local_path` to `rfd`,
which ignores a directory that is not there - and on a first run that directory does not exist,
because nothing has created it yet. Made before the dialog opens.

The surprise worth keeping: **none of these was the refusal being wrong.** Every one was
something around it disagreeing about which folder the question was about, and together they
made a correct refusal look like a broken button. Two names for one directory is enough to do
that on its own; a comment asserting the wrong one of the two is what made it survive.
