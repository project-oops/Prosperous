# D010 - Two more columns, and neither of them may lie about what was not looked at


**decided** - 2026-08-26 - the payload browser

A window that lists payloads was asked for. Building it needed two judgements that do not
belong in a window, so both are in `pros-core` and the command line gives the same answers.

### Presence has three states, and the third one is the point

**A payload nothing here can see is not a payload that is absent.**

Five services have known ports, so their presence is a measurement. Everything else in a
repository - a file manager, a cheat menu, anything somebody added - has no port this project
knows, and no amount of probing will find it. Reporting those as *not loaded* would be
inventing a measurement, and it would be believed, because it would sit in the same column as
the ones that are real.

The first target-shaped test of this had `ShadowMountPlus` in the manifest, which is in the
real boot chain and answers on nothing. It reads `?`, and that is correct.

### The boot list is a second question, not more of the first

A check says what is answering **now**. It says nothing about what will be answering after
the next power cycle, and those have the same-looking answer. **A service can be running and
absent from the boot list**, which means it is there until somebody turns the target off -
and that is usually the finding somebody actually needed.

So `chain` reads `/data/pldmgr/autoload.txt` and the table gains a boot column with the same
discipline: a list that could not be read is *unknown*, never *not in it*.

**That path is a constant while the repository's is a parameter**, and the difference is the
whole point of the grading the sibling projects do. The boot list's path and the order it
produces were measured against a target on 2026-08-25. The repository's path was reasoned
about. One is a fact and the other is a good guess, and a program that stored them the same
way would have forgotten which was which.

### Staging is the answer to fetching, not a substitute for it

`pros check` cannot repair what it finds because reaching a public mirror needs a security
layer nobody has argued for yet. That is a real gap and it hid a better observation:
**somebody who already has the payload needs nothing from that decision.** They downloaded it
from the project that publishes it, which is where the manifest was going to point.

So `pros stage` takes a file you already have, checks it against what the manifest says it
should be, and keeps it. Fetching, when it arrives, becomes **a way of filling that directory
rather than a new path through the program** - which is a much smaller thing to add, and a
much easier decision to take once the workflow around it already works.

**The digest is checked on the way in, not on the way out.** Everything in the staging
directory is then already known to be what it claims, which is a promise a directory can
keep. An entry whose digest cannot be checked gets no file staged at all: one exception and
the promise is worth nothing.

### The browser shows the gap rather than hiding it

The send button exists for every row, disabled where it cannot work, and says why on hover -
*not staged here, and fetching from a mirror is not built yet*. Every control is disabled
rather than hidden, which is the sibling project's rule: a control that vanishes reads as a
bug, a greyed one reads as a state.

That puts the missing half of this project in the product where somebody will meet it, rather
than in a decision log where only I will.

