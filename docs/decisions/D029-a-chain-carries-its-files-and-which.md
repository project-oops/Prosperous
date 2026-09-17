# D029 - A chain carries its files, and which files is data


**decided** · 2026-09-15 · principle 2

A chain was a payload order and nothing else, so `export chain` wrote the order down and lost
everything a console kept beside it - the manager's own settings above all, the switch that
decides whether the list runs at all. A chain deployed from that export came up in the right
order and behaved differently, because the behaviour was never in the chain. So a chain now
**carries files**: `baseline::Captured` is a `{path, content}` pair, `Preset.files` a list of
them, and deploying a chain puts each back verbatim (`doctor::Step::Place`).

**The program understands none of it.** A captured file is bytes with a path; nothing here reads
a settings file differently from a note. That is deliberate - the filesystem those files live on
is somebody else's and its names move, so a program that parsed the contents would be a program
that broke when they changed. What it can do without understanding is put a file back where it
was.

**Which files are worth carrying is declared as data, not written in code.** `Document.capture`
(a top-level block in `chain.json`, `baseline::Capture`) names the paths `export chain` reads
off a target, with `{device}`/`{usb}` expanded exactly as a list's places are. This is the
principle-2 move the autoload path already made, taken one step further: a single measured path
could be a constant, but *which files a chain should carry* is a list that grows - a second
settings file, a name that moved - so it lives beside the lists it sits next to on a real
machine, corrected by editing the tracked file rather than by a rebuild. It declares **paths
only**: no machine's settings are shipped here, and a captured copy exists only in a chain
somebody exported off their own console (their `chains.json`, never this repository). This is
what keeps the whole thing JSON-driven - the one thing in code is the switch below.

**The switch still runs, and runs last.** Deploying a manager chain already turned autoload on by
merging one key into the settings (D-era `Step::Enable`, principle 2's endorsed constant path).
That stays, after the carried files are put back, so the order is: write the list, restore the
carried files, then guarantee the switch. A chain whose captured settings happen to have autoload
off is therefore not deployed inert - the switch is the protocol requirement that a list nobody
reads is pointless, and it is independent of whatever the captured file said.

**A shipped chain carries no files.** `capture` names paths to *read*; the shipped presets carry
no `content`, because shipping one console's settings would bake that machine's habits - its
disc-player behaviour, its startup delay - into the program for every target. So the feature is
inert for the chains this program ships (they get the switch and nothing else, and a fresh target
keeps the manager's own defaults) and populated only for a chain someone recorded off a console
that already worked. Recording, not designing - the same stance `from_list` already took for the
payload order.
