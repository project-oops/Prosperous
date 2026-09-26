# D015 - Names come from the target

**Status:** decided
**Date:** 2026-09-26

A title's name is read from `/user/appmeta/<id>/param.json` on the target, in any language it
carries, trusting the file's own `titleId` over its folder. A title with no name is shown as its
identifier. A save path descends through a user folder only when there is exactly one.

**Why:** the mapping lives on the target and nowhere else. Any language is better than an
identifier, and an empty name looks like a title called nothing. Picking one of several users
would be picking whose saves are about to be overwritten.

**Rejected:**
- A shipped name table: goes stale and covers only what somebody wrote down.
- Choosing a default user: an unasked choice with data at stake.
