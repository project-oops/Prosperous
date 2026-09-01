# D019 - The first write to a target, and what it had to earn


Everything until this point asked questions. Editing the payload manager's settings is the
first thing that writes, and the file it writes decides what loads at startup - so a wrong one
is a target that comes up without its file service or its loader, which is precisely the state
where nothing in this tool can help. The recovery is re-running the jailbreak by hand.

Three rules, all the same fear:

- **Nothing is written that was not read first.** An edit is applied to the text that came off
  the target, in memory, and that text goes back. Comments, ordering and keys this version
  does not understand survive because they were never taken apart.
- **What would change is shown before it changes**, line by line. Confirming *"change the
  autoload delay"* and confirming *these two lines* are different acts, and only the second
  catches a tool about to do something else as well.
- **Setting a value to what it already is produces no change at all** - not an empty one, none.
  A confirm dialog for a write that would do nothing teaches somebody to click through
  confirms, which is the habit that makes the real one dangerous.

Verified against a target read-only: the real file fetched, the edit made in memory, the diff
checked to touch one line with every other setting intact. **Nothing was sent.** A test that
reorders somebody's boot list to prove it can is a test that breaks their target to prove it
works.

