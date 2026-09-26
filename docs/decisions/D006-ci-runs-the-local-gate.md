# D006 - Continuous integration runs the local gate

**Status:** decided
**Date:** 2026-09-26

The pipeline installs a toolchain, fetches the siblings and runs the same check a person runs
before pushing. It has no list of steps of its own, and its checkout is full, not shallow.

**Why:** a pipeline that runs something else lets a green build and a broken working copy
disagree. The provenance step refuses to pass without a repository to ask, so a shallow
checkout would fail it.

**Rejected:**
- Separate CI steps: a second list of checks that drifts from the first.
- A shallow checkout: faster, and leaves the provenance step nothing to read.
