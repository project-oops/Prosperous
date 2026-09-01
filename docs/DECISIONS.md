# Decisions

Numbered, append-only, cited from source by number. A decision here is a choice that
constrained something afterwards - not a summary of what the code does, which the code
already says.

The same discipline as the sibling projects: if a citation in a comment points at a number,
this file explains it, and a number that duplicates another makes every citation of it
ambiguous.

**D016-D024 carry no date.** Dating stopped after D015 (2026-08-26); those entries were made
on or after that day and nothing narrower is recoverable. Entries after them carry one.

**This table is generated.** Edit an entry under `decisions/`, then run
`tools/split-decisions.sh --index prosperous`. A number resolves to exactly one file.

| | # | decision | status | date |
|---|---|---|---|---|
| 🟢 | D001 | [Prosperous exists, and it is a library with two consumers](decisions/D001-prosperous-exists-and-it-is-a-library.md) | decided | 2026-08-25 |
| 🟢 | D002 | [A target's answer is read on its own terms, and a refusal is not a failure](decisions/D002-a-target-s-answer-is-read-on-its-own.md) | decided | 2026-08-25 |
| 🟢 | D003 | [The fake blocks on accept and knocks to stop, because polling was measured and was terrible](decisions/D003-the-fake-blocks-on-accept-and-knocks-to.md) | measured | 2026-08-26 |
| 🟢 | D004 | [What cannot be checked is refused by name, in both places it comes up](decisions/D004-what-cannot-be-checked-is-refused-by.md) | decided | 2026-08-26 |
| 🟢 | D005 | [The command line holds nothing, and its exit codes are part of what it holds](decisions/D005-the-command-line-holds-nothing-and-its.md) | decided | 2026-08-26 |
| 🟢 | D006 | [Continuous integration runs the one dev command, and nothing else](decisions/D006-continuous-integration-runs-the-one-dev.md) | decided | 2026-08-26 |
| 🟡 | D007 | [The target is a manifest source, and the path is asked for rather than assumed](decisions/D007-the-target-is-a-manifest-source-and-the.md) | assumed | 2026-08-26 |
| 🟢 | D008 | [The frame grabber is a resident payload with its own port, and its protocol is written down before it is built](decisions/D008-the-frame-grabber-is-a-resident-payload.md) | decided | 2026-08-26 |
| 🟢 | D009 | [The window matches orbistoun's, deliberately, down to the version](decisions/D009-the-window-matches-orbistoun-s.md) | decided | 2026-08-26 |
| 🟢 | D010 | [Two more columns, and neither of them may lie about what was not looked at](decisions/D010-two-more-columns-and-neither-of-them.md) | decided | 2026-08-26 |
| 🟢 | D011 | [The library, backups, and what a copy has to promise](decisions/D011-the-library-backups-and-what-a-copy-has.md) | decided | 2026-08-26 |
| 🟢 | D012 | [The window is a sidebar of sections, and streaming is somebody else's program](decisions/D012-the-window-is-a-sidebar-of-sections-and.md) | decided | 2026-08-26 |
| 🟢 | D013 | [A target answered, and three guesses became measurements while two rules turned out wrong](decisions/D013-a-target-answered-and-three-guesses.md) | decided | 2026-08-26 |
| 🟢 | D014 | [Downloading is built, because a target handed over digests to check it against](decisions/D014-downloading-is-built-because-a-target.md) | decided | 2026-08-26 |
| 🟢 | D015 | [Names come from the target, and are never invented](decisions/D015-names-come-from-the-target-and-are.md) | decided | 2026-08-26 |
| ⚪ | D016 | [The manifest format is one tool's file, and `port` is ours](decisions/D016-the-manifest-format-is-one-tool-s-file.md) | unrecorded | ~2026-08-26..08-30 |
| 🟢 | D017 | [Five lists, one mechanism, and what an honest list can contain](decisions/D017-five-lists-one-mechanism-and-what-an.md) | measured | ~2026-08-26..08-30 |
| ⚪ | D018 | [A save transfer decides before it moves anything](decisions/D018-a-save-transfer-decides-before-it-moves.md) | unrecorded | ~2026-08-26..08-30 |
| ⚪ | D019 | [The first write to a target, and what it had to earn](decisions/D019-the-first-write-to-a-target-and-what-it.md) | unrecorded | ~2026-08-26..08-30 |
| 🟢 | D020 | [What the target is, measured rather than looked up](decisions/D020-what-the-target-is-measured-rather-than.md) | measured | ~2026-08-26..08-30 |
| 🟢 | D021 | [Package installation was a shell builtin, not a service](decisions/D021-package-installation-was-a-shell.md) | measured | ~2026-08-26..08-30 |
| 🟢 | D022 | [This project sends and supervises; it does not drive](decisions/D022-this-project-sends-and-supervises-it.md) | hardware | ~2026-08-26..08-30 |
| ⚪ | D023 | [Two pad models, on purpose](decisions/D023-two-pad-models-on-purpose.md) | unrecorded | ~2026-08-26..08-30 |
| ⚪ | D024 | [Three ways to make something run on a target, and three buttons](decisions/D024-three-ways-to-make-something-run-on-a.md) | unrecorded | ~2026-08-26..08-30 |
| 🟢 | D025 | [`pros-link` takes the logging facade, and the empty dependency table goes](decisions/D025-pros-link-takes-the-logging-facade-and.md) | decided | 2026-08-30 |
| 🟢 | D026 | [The window ships its own manual](decisions/D026-the-window-ships-its-own-manual.md) | decided | 2026-08-30 |

| | meaning |
|---|---|
| 🟢 | settled, and the reasoning rests on something checkable |
| 🟡 | assumed or proposed - made without input, and in the review queue |
| 🔴 | reversed, superseded or blocked |
| ⚪ | no status recorded |

A date with `~` is **not recorded** - it is worked out from the dated entries either
side, because an entry between two of them was written between their dates. `~` alone
is a day both neighbours agree on; `~a..b` is a span, and no day inside it is claimed;
`~>a` and `~<a` are entries with a dated neighbour on only one side. A bare `-` has no
dated entry either side to reason from.
