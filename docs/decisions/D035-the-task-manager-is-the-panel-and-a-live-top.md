# D035 - The task manager is the panel that already exists, plus a live `pros top`


**decided** · 2026-09-22 · principles 2, 3 and 6

The ask was a task manager built on the existing `ps`/`kill` logic. But the logic - find a process,
signal it, list what is running - was already in `pros_core::system`, and the window already had a
system panel that lists every process with a **close** button on each title, an **end**-by-pid
button on everything else, and a **restart UI** button: a task manager in all but name. So this did
not add a new thing beside the ones that exist. It filled the two gaps in the views that already
have the capability.

**Memory, which `ps` measured and the parser threw away.** The measured `ps` prints a
`Memory (MiB)` column as `current / peak`, and `processes()` read past it - keeping only pid,
state, title and command. It is now on `Process`, read *from the end* of the row rather than by
column index, because the title column is present for a game and blank for everything else, which
shifts every fixed position - but the command is always last and the memory always the three tokens
before it. It shows in `pros ps`, in `pros top`, and on each GUI row with the peak on hover.
**CPU is not shown, because the measured `ps` has no CPU column.** Inventing one is the thing
principle 2 refuses; memory is the only resource figure this platform hands over, so it is the only
one reported.

**`pros top` is the live command-line form, and it is CLI-first like `logs` and `probe`.** The same
table `ps` prints, re-read on an interval until Ctrl-C or a `--seconds` cap - a shim tie-together
over pieces that live in `pros-core`, the same shape D033 settled for `probe`. It is **not
interactive**: it redraws and reads, but ending a process is still `pros close` / `pros kill`, not a
keypress. That is deliberate - an interactive kill needs terminal raw-mode handling and a dependency
the command line does not carry, and a monitor beside the two verbs that already end things covers
the need. On a terminal it clears between draws; piped, it prints successive tables so a capture
stays readable. Interactive keys are a later request, not a gap left by accident.

**The window panel got the same three, no more.** The memory column; a sort chooser (as listed, by
memory, by state) applied *within* the titles section and the everything-else section, so the
titles-first grouping a person is usually looking for survives the sort; and an opt-in auto-refresh
that re-reads every few seconds. Auto-refresh is **off by default and guarded** - a timestamp stops
it becoming a request every frame, and it is gated on the worker being idle so it never stacks a
read on a running one - because a round trip to the target is not something to do unasked.

**One printer behind both `ps` and `top`.** `say::processes` formats the table for both, so a column
shown by one and not the other cannot happen - the drift between two front ends principle 3 exists
to prevent, in miniature. A separate GUI "task manager" was not built, because it would duplicate
the panel; the shared thing is the logic in `pros_core::system`, which all of `ps`, `kill`, `close`,
`top` and the window now read.
