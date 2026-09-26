# D027 - Process control is Prosperous work

**Status:** decided
**Date:** 2026-09-26

Finding, signalling and ending target processes lives in `pros_core::system` as pure builders and
selectors (`shell_ui`, `of_title`, `kill`, `end`), with the shell call left to the shim. `end` wakes
a stopped process before killing it. `pros restart-ui` and `pros close` expose it, and consumers call
it rather than keeping a copy.

**Why:** which process is the target's UI and how to end a title cleanly is target-management
knowledge, and Prosperous is the target-management product. Policy as data is tested against a
fixture without a target.

**Rejected:**
- Keeping it in obSCEne's tool: the knowledge lives where it was first needed, not where it belongs.
- `oops-libs`: its admission rule bars domain code.
