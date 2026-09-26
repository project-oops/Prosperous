# D017 - Shipped lists carry only verifiable entries

**Status:** decided
**Date:** 2026-09-26

No shipped list carries a url without a digest this can check. Titles lists only homebrew, each
entry from its own publisher; cheat urls are pinned to a commit; the saves list ships empty. Tests
pin each rule.

**Why:** a url with no digest invites an unverifiable download. A list of urls for commercial
titles is a piracy index. A branch url changes under its digest, and a stale digest reads as a
corrupted download. A save is signed for the target that wrote it, so a downloaded one is rejected.

**Rejected:**
- Entries with a url and no digest: the one combination that cannot be checked.
- Branch urls for cheats: the digest goes stale on the next push.
- A populated saves list: every entry would be a file the target refuses.
