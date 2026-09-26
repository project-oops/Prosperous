# D004 - Input that cannot be verified is refused by name

**Status:** decided
**Date:** 2026-09-26

A manifest entry whose digest algorithm this cannot check is an error that names what it found,
and a document in an unrecognised shape is reported as such, never read as an empty repository.

**Why:** a payload is fetched from a mirror and then run on the target, so a check that passes
over what it does not recognise reports success for an entry it never checked. An unreadable
file read as empty looks like a fact about the target and would be believed.

**Rejected:**
- Verifying what is recognised and skipping the rest: a silent pass nobody learns about.
- Treating an unknown document shape as no payloads: indistinguishable from a real empty list.
