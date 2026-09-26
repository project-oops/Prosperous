# D019 - A write to a target edits the text it read

**Status:** decided
**Date:** 2026-09-26

A settings change applies an edit to the text read off the target and sends that text back. The
change is shown line by line before it is made, and setting a value to what it already is produces
no change at all.

**Why:** the payload manager's settings decide what loads at startup, and a wrong file leaves a
target without the services this tool needs to repair it. Comments, order and unknown keys survive
because they are never taken apart. Confirming exact lines catches a tool about to do more than
asked, and a confirm for a no-op teaches clicking through.

**Rejected:**
- Regenerating the file from a parsed model: loses what the model does not understand.
- Confirming a description of the change: does not show what is written.
