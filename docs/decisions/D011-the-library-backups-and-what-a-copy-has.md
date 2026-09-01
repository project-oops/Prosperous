# D011 - The library, backups, and what a copy has to promise


**decided** - 2026-08-26 - extending the window into titles, saves and packages

Browsing storage and copying folders needs no target and no measurement beyond the file
service, so all of it is built. Installing a package is not, and is not pretended: that needs
a protocol nobody here has measured.

### A folder is a title only when the identifier is the whole of its name

Save folders are named after the titles they belong to. Calling those titles would put saves
in the same column as installed software - a claim nobody made. The identifier is still
reported, because *whose save is this* is the useful half of the question.

The identifier is matched as a **shape**: four letters and five digits. That says the name
looks like an identifier. It does not say a title exists, and it certainly does not say which.

### A backup that quietly missed a file is worse than no backup

Because it is trusted at the moment it matters. So the walk **collects everything it did not
copy** and the summary carries it; an incomplete copy exits non-zero and says *do not treat
this as a backup*.

Three rules follow from that one:

- **One unreadable file does not end the walk.** A copy that stops at the first failure has
  saved nothing, and the file it stopped on is usually the least important in the folder.
- **Links are not followed.** One can point at its own parent, and a walk that follows it
  fills a disk. Following them safely needs identity the protocol does not offer, so they are
  reported as skipped - which is a fact about the backup, in the same list as everything else.
- **There is a depth bound**, tested against a listing that recurses for ever.

### Progress is a different kind of message from an answer

The worker's channel carries two: a report of progress ends nothing, and an answer ends the
job and replaces a panel. Folding them into one would make the state machine decide which by
inspecting the payload.

The reason it exists at all is the one this project keeps meeting: a clock says time is
passing, which is exactly what a hung process also does. Naming the file going across is the
difference between *wait* and *kill it*.

### The listing header is not a missing file

The first backup ever run reported three things not copied, and one was `total 8` - part of
the long-form listing format. Reporting it would have named something in every backup ever
taken, **which is how a real warning stops being read**. The transport recognises it now, and
the fake emits a header *and* a genuinely unreadable line so the test still checks the
distinction rather than passing on the header.

### One browser, not two

The main window had a file panel doing what the library window does, over the same service,
with the same code. Two browsers is a thing to explain rather than a thing to have, so the
panel is gone and `library -> files...` opens the one browser at `/data`.

Removing it took a job, an answer, a panel and a state field with it. **Dead paths left after
a removal are how a state machine grows things nobody can account for.**

### The main window is tabbed

Check, log and shell are three activities rather than three parts of one, and stacked in a
single scrolling column each competed for space all of them wanted.

