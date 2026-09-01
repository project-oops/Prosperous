
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
