# D022 - Prosperous sends and supervises a probe, and does not drive it

**Status:** decided
**Date:** 2026-09-26

`pros supervise` keeps a conformance probe running: it re-sends the same bytes through the loader
when the port stops answering, never sends while the probe answers, and gives up after a bounded
run of starts that never answer. Speaking the probe's protocol stays with its existing driver.

**Why:** faulting is the probe's normal case and its protocol leaves restarting to a supervisor.
Prosperous already reaches the target, so sender and supervisor in one program make the restart
unattended. A second protocol client would be another place to disagree about what `died` means.

**Rejected:**
- A protocol client here: permitted by the published specification, wanted by nobody.
- A cap on total restarts: stops the useful case of faults with working sessions between them.
