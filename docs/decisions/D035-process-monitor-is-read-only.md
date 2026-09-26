# D035 - The process monitor reads and does not act

**Status:** decided
**Date:** 2026-09-26

`pros top` redraws the `pros ps` table on an interval and takes no keys; ending a process stays with
`pros close` and `pros kill`. Memory is the only resource column, and the window's system panel is
the task manager, with auto-refresh off by default.

**Why:** an interactive kill needs terminal raw mode and a dependency the command line does not
carry. The target's `ps` reports memory and no CPU, and a figure it does not report is not
invented. A round trip to the target is not made unasked.

**Rejected:**
- Interactive keys in `top`: a dependency for what two verbs already do.
- A CPU column: nothing measures it.
- A separate task manager window: duplicates the system panel.
