# D030 - The guard routes a location and never rewrites an identity

**Status:** decided
**Date:** 2026-09-26

`guard::check` refuses a destination the target will not scan (`IssueKind::InertPath`) and offers
the same title under `/data/homebrew`. Any other destination is accepted whatever the title's
prefix, and `suggested_id` is always the id that came in.

**Why:** the prefix rule was reasoned, not measured, and rewriting an id sent one title on top of
an unrelated installed one. A title under the homebrew folder with its own id is where it belongs.

**Rejected:**
- A list of supported prefixes: unmeasured, and it refused correct destinations.
- Rewriting the id to a supported prefix: a silent overwrite of another title.
