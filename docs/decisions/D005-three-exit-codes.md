# D005 - Three exit codes

**Status:** decided
**Date:** 2026-09-26

`pros` exits 0 when it worked or the target answered *not ready*, 1 when it could not do what it
was asked, and 2 when a check found the target blocked.

**Why:** a blocked target is an answer, not a malfunction, but a script has to tell it from a
broken tool without reading the message.

**Rejected:**
- Success for a blocked target: a script cannot branch on it.
- Failure for a blocked target: conflates the answer with the tool breaking.
