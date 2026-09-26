# D013 - Target tests are opt-in and read-only

**Status:** decided
**Date:** 2026-09-26

Tests against a real target are `#[ignore]` with a reason, run only with `--ignored` and
`PROS_TARGET` set, and fail when asked for without an address. They never write to the target.

**Why:** an ordinary run shows them as ignored rather than claiming evidence a stand-in cannot
produce. A suite asked for by name that does nothing must not report success. A suite that can
alter the machine it measures has ambiguous failures.

**Rejected:**
- Skipping silently when no address is set: a pass that checked nothing.
- A feature flag: hides the tests from the ordinary report.
