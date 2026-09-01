# D021 - Package installation was a shell builtin, not a service


An earlier decision here recorded package installation as unmeasured: nothing answered on
8080, 9090 or 12800, so it was left as an open question rather than guessed at. That was the
right call and it was also looking in the wrong place.

**The shell has `pkg_install`.** No service, no port - a builtin, on the shell that was already
being used for everything else. Found by running `help` on it, which also turned up `launch`,
`hbldr`, `notify`, `sum` and `sysctl`. A whole capability sat one command away for as long as
this project had been probing ports for it.

It takes a URL, and a bare path reaches the same code inside it as a `file://` one - measured
by handing it a file that does not exist and getting an identical complaint from each. So a
package goes onto the target first and is installed from there. The alternative is serving it
over HTTP from this machine, and a tool that opens a listening socket to move one file is a
tool with a second thing to get wrong.

**Amended twice, and the paragraph above is wrong on both counts.** It is kept because how it
was wrong is the useful part.

*First:* the two forms gave an identical complaint because **both fail**, not because they
share code. A missing file is the one input that cannot distinguish two paths through a
program. The conclusion was then acted on - install was wired to a form that had never moved a
real package - and it took testing with a real one to find out. This project now serves the
package over HTTP, which the paragraph above argues against; see `handover`, which says why
that argument lost.

*Second, and later still:* **"it takes a URL" is more than was measured.** The builtin does no
scheme checking whatsoever - `metainfo.uri = argv[1]` handed straight to
`sceAppInstUtilInstallByPackage`, read in `shsrv/bundles/pkg_install/pkg_install.c` - and
etaHEN's writeup of that API says its `url` accepts local paths too. What is actually known is
that **one path form was tried and produced nothing**. `/user/data/...` was never tried, and
there is a reason to think the distinction matters: `/user/data` and `/data` are the same store
under two names, and the installer is a system service that need not share the shell's view of
the tree.

Both amendments are the same mistake: **a measurement of one case, written down as a fact about
every case.**

**Nobody here has watched a successful install**, because finding out what success looks like
means installing something on somebody's target. So:

- The measured failure - an empty `content_id`, which is what an unreadable package produces -
  is reported as a failure.
- Silence is reported as silence and explicitly not as success. A fetch can outlast the window
  the shell is given, so nothing said means *still going or never started*, and this side
  cannot tell those apart.
- **Everything else is `Unclear`**, carrying the target's own words. A version that called any
  other output *installed* would be right most of the time and would say exactly the same
  thing when it was wrong.

A path with a space in it is refused rather than sent. The shell splits on spaces and has no
quoting, so passing one would install whatever the first word named - and the button says so
on hover instead of quietly not being there.

