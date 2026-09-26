# D018 - A save transfer decides before it moves anything

**Status:** decided
**Date:** 2026-09-26

Before a save goes to a target, `origin` decides whether it belongs to that target's account: from
the save's own `ACCOUNT_ID`, then from a record written when this tool made the copy, and otherwise
`Unknown`. A mismatch or `Unknown` is a refusal naming both accounts; *copy anyway* is offered and
never the default.

**Why:** a copy to the wrong account completes without error and fails later, when the target
refuses the save. Not every save carries a parameter file, and a save with no provenance is the one
most likely to have come from elsewhere.

**Rejected:**
- The save's parameter file alone: many saves have none.
- Treating `Unknown` as a plain copy: puts the failure where it does most damage.
- Refusing without an override: the operator may know something this does not.
