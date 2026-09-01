# D015 - Names come from the target, and are never invented


**decided** - 2026-08-26 - after a target showed what it knows about itself

A list of `PPSA01650`, `PPSA02664`, `PPSA04263` is a list somebody has to decode, and the
decoding is not something they can do: the mapping lives on the target, beside each title's
artwork, in `/user/appmeta/<id>/param.json`. Measured, so it is a constant.

    PPSA01650  YouTube
    PPSA02664  Alex Kidd in Miracle World DX
    PPSA04263  Grand Theft Auto V

### Three rules, all of them about not inventing

- **A title that does not answer is shown as its identifier.** Two on this target are
  homebrew with no description at all. They report the identifier and the target's own
  refusal, which is true, rather than a blank where a name should be. In the window they are
  simply absent from the name list rather than present with an empty one - an empty name
  looks like a title called nothing.
- **Any language beats an identifier.** The file names a default language and carries
  several. A name in the wrong language is still the name of the right game; an identifier is
  nobody's language.
- **The file's own `titleId` is believed over the folder it was found in.** A folder can be
  copied; a file describes itself.

### Saves are further down than a default can reach, and this does not choose for anybody

`/user/home/<user>/savedata_prospero/<title>`. The middle part is the problem: a target can
have several accounts, and **picking one would be picking whose saves somebody is about to
overwrite.**

So it descends only when there is nothing to choose between - one user, one answer, path
shown. Several, and it lists them and stops. That is the same rule as resolving a target by
name when only one is registered, applied to something with more at stake.

A save whose title is no longer installed reads *(not installed, so nothing names it)*. Two
of the three on this target are exactly that, and it is the honest thing to say: the save is
real, and nothing on the machine knows what it belongs to any more.

