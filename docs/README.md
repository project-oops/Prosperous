# Prosperous documentation

Remote management for anything that runs Orbis software - a prepared machine, or orbistoun.
Register it, ask what it can currently do, put a payload on it, read its log, move files,
watch its output.

New here? The [root README](../README.md) has the pitch. Then, depending on what you came for:

| | |
|---|---|
| **using it** | [Getting started](guide/getting-started.md) |
| **building it** | [BUILDING.md](BUILDING.md) |
| **understanding it** | [DESIGN.md](DESIGN.md) |


## The words

- [GLOSSARY.md](GLOSSARY.md) - the services on the target, chains against health checks, scan
  roots, portable mode and Porthole. The collection's glossary covers standard ELF and the
  words that mean something else in the sibling repositories.

## Guide

- **[Getting started](guide/getting-started.md)** - from a download to a target that answers,
  and how to read the answer. Assumes nothing, including that you know what the five services
  are.
- **[Targets](features/targets.md)** - registering one, port overrides, and what a
  registration does and does not remember.
- **[Payloads](features/payloads.md)** - sending one, watching it, and reading the log when it
  dies. Also why none are bundled.
- **[The target's storage](features/library.md)** - titles, saves and packages, and the file
  operations in both directions.
- **[BUILDING.md](BUILDING.md)** - `bin/prosperous`, what each verb does, what `check` runs
  and in what order, and what CI runs. You do not need this to *use* Prosperous; the releases
  page has binaries.

## Reference

- [USAGE.md](USAGE.md) - what `pros` reports and how to drive it, for somebody already
  running the tool. This was the middle of the README.
- [DESIGN.md](DESIGN.md) - what the tool is and why it is shaped this way: one instrument,
  two transports, and the library underneath both.
- [CAPABILITIES.md](CAPABILITIES.md) - a design note on being worth running once every payload
  it currently names has been superseded. Steps 1 and 2 are built; the rest is design.
- [VIDEO.md](VIDEO.md) - watching and diffing: two problems that look like one and share no
  code.
- [PAD.md](PAD.md) - a proposal, not a decision, about where the pad should live. Nothing in
  it is built.

## Project memory

- [DECISIONS.md](DECISIONS.md) - a generated index over `decisions/`, one file per
  entry. Every non-obvious choice, numbered, with the reasoning,
  including the ones reversed on evidence.
- [WORKLOG.md](WORKLOG.md) - what was done, in order, with the surprises.
- [ROADMAP.md](ROADMAP.md) - what is wrong, what is built and not reachable, what is missing,
  and what is next.

**These are dated records.** What they say was true when it was written and is not corrected
afterwards. Correcting a log is falsifying it.

Shared rules - provenance, naming, decision logs, gates - are in
[the OOPS conventions](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md) and
not restated here.

## Adding to a log

The long-running documents are **directories with a generated index**. Add a file under
`decisions/`, `backlog/` or `worklog/`, then regenerate the table:

```bash
tools/split-decisions.sh --index prosperous
tools/split-doc.sh --index prosperous BACKLOG 2 backlog
```

Do not edit the index by hand - it is overwritten. The split exists because two sessions
appending to one file collide, which is where the duplicate numbers and out-of-order entries
came from, and because a log past half a megabyte stops rendering on GitHub entirely.
