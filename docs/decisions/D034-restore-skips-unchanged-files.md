# D034 - A restore skips unchanged files on its own record

**Status:** decided
**Date:** 2026-09-26

After a store verifies (D031), the digest of the bytes sent is recorded in `deployed.json`, keyed by
target and remote path. A restore skips a file only when its local digest matches the record and a
directory listing still shows it on the target; a failed store forgets the record, and `--all` sends
everything.

**Why:** re-sending an unchanged tree costs minutes per deploy. The target unwraps SELF containers
on access, so a remote hash or size cannot be compared with what was sent. A listing names the file
unchanged by the unwrap and costs one round trip per folder. Every error lands on re-sending.

**Rejected:**
- Comparing against the target's hash or size: disagrees for every container.
- Presence by `SIZE`: answered for a file that had been deleted.
- Always sending everything: the cost this removes.
