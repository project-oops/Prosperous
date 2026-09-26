# Decisions

The decisions in force, one file each under `decisions/`. Format and rules are in
[STYLE](https://github.com/project-oops/OOPS/blob/main/docs/STYLE.md#decisions).

**This table is generated.** Edit an entry under `decisions/`, then run
`tools/split-decisions.sh --index prosperous`. A number resolves to exactly one file.

| | # | decision | status | date |
|---|---|---|---|---|
| 🟢 | D001 | [One target library for the collection](decisions/D001-one-target-library.md) | decided | 2026-09-26 |
| 🟢 | D002 | [A refusal is not a failure](decisions/D002-refusal-is-not-a-failure.md) | decided | 2026-09-26 |
| 🟢 | D004 | [Input that cannot be verified is refused by name](decisions/D004-unverifiable-input-is-refused.md) | decided | 2026-09-26 |
| 🟢 | D005 | [Three exit codes](decisions/D005-three-exit-codes.md) | decided | 2026-09-26 |
| 🟢 | D006 | [Continuous integration runs the local gate](decisions/D006-ci-runs-the-local-gate.md) | decided | 2026-09-26 |
| 🟢 | D008 | [Frame grab protocol](decisions/D008-frame-grab-protocol.md) | decided | 2026-09-26 |
| 🟢 | D009 | [The window matches Orbistoun's toolkit](decisions/D009-window-matches-orbistoun.md) | decided | 2026-09-26 |
| 🟢 | D010 | [Presence and boot membership can be unknown](decisions/D010-presence-can-be-unknown.md) | decided | 2026-09-26 |
| 🟢 | D011 | [A copy lists everything it did not copy](decisions/D011-a-copy-lists-what-it-missed.md) | decided | 2026-09-26 |
| 🟢 | D012 | [Solved problems run as configured commands](decisions/D012-solved-problems-run-as-commands.md) | decided | 2026-09-26 |
| 🟢 | D013 | [Target tests are opt-in and read-only](decisions/D013-target-tests-are-opt-in.md) | decided | 2026-09-26 |
| 🟢 | D014 | [Downloads are verified before they are staged](decisions/D014-downloads-verify-before-staging.md) | decided | 2026-09-26 |
| 🟢 | D015 | [Names come from the target](decisions/D015-names-come-from-the-target.md) | decided | 2026-09-26 |
| 🟢 | D016 | [The manifest format is one tool's file, plus `port`](decisions/D016-manifest-format-and-port.md) | decided | 2026-09-26 |
| 🟢 | D017 | [Shipped lists carry only verifiable entries](decisions/D017-shipped-lists-are-verifiable.md) | decided | 2026-09-26 |
| 🟢 | D018 | [A save transfer decides before it moves anything](decisions/D018-save-transfer-decides-first.md) | decided | 2026-09-26 |
| 🟢 | D019 | [A write to a target edits the text it read](decisions/D019-target-writes-edit-what-was-read.md) | decided | 2026-09-26 |
| 🟡 | D021 | [Packages are handed over, and an unclear result stays unclear](decisions/D021-packages-are-handed-over.md) | assumed | 2026-09-26 |
| 🟢 | D022 | [Prosperous sends and supervises a probe, and does not drive it](decisions/D022-send-and-supervise-not-drive.md) | decided | 2026-09-26 |
| 🟢 | D023 | [Two pad models](decisions/D023-two-pad-models.md) | decided | 2026-09-26 |
| 🟢 | D025 | [`pros-link` takes `tracing` and nothing else](decisions/D025-pros-link-takes-tracing.md) | decided | 2026-09-26 |
| 🟢 | D026 | [The window ships its own manual](decisions/D026-the-window-ships-its-manual.md) | decided | 2026-09-26 |
| 🟢 | D027 | [Process control is Prosperous work](decisions/D027-process-control-is-prosperous-work.md) | decided | 2026-09-26 |
| 🟢 | D028 | [The `param.sfo` reader comes from SELFish, the in-place writer stays](decisions/D028-sfo-reader-comes-from-selfish.md) | decided | 2026-09-26 |
| 🟢 | D029 | [A chain carries its files, and which files is data](decisions/D029-a-chain-carries-its-files.md) | decided | 2026-09-26 |
| 🟢 | D030 | [The guard routes a location and never rewrites an identity](decisions/D030-guard-never-rewrites-an-identity.md) | decided | 2026-09-26 |
| 🟢 | D031 | [A store is verified after it lands](decisions/D031-a-store-is-verified-after-landing.md) | decided | 2026-09-26 |
| 🟢 | D033 | [The probe loop ends on park, exit or a cap](decisions/D033-probe-loop-ends-on-park-exit-or-cap.md) | decided | 2026-09-26 |
| 🟢 | D034 | [A restore skips unchanged files on its own record](decisions/D034-restore-skips-unchanged-files.md) | decided | 2026-09-26 |
| 🟢 | D035 | [The process monitor reads and does not act](decisions/D035-process-monitor-is-read-only.md) | decided | 2026-09-26 |
| 🟢 | D036 | [The log filter uses `regex-lite`](decisions/D036-log-filter-uses-regex-lite.md) | decided | 2026-09-26 |

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
