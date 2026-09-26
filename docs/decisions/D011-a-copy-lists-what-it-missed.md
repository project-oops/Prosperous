# D011 - A copy lists everything it did not copy

**Status:** decided
**Date:** 2026-09-26

A folder copy walks past an unreadable file, does not follow links, stops at a depth bound, and
records every skipped item in its summary. An incomplete summary exits non-zero and says the copy
is not a backup.

**Why:** a backup that quietly missed a file is trusted at the moment it matters. A link can point
at its own parent, and the protocol offers no identity to detect the loop.

**Rejected:**
- Stopping at the first failure: saves nothing, usually over the least important file.
- Following links: can recurse until the disk fills.
- Reporting only a count: the reader needs to know which files.
