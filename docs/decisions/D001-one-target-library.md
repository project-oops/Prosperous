# D001 - One target library for the collection

**Status:** decided
**Date:** 2026-09-26

Prosperous is the one library that reaches a target, taken by the sibling projects as relative
path dependencies. `pros-cli` and `pros-gui` are interaction surfaces over its crates and hold no
rules of their own.

**Why:** two projects had begun building the same transport, and two copies drift. A command
line and a window that apply the same rules from one place cannot disagree about a target.

**Rejected:**
- A transport copy inside each consumer: the copies drift and each fixes its defects alone.
- Publishing the crates and depending on versions: trades a checkout convention for a release
  process.
- Rules in the shims: a second front end then needs a second implementation.
