# D018 - A save transfer decides before it moves anything


Save data here is encrypted and signed for the account that wrote it. Fetching one off a
target always works; putting one back is only a plain send when it is going to the account it
came from. Otherwise it needs decrypting and re-signing, which is a different job and a
different payload.

**The two cases are indistinguishable while they happen.** Both are a directory of files going
across a network, both finish without error, and the difference appears later when a target
refuses a save somebody was relying on - with nothing connecting the refusal back to the copy.
So the decision is made before a byte moves.

Three sources, in order of what they are worth:

1. **The save's own `ACCOUNT_ID`**, read out of the `.sfo` parameter file, compared against the
   account the destination target belongs to. True for a save that arrived by any route.
2. **A record written when this tool made the copy** - the account is in the path it came out
   of, which is the only moment it is known for certain.
3. **Neither, which is `Unknown`** and deliberately not the same as *fine*.

The third exists because measuring said it had to. Of three saves on a target, **one carried a
parameter file and two carried only icons**, so a design resting on (1) alone would work for a
third of saves and fail quietly for the rest. A save with no provenance is also the one most
likely to have come from somewhere else, so defaulting it to a plain copy puts the failure
exactly where it does most damage. The same holds for a known account with nothing to compare
it against: an identifier alone says nothing about whether a copy will work.

The target's own account is read the same way - from a save already on it. A target test
asserts **every save on one target names the same account**, because if that stopped being
true there would be no single account to compare against and the answer would depend on which
save was read first.

A refusal is `Done::Refused`, not `Done::Failed`: nothing went wrong, so it does not clear the
panel somebody is looking at. The reason names both accounts and says what would fix it, and
**copy anyway is offered and is never the default** - somebody may know something this does
not, but they should know it before the files move rather than after.

