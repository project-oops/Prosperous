# D010 - Presence and boot membership can be unknown

**Status:** decided
**Date:** 2026-09-26

A payload's presence is `Presence::Unknown` when nothing here can measure it (no known or declared
port), and a boot list that could not be read is unknown, never *not in it*. Running now and
listed for the next boot are separate columns.

**Why:** reporting an unmeasurable payload as not loaded invents a measurement and puts it in the
same column as the real ones. A service can be running and absent from the boot list, which is
often the finding that matters.

**Rejected:**
- Two states, loaded and not loaded: an unmeasured row reads as a measured absence.
- One column for now and after a reboot: the two answers look the same and mean different things.
