# Glossary

The words Prosperous uses. For the vocabulary the whole collection shares - standard ELF, and
the words that mean different things in different repositories - see
[the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md).
For the file formats, see [SELFish's](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md).

**target** is defined for all five repositories in
[CONVENTIONS.md §2](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#the-words-for-our-own-layers):
one machine Prosperous has registered, by name and address. It is the word this project uses
most, and it is not repeated here.

## What is running over there

Prosperous does not put software on a console. It speaks to services somebody else's exploit
chain already started, and invents no protocol of its own.
[DESIGN.md](DESIGN.md) has the full picture; the names you will meet:

| Name | What it is |
|---|---|
| **`elfldr`** | Send it an ELF, it runs it. The one everything else goes through, which makes it a single point of failure that fails invisibly |
| **`pldmgr`** | The payload manager. Launches things - through `elfldr` |
| **`klogsrv`** | The kernel log, read over a socket |
| **`shsrv`** | A shell |
| **`ftpsrv`** | File transfer, which is how anything gets copied |

**Payload** - a plain ELF sent to a target to be run. Not a package, not installed: mapped and
executed.

## The concepts this project added

**Chain** - what the target loads *when it comes back*. A health check says what is answering
**now**, which is a different question with a similar-looking answer: a service can be running
today and absent after the next power cycle, because it was never in the boot list. The payload
manager reads that list in order from a file, and a tool that reports "klogsrv is not loaded"
without adding "and it is not in the list either" has left out the useful half.

**Autoload** - that boot list, and the settings around it. The path is a measured constant
rather than a parameter, because it was observed on a target rather than reasoned about - the
distinction the whole project turns on.

**Doctor** - health checks that say what is wrong *and exactly what would put it right*. A
finding with no remedy is half a finding.

**Scan root** - a directory an auto-mounter watches. Drop a title directory there and it gets
registered; it is not the directory titles are registered *to*, which is never scanned, so a
copy placed there is inert.

**Portable mode** - put an empty `.portable` directory beside the binaries and both programs
keep their data there rather than in a user profile. A copy on a stick stays a copy on a stick.

**Porthole** - the planned first-party capture and input path: our own video out and our own
controller state in, over our own payload, so watching and playing a jailbroken target never
speaks the vendor's remote-play protocol. The host half exists; the target payload is a
scaffold that compiles and is deliberately not linked. See
[VIDEO.md](VIDEO.md) part three.

## The two programs

**`pros`** - the command line. **`pros-gui`** - the window. They do nearly the same things on
purpose: a capability in only one of them is a capability that gets forgotten.

## Words that mean something else next door

**Shape**, **check**, **corpus** and **probe** all carry other senses in obSCEne and orbistoun.
The [collection glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md) has
the collisions in one table.

## Where the rest is

- [the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md) - standard ELF, `DT_`/`PT_`, and the cross-repository word collisions
- [SELFish](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md) - NID, fSELF, PFS, packages, the generation split
- [obSCEne](https://github.com/project-oops/obSCEne/blob/main/docs/GLOSSARY.md) - checks, the census, `ps4_mode` against native
- [orbistoun](https://github.com/project-oops/Orbistoun/blob/main/docs/GLOSSARY.md) - guest execution, thunks, HLE
