# D017 - Five lists, one mechanism, and what an honest list can contain


The shipped payload list used to hold nine entries with no url and no checksum, on the
grounds that this project had measured neither. A target then handed over both, and one
entry was fetched and verified end to end. Keeping the stub after that would have been a
different dishonesty: pretending not to know something that had been checked.

So every kind now ships a real list, and the test that used to assert *no entry states a
digest* asserts the opposite - **nothing ships with a url it cannot check**. A url with no
digest is the one combination that invites an unverifiable download, and it is now impossible
to ship rather than merely discouraged.

What each list can honestly contain was decided per kind, not by what was easy to find:

- **Payloads** (25) came off a target's own payload-manager repository.
- **Packages** (17) and **titles** (9) are one release and one kind of artifact - zip bundles
  launched through the homebrew server, installed under `/data/homebrew`. The split between
  the two files is this project's convenience and nothing depends on it being right, which is
  said in both files rather than left to be inferred.
- **An overstatement, corrected on sight.** This first read *PS5 homebrew is not distributed
  as PKG files*, generalised from one release shipping zips. A target's `/data/pkg` holds
  `.pkg` files, some of them homebrew, so the claim was false. What is true is narrower: no
  public source of downloadable homebrew `.pkg` files with digests was found. **A gap in what
  has been measured is not evidence of absence**, and writing it as though it were is the same
  failure this project exists to catch, committed in prose.
- **Titles carries no commercial games and never will**, because a list of urls for commercial
  titles is a piracy index. A test asserts every entry names its own publisher - it cannot
  test intent, so it tests the property that follows from it.
- **Cheats** (4) are pinned to a commit rather than a branch. A branch url serves whatever is
  there today, so the recorded digest goes stale on the next push - and a stale digest reads
  as a corrupted download, sending somebody after a network fault that does not exist.
- **Saves ship empty on purpose.** A save here is signed for the target that wrote it, so a
  downloaded save is a file the target rejects. A list of them would be a list of things that
  do not work, each entry indistinguishable from one that does. A test pins the emptiness,
  because *nobody got round to it* and *this cannot honestly exist* look identical in an empty
  file and only one of them should survive somebody tidying up.

**The digests for packages and titles are GitHub's stated asset digests.** That is a claim by
a host rather than a measurement here, so two of them were confirmed by downloading the file
and hashing it. Both matched, which is what justifies trusting the rest without pulling a
gigabyte. The cheat digests were computed here from the downloaded files.

`/data/homebrew` was measured on a target. The schema in `docs/manifest.schema.json` covers
all five, because none of the machinery - fetch, verify, stage, send - cares what kind of
thing it is moving.


