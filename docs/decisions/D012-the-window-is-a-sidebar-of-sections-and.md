# D012 - The window is a sidebar of sections, and streaming is somebody else's program


**decided** - 2026-08-26 - restructuring the window around what a person came to do

### Target belongs in the menu, not on the form

Registering a target is done once and then forgotten. A form for it occupying the left of
the window is a form in the way of everything after the first five minutes. It is a dialog
off a `target` menu now, alongside forgetting one and reloading the file.

The sidebar is left for **interaction**: which machine, watching it, and the sections.

### The sections are activities, not panels

`check`, `payloads`, `packages`, `titles`, `saves`, `cheats`, `filesystem`, `log`, `shell`.
Each is a view with its own toolbar, and each gets the whole window.

Four of them - packages, titles, saves and filesystem - are **one browser with four starting
places**. They differ in where they open and in nothing else, and four copies of one thing
would drift within a month. Each remembers where somebody navigated to, so switching away
and back does not undo the navigating.

**Cheats is an empty section that says why it is empty.** Nothing is tracked there yet. A
person looking for it needs to know whether it is missing or merely unfilled, and an absent
section answers neither.

### Three things a payload can have done to it, and one of them is disabled

- **download** - present, disabled, and it says why on hover: reaching a mirror needs a
  security layer this project has not argued for. Hiding it would leave a person wondering
  whether they had missed it, which is worse than a greyed control that explains itself.
- **send** - run it now, through the loader.
- **install** - copy it onto the target's disk to be loaded later.

Install says, every time, that **nothing here has added it to the boot list**. What that file
will accept has not been measured, so this project does not edit it. A tool that silently
half-did that would be the worst version of this available: a payload on the disk, absent
from the chain, and a person believing otherwise.

The directory it installs to is a box a person can correct rather than a constant they
cannot, for the same reason as every other unmeasured path here. (D007)

### Streaming starts a client rather than becoming one

Remote play means pairing, the vendor's transport, per-session encryption, forward error
correction, two video codecs and an audio one. Reimplementing it is a project. Embedding a
client means a large C++ dependency through a foreign function interface, in a workspace that
**forbids** unsafe rather than discouraging it.

So the button starts something that already speaks it, and the command is **a line of text in
a file** with `{address}` substituted. Nobody installs a client in the same place twice and
its arguments differ between forks; hard-coding either would make this work on one machine.
Rules in data, the same as the manifest.

With nothing configured the button is disabled and says so, and a second button writes a
commented file explaining what to put in it. **A disabled button that names the file it needs
is a working feature with a setup step; a hidden one is a missing feature.**

