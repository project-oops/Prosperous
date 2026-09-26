# Glossary

The words Prosperous uses. Shared vocabulary, and words that mean different things in
different repositories, are in
[the collection's glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md).
File formats are in [SELFish's](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md).

**Target** is defined for the collection in
[CONVENTIONS section 2](https://github.com/project-oops/OOPS/blob/main/docs/CONVENTIONS.md#the-words-for-our-own-layers):
here, a machine Prosperous has registered by name and address.

## Services on the target

Prosperous speaks to services the entry point starts. [DESIGN.md](DESIGN.md) has the ports.

| Name | What it is |
|---|---|
| **`elfldr`** | The loader: send it an ELF and it runs it. Everything else is launched through it |
| **`pldmgr`** | The payload manager, which starts the chain through `elfldr` |
| **`klogsrv`** | The kernel log, streamed over a socket |
| **`shsrv`** | A shell over raw TCP |
| **`ftpsrv`** | File transfer |

**Entry point** - the procedure run by hand that starts the homebrew services on the target.

**Payload** - a plain ELF sent to a target and run. It is mapped and executed, not installed.

## Prosperous terms

**Autoload** - the payload manager's startup list and the settings around it. Its path is a
measured constant.

**Capability** - something Prosperous needs from a target, such as moving files, satisfied by
any provider that answers. See [DESIGN.md](DESIGN.md#capabilities).

**Chain** - what the target loads when it comes back after a power cycle. A check says what
answers now; the chain says what will answer next time.

**Check** - a report of which services answer and what each one unlocks, with a verdict of
ready, dimmed or blocked.

**Doctor** - a check that also states the repair for each finding, as a plan a person
confirms.

**Portable mode** - an empty `.portable` directory beside the binaries makes both programs
keep their data there instead of in the user profile.

**Porthole** - the first-party capture and input path: the target's encoded video out on
9805 and controller state in on 9806, over a payload of our own. See [VIDEO.md](VIDEO.md#porthole).

**Provider** - a payload that supplies a capability, on a given port and at given paths.

**Scan root** - a directory an auto-mounter watches. A title directory placed there is
registered; the directory titles are registered to is never scanned.

## The two programs

**`pros`** is the command line and **`pros-gui`** is the window. They offer the same
capabilities, so none is reachable from only one of them.

## Words that mean something else next door

**Shape**, **check**, **corpus** and **probe** carry other senses in obSCEne and Orbistoun.
The [collection glossary](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md)
lists the collisions.

## Other glossaries

- [the collection's](https://github.com/project-oops/OOPS/blob/main/docs/GLOSSARY.md) - ELF, `DT_`/`PT_`, cross-repository collisions
- [SELFish](https://github.com/project-oops/SELFish/blob/main/docs/GLOSSARY.md) - NID, fSELF, PFS, packages, the generation split
- [obSCEne](https://github.com/project-oops/obSCEne/blob/main/docs/GLOSSARY.md) - checks, the census, previous-generation mode against native
- [Orbistoun](https://github.com/project-oops/Orbistoun/blob/main/docs/GLOSSARY.md) - guest execution, thunks, HLE
