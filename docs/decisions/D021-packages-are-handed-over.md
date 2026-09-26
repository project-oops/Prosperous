# D021 - Packages are handed over, and an unclear result stays unclear

**Status:** assumed
**Date:** 2026-09-26

`pkg_install` is given an HTTP url served by `handover`, a one-file listener bound to the interface
that reaches the target. The known failure reply is a failure, silence is reported as silence, and
any other output is `Said::Unclear` carrying the target's words. A path with a space is refused.

**Why:** a path on the target's own disk produced nothing, and nothing on the target serves files.
What a successful install prints has not been observed, so calling other output success would say
the same thing when it was wrong. The shell splits on spaces and has no quoting.

**Rejected:**
- Installing from a path on the target: the one form tried produced nothing.
- A general file server: path handling to get wrong for a one-file transfer.
- Reporting unrecognised output as installed: right most of the time, identical when wrong.
