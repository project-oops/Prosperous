# D006 - Continuous integration runs the one dev command, and nothing else


**decided** - 2026-08-26 - adding the workflow

The pipeline installs a toolchain and runs `bash prosperous.sh check`. It does not have its
own list of steps.

**A pipeline that runs something else is how a green build and a broken working copy come to
disagree.** Whoever changes the checks changes them in one place, and the thing a person runs
before pushing is the thing that gates the push. The cost is that CI cannot do anything the
script cannot, which is the point rather than a limitation.

The checkout is deliberately not shallow. The provenance step refuses to pass when it cannot
look - that was fixed once already, after a version that swallowed the failure and reported
success having checked nothing - so anything that leaves it without a repository becomes a
failure rather than a silent pass. A full checkout removes the question entirely.

### What this does not fix

**obSCEne's pipeline still cannot build its tool**, because it checks out one repository and
the path dependency added in that project's D189 points outside it. That needs a second
checkout step and a remote that does not exist yet. Recorded here as well as there, because
the two projects are now checked out as a set and a reader of either should find it.

