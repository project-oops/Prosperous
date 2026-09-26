# D002 - A refusal is not a failure

**Status:** decided
**Date:** 2026-09-26

`pros-link` reports a target that understood and said no as `Error::Rejected`, and a target
that answered in a shape this crate cannot read as `Error::Unintelligible`, carrying what was
said. Neither is a broken link.

**Why:** the two call for opposite next actions. A rejection is usually the operator's path and
nothing is broken; an unreadable answer is usually this crate being wrong about the server, and
what was said is the only useful thing to report.

**Rejected:**
- Folding both into a connection failure: a caller then reconnects to fix a typo.
- A finer set of variants: distinctions that do not change what the reader does next.
