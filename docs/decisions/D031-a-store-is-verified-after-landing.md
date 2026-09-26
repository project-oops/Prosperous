# D031 - A store is verified after it lands

**Status:** decided
**Date:** 2026-09-26

After each store, `transfer::upload` reads the file's size back. A plain file counts as copied only
when the size matches what was sent. A SELF container, recognised by
`selfish_abi::Generation::from_container_magic`, counts when a size comes back at all, because the
target reports the unwrapped payload's size. Anything else is recorded as not copied.

**Why:** a target with the title mounted, or an overlay that swallows the write, acknowledges the
store and keeps the old file. A size read is one protocol line per file. The target unwraps a
container on access, so its size can only confirm presence.

**Rejected:**
- Trusting the completion reply: the fault this exists to catch.
- Reading the whole file back: a large cost on every file for a rarer same-size fault.
- Computing the unwrapped size here: reimplements a format that belongs to SELFish.
