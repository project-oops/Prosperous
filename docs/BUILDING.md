# Building Prosperous

There is one command and it is `bin/prosperous`. Every verb is the same command CI runs.

```bash
./bin/prosperous check
```

If that passes, the tree is sound. It needs no target, no console, and no network.

## What you need

**A Rust toolchain.** Nothing else - no C compiler, no vendor SDK, no firmware, no signing
keys.

### One sibling

**A clone of only this repository is not enough.** Prosperous takes `oops-build`, `oops-log`
and `oops-paths` from `oops-libs` by relative path, as a sibling, so the layout is a build
requirement rather than a convenience. Without it the build fails as a missing *directory*
rather than as a missing dependency, which is a much worse error to read.

```bash
./bin/oops bootstrap prosperous    # fetches oops-libs, and nothing else
```

### Tests need no target

`pros-link` ships a fake one. It is **not** behind `#[cfg(test)]`, deliberately: every
consumer has the same problem, and three private copies of a fake target is exactly what that
crate exists to prevent.

The integration tests stand that fake on the hardware's own port numbers. Those are free on a
runner, and the bind is *checked* rather than assumed - a bind failure fails the test by name
rather than turning into a mysterious connection error further down.

## The seven shared verbs

So `oops test prosperous` and `./bin/prosperous test` are one command reached two ways.

| verb | what it does |
|---|---|
| `build` | `cargo build --release --workspace` |
| `test` | `cargo test --workspace` |
| `lint` | clippy at `-D warnings` |
| `fmt` | format in place |
| `check` | the full gate - see below |
| `clean` | `cargo clean` |
| `doc` | `cargo doc --no-deps --workspace` |

`check` runs these and more. The individual verbs exist so the pieces can be asked for one at
a time, and so the same word means the same thing in all four projects.

## Prosperous's own two

| verb | what it does |
|---|---|
| `provenance` | no payload binaries are tracked |
| `target` | the read-only tests against a real machine |

### `provenance`

The whole distribution policy rests on this. The payloads are GPL-3.0: shipping a binary
obliges you to offer its source, while pointing at upstream obliges nothing. A policy with no
check is a habit, and habits do not survive a hurried afternoon.

It **refuses to pass when it could not look**. The first version swallowed the failure with
`|| true`, so outside a git repository it reported success having checked nothing - the same
shape as every guard that has ever been trusted wrongly.

### `target`

```bash
PROS_TARGET=192.168.1.211 ./bin/prosperous target
```

Separate from `check` on purpose. Everything in `check` runs anywhere; these need a machine
that is switched on, and a suite that *sometimes* needs a target is a suite whose failures
nobody can interpret. They are `#[ignore]` by default, so an ordinary run reports them as
**ignored** rather than as passing.

## What `check` runs

In order, and it is what CI runs:

1. `provenance` - no payload binaries tracked
2. `cargo fmt --all -- --check`
3. `cargo clippy --all-targets -- -D warnings`
4. `cargo test`
5. `cargo doc --no-deps --workspace` with `RUSTDOCFLAGS="-D warnings"`

The doc build is in the gate rather than beside it because this project's documentation is
load-bearing - the reasoning behind a decision lives next to the code resting on it - and a
warning nobody fails on is a warning nobody reads.

## The two binaries

The workspace builds four crates; two of them are programs, and they do **nearly** the same
things - see the exception below.

| | |
|---|---|
| `pros` | the command line |
| `pros-gui` | the window |

```bash
./bin/prosperous build
./target/release/pros check --name living-room
./target/release/pros-gui
```

`pros-gui` holds no logic. Every decision it presents - what a registration is, what a missing
loader means, whether an answer was slow enough to remark on - is made in `pros-core` or
`pros-link`, and nearly all of it is reachable from the command line too.

**One thing is not.** `pros_core::install` - the `pkg_install` shell builtin that installs a
package - is called by `pros-gui` and by nothing in `pros-cli`. So the window can install a
package and the command line cannot, which is the single place the two surfaces have drifted
apart. **What has not been established by any
test is that a window appears**; that needs somebody to run it, and saying so is cheaper than
implying otherwise.

Neither program installs anything or writes outside its own data directory, so uninstalling
is deleting the folder. Put an empty `.portable` directory beside the binaries and both keep
their data there instead of in a user profile.

## What CI runs

`.github/workflows/check.yml`, one job, one step: `oops check prosperous` - which is a thin
wrapper over `./bin/prosperous check`, the same command reached the same way a person reaches
it. CI running something *else* is how a green pipeline and a broken working copy come to
disagree.

It checks out the collection without submodules, then this repository **into** that layout,
then `oops bootstrap prosperous` for the sibling. `fetch-depth: 0` is deliberate: the
provenance guard refuses to pass when it cannot look, so a full checkout removes the question
rather than relying on a shallow one still answering it.

## From the collection

[OOPS](https://github.com/project-oops/OOPS) holds all four side by side:

```bash
./bin/oops check prosperous     # also: build, test, fmt, clean
```

[The collection's BUILDING.md](https://github.com/project-oops/OOPS/blob/main/docs/BUILDING.md)
covers `bootstrap`, `gates`, `all`, `git`, `status` and the Windows handling.

## Next

- **[Getting started](guide/getting-started.md)** - from a download to a target that answers.
  Building is not required to use it; the releases page has binaries.
- **[USAGE.md](USAGE.md)** - reading a check, fetching payloads, working with titles and saves.
