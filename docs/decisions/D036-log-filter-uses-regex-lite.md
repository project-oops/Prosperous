# D036 - The log filter uses `regex-lite`

**Status:** decided
**Date:** 2026-09-26

The log filter is a plain substring by default and a regular expression behind a checkbox, through
`regex-lite`. A pattern that does not compile shows every line and says *invalid regex*.

**Why:** a few thousand short lines need no throughput engine, and `regex-lite` has no dependencies
of its own. Blanking the log on each keystroke of a half-typed pattern hides the log while it is
being filtered.

**Rejected:**
- `regex`: four more crates for speed this does not need.
- Showing nothing for an invalid pattern: reads as a filter that matched nothing.
