# D033 - The probe loop ends on park, exit or a cap

**Status:** decided
**Date:** 2026-09-26

The probe loop (`pros_core::probe`, used by `pros probe` and the window) closes the title, restores
the build, waits for the title to register, attaches to the log, launches, and follows until the
park sentinel, the process leaving the list, or a time cap. A title never seen is reported as its
own case.

**Why:** a finished probe parks rather than exiting, so waiting for exit alone would never end. A
probe does its work in the first second or two, so a follower attached after the launch captures
nothing. A launch before registration fails, and the filesystem reports the restore immediately.

**Rejected:**
- Ending only on process exit: a parked title never exits.
- Attaching the log after the launch: loses the output the loop exists to capture.
- Launching straight after the restore: fires into the registration gap.
