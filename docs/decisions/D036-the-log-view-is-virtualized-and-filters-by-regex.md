# D036 - The log view is virtualized, its buffer is large, and its filter can be a regex


**decided** · 2026-09-22 · principle 2

The log kept 2000 lines on screen, and it was not memory that set the number - 2000 lines is a
few hundred KB, and 50,000 would be single-figure MB. It was the render: the view was one
`TextEdit` over every kept line joined into a single string, laid out in full **every frame**, so a
larger buffer stuttered on a busy target exactly when the log matters most. The bound was the
layout cost, and it was in the wrong place.

**So the view is virtualized.** `egui::ScrollArea::show_rows` lays out only the rows actually on
screen, so a full log costs what a screenful costs and the buffer size stops mattering to the
render. The cap is now 20,000 lines. It is only *scrollback*, not the record: every line is already
appended to the kept file, which holds the full history and rolls at its own size, so the in-memory
cap bounds how far back the window scrolls and nothing more. What remains O(N) is the per-line
filter pass, run on the frames a line arrives - so the previous `line.to_lowercase().contains(..)`,
a fresh `String` per line per frame, is replaced by an allocation-free ASCII-case-insensitive scan.
That pass, not the render, is what a still-larger cap would eventually cost.

This also settled a latent double render: the panel drew the `TextEdit` **and** the toolbar it
called drew a second `ScrollArea` of the same lines. There is one surface now, filling the panel,
which is also why the old "two boxes both claiming the height" layout bug cannot return - there is
no second box to disagree with.

**The filter box can be read as a regular expression, behind a checkbox.** Plain substring is the
default and what most filtering wants; regex is there for what a substring cannot say. The
dependency is **`regex-lite`, not `regex`** - the same team's cut-down engine, with no transitive
dependencies, against the full engine's `aho-corasick` + `memchr` + two `regex-*` crates for
throughput a few thousand short lines do not need. It is argued in `pros-gui/Cargo.toml` beside
`rfd`, the way every dependency here is, rather than assumed. A pattern that does not compile is its
own state: the filter is not applied, **every line is shown** rather than the log blanking on each
keystroke of a half-typed pattern, and the toolbar says *invalid regex* so the unfiltered view is
not mistaken for a match-everything one.
