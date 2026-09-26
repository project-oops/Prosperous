# D014 - Downloads are verified before they are staged

**Status:** decided
**Date:** 2026-09-26

A fetch refuses an entry with no digest this can check before downloading, downloads into
`incoming/`, verifies, and only then moves the file into the staging directory. A file that fails
is removed. `pros stage` checks a local file the same way on the way in.

**Why:** the staging directory promises that everything in it was verified, and an unchecked file
sitting there for the length of a download breaks that promise. A download nobody can verify looks
exactly like one that worked.

**Rejected:**
- Downloading straight into staging: an unchecked file is present while it transfers.
- Verifying at send time: the directory would hold files of unknown standing.
- Keeping a failed download: a wrong file left looking like a payload.
