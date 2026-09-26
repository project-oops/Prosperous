# D028 - The `param.sfo` reader comes from SELFish, the in-place writer stays

**Status:** decided
**Date:** 2026-09-26

`param.sfo` is read through `selfish_title::sfo::Sfo`. `pros_core::sfo` keeps only what Prosperous
adds: the account id rendered as hex, and `set`, which overwrites one value in place within the room
the file already has.

**Why:** platform formats come from SELFish, and a second layout table drifts. `graft` depends on
editing a save rather than rebuilding it, because a reassembled file still parses and is refused
later by the target with nothing pointing at the wrong byte.

**Rejected:**
- A parser of Prosperous's own: a duplicate of a platform format.
- `Sfo::to_bytes` for writes: re-serialises the whole file.
