# D026 - The window ships its own manual

**Status:** decided
**Date:** 2026-09-26

`help -> documentation...` opens the user guide embedded in the binary by `include_str!` and read
through `oops-docs`. The list of pages lives in `pros-gui`; the reader is shared.

**Why:** embedded pages always match the build being run and need no network, which is often
absent where this tool is used. `include_str!` resolves relative to the file it is written in, so
only this repository can embed its pages.

**Rejected:**
- Linking to pages online: needs a network and can describe a different version.
- Pages shipped as files beside the binary: a second thing to keep in step with it.
