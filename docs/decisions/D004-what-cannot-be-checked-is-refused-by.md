# D004 - What cannot be checked is refused by name, in both places it comes up


**decided** - 2026-08-26 - building `pros-core`

Two of this crate's four modules read a document somebody else wrote, and both met the same
question: what to do with input that is well-formed and cannot be used. The answer is the
same in both, and it is not the obvious one.

### A checksum algorithm this cannot verify is an error

The obvious implementation verifies what it recognises and passes over what it does not.
That produces a tool which **reports success for an entry it never checked** - the recurring
defect of this project, aimed squarely at the one place where being wrong matters: a payload
is fetched from a mirror somebody else controls and then run with kernel-adjacent privileges.

So an unsupported digest fails at the point the manifest is read, and the message names what
it found - `a md5 digest of 32 digits, which this cannot check`. A person can fix a manifest.
Nobody can fix a silent pass they never learn about.

Only SHA-256 is implemented, because that is what release assets are published with and
because the payload manager's `checksum` format **has not been measured**. Writing a second
algorithm now would mean writing code that has never seen a real input; the error message
already says exactly what to add when one turns up.

### A document that is not a manifest is named, not read as empty

The payload repository's *field names* are known and its *shape* is not. Three plausible
shapes are recognised - a list, a wrapper around a list, an object keyed by name - and
anything else reports what it actually saw.

The failure that avoids is quiet and expensive: **a full file in an unrecognised shape read
as a target with no payloads configured.** That looks like a fact about the target rather
than a tool that did not understand a file, and it would be believed.

The keyed shape supplies the name from its key *before* the entry is read, rather than making
the name optional and filling it in afterwards. Optional would also let a list entry through
unnamed, and a payload that cannot be named cannot be asked for.

### The verdict logic is pure, and the probing is one function

`Report::verdict` decides what a set of findings means; `check` fills those findings in from a
target. The split is deliberate: **a rule about what a missing loader means should not be
reachable only by switching a real target off.** Every rule below is tested with constructed
findings and no network.

The rule that earns it: a missing loader is `Remedy::RerunTheJailbreak`, not one absent
service among several. The payload manager launches everything through the loader - including
the loader - so nothing on the machine can put it back, and every other remedy on the list
assumes it is there. `loader_is_down` asks by **name** rather than by position, because the
loader being first in the table is presentation, and a rule resting on that would break the
day somebody sorted the list.

### What is deliberately not here

**Fetching over the network.** Reading the target's own repository already works over the
local network through `pros-link`, with no dependency at all. Reaching a public mirror needs a
security layer, and that is a real dependency with a real argument to be had - not something
to acquire quietly while building something else.

### Proved by breaking

Loosening the checksum parser to accept an unrecognised algorithm failed two tests, one of
them the manifest-level report of untrustworthy entries. Making an unrecognised document read
as an empty repository failed a third. Restored, twenty-six pass.

