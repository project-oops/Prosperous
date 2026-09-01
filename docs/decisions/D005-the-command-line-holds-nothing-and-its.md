# D005 - The command line holds nothing, and its exit codes are part of what it holds


**decided** - 2026-08-26 - building `pros-cli`

`pros` is eleven subcommands over two crates and it contains no decision of its own. Every
rule it applies - what a registration is, what a missing loader means, which digests can be
trusted, which files a loader will accept - lives in a crate, because a graphical version has
to apply exactly the same rules and a second implementation of them would drift inside a
month. This is the sibling projects' shim rule taken as a starting condition rather than
arrived at after the first drift.

What is left in the shim is argument parsing and `say.rs`: how a finding is worded and where
the columns line up. **That is the one job a library should not do on somebody's behalf** - a
library that prints has chosen the interface of every tool that uses it. It is also why
`target::register` returns the path it wrote instead of printing it, which is the one place
the reference implementation had it the other way round.

### Three exit codes, not two

- **0** - it worked, or the target answered and the answer was *not ready*
- **1** - this program could not do what it was asked
- **2** - a check found the target blocked

The reference implementation returns success for a blocked target, reasoning that *the
target is off* is an answer rather than a malfunction. That reasoning is right and the
conclusion does not follow: a script branching on it then has to tell a blocked target from
a broken tool by reading the message. **Separating them is what makes the answer usable
without conflating it with a failure**, which was the point of not returning failure in the
first place.

Written into the module documentation, because an exit code that is not documented is a
number somebody has to discover by experiment.

### One real defect, found by running it

`pros send` printed *sending 64 bytes to 127.0.0.1* and then refused the file, because the
announcement came before the guard. **The tool described an action that never happened** -
the same defect as a check that cannot fail, wearing different clothes, in a program whose
whole subject is that class of mistake.

The shape check now runs in the shim as well as in the library. That is not a duplicated
guard: the library's is the real one, and this one exists so that nothing is announced which
is not going to happen.

### Proved end to end, against stand-ins rather than a target

In an isolated home directory so no real registration was touched, against two Python
stand-ins - deliberately not the crate's own fake:

- `check` with nothing listening: every service down, `re-run the jailbreak`, **exit 2**
- `check` with the chain up but the dashboard closed: `usable, but pldmgr is not loaded`,
  exit 0, and the 1515 ms refusal reported as a timing
- `logs` ended on its own clock; `sh` came back without the banner
- `send` of a vendor module refused with nothing announced, **exit 1**; a payload sent
- `push` then `pull` of the same file: **identical after a round trip**
- `manifest` marked two of three entries untrustworthy and said why for each
- `verify` passed the right file, and refused both the wrong file and an entry whose digest
  cannot be checked, exit 1 for both

### Still not here

Fetching a payload from a public mirror. `pros check` cannot yet repair what it finds, which
is the half of that command worth having, and it stays undone until the dependency that makes
it possible is argued for rather than acquired in passing. See D004.

