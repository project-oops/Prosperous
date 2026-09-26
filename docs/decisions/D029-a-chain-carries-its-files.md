# D029 - A chain carries its files, and which files is data

**Status:** decided
**Date:** 2026-09-26

A chain carries captured `{path, content}` files that deploying puts back verbatim, after the
payload list and before the autoload switch is guaranteed on. Which paths are captured is declared
in the `capture` block of `chain.json`, and shipped chains carry no content.

**Why:** a payload order alone loses the manager's settings, so a deployed chain behaved
differently. The files belong to another tool and their names move, so the program copies bytes
rather than parsing them, and the list of paths is edited rather than rebuilt. Shipping one
target's settings would impose its habits on every target.

**Rejected:**
- Parsing the captured settings: breaks when the other tool changes them.
- Paths in code: a list that grows needs a release for each addition.
- Letting captured settings turn autoload off: a deployed list nobody reads.
