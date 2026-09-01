# Unpairing from the scene

A design note, written 2026-08-27. **Steps 1 and 2 of the staging below are built**; the rest
is design.

Prosperous should still be worth running when every payload it currently names has been
replaced. This says what that requires, and - more usefully - what it does **not** require,
because the instinct to make everything configurable is how a tool acquires a second source
of truth that can disagree with itself.

---

## The evidence that this is not hypothetical

The payload list this program ships already contains **three FTP servers** - `ftpsrv`,
`ftpsrv-drakmor` and `zftpd` - and **several things that load payloads at startup** -
`pldmgr`, `etaHEN`, `WebKit-Autoloader-Installer`.

Prosperous hardcodes one of each. Someone running `zftpd` gets told `ftpsrv is not loaded`
while every file transfer works fine, because `zftpd` answers the same port with the same
protocol. Someone running etaHEN's autoloader gets an autoload screen that is not degraded
but **dead**: four paths and three file formats, all of them pldmgr's.

---

## Three layers, and only the middle one is configuration

The mistake would be to make it all config. These are genuinely different kinds of fact.

### 1. Protocols - durable, and they stay in code

FTP is FTP whoever serves it. A telnet-shaped shell is a shell. A socket that accepts an ELF
and spawns it is a loader. **This is what Prosperous actually speaks**, and none of it changes
when a payload is replaced by a rival.

A config file must never be able to say "this provider speaks a protocol we have no code for".
That is not extensibility, it is a promise the program cannot keep.

### 2. Providers - volatile, and this is the config

Which payload provides which capability, on which port, at which paths, in which file format.
**Every hardcoded name in the table below belongs here.** This is the layer the scene churns.

### 3. Facts about the machine - durable, and they stay in code

`/user/home`, `/user/app`, `/user/appmeta`, the SFO layout, the save container shape, the
keystone. These are properties of the machine itself, not of anything anybody installed. Making them
configurable would invite somebody to write a wrong one and get a confidently wrong answer -
and unlike a port, there is no second opinion available.

**The test for which layer something belongs in:** *would a different jailbreak change it?* If
yes it is layer 2. If only a different machine would change it, it is layer 3.

---

## What is in the wrong layer today

Non-test code only.

| Constant | Currently | Belongs in |
|---|---|---|
| `chain::PATH` = `/data/pldmgr/autoload.txt` | code | 2 |
| `autoload::CONFIG` = `/data/pldmgr/pldmgr_config.txt` | code | 2 |
| `manifest::TARGET_REPOSITORY` = `/data/pldmgr/repository_cache.json` | code | 2 |
| `PAYLOADS` = `/data/pldmgr/payloads` | code | 2 |
| `SERVICES` - five names with ports, `required`, `unlocks` | code, **ports overridable per target** | 2 |
| `Section::candidates` - cheat and package directories | code | 2 (already half-moved) |
| `saves::HOME`, `titles::APPMETA`, `/user/app` | code | 3 - **correct as they are** |

The cheats section is the shape to copy: it already asks the target which of several candidate
directories exists, rather than asserting one. That was built because no standard location
existed. The same is true of everything else in this table; it just had one obvious answer at
the time it was written.

---

## Capability, not service

The check currently asks *is ftpsrv answering?* The question it means is *can I move files?*

Those differ exactly when somebody swaps a payload, which is the case this document exists
for. So a capability names what Prosperous needs, and lists what is known to provide it:

```toml
[[capability]]
id       = "files"
speaks   = "ftp"                 # must name a protocol this program has code for
unlocks  = "retrieve reports, stage payloads and packages"
required = true

  [[capability.provider]]
  payload = "ftpsrv"
  port    = 2121

  [[capability.provider]]
  payload = "zftpd"
  port    = 2121

  [[capability.provider]]
  payload = "ftpsrv-drakmor"
  port    = 2121
```

A capability is satisfied when **any** provider answers. The report says which one, because
"files: yes, via zftpd" is a different fact from "files: yes" and the difference is what
somebody debugs with.

Paths move the same way, onto the provider that owns them:

```toml
[[capability]]
id     = "autoload"
speaks = "pldmgr-v1"             # a file format, and a version of it

  [[capability.provider]]
  payload  = "pldmgr"
  port     = 8084
  chain    = "/data/pldmgr/autoload.txt"
  settings = "/data/pldmgr/pldmgr_config.txt"
  payloads = "/data/pldmgr/payloads"
```

`speaks` is what stops this becoming a lie: etaHEN's autoloader can be listed as a provider of
`autoload` only once there is code that reads its format. Until then it is absent from the
file, and the honest report is *no autoload provider found*, not a screen full of parse errors.

### Dependencies

Providers depend on each other, and the current check cannot say so. `pldmgr` launches
everything through `elfldr`, so `elfldr` being down explains `pldmgr` being down - and
reporting them as two independent failures buries the one that matters.

```toml
  [[capability.provider]]
  payload = "pldmgr"
  needs   = ["loader"]           # capability ids, not payload names
```

Stated against **capabilities**, not payloads, or the dependency graph re-couples to the names
this whole document is about removing.

The payoff is the verdict getting shorter and truer: a root failure is reported once, and what
follows from it is reported as following from it. That is the `RerunTheJailbreak` remedy
generalised - it exists today precisely because the loader is the one dependency the code
knows about, hardcoded.

---

## What this must not become

- **A file that can describe a protocol nobody implemented.** `speaks` is validated against
  what is compiled in, and an unknown value is refused loudly when the file is read - not at
  the moment somebody presses a button.
- **A second list of payloads.** The manifest already describes payloads: name, url, digest,
  version, and now `port`. This file describes **capabilities**, and refers to payloads by the
  name the manifest already uses. Two files that both list payloads would eventually disagree.
- **A reason to make layer 3 configurable.** A wrong port is discovered in seconds. A wrong
  save path is discovered when a restore does not come back.
- **Editable without a default.** The built-in list stays compiled in and is what runs when no
  file exists. A tool that needs a config file before it works is a tool that is broken out of
  the box.

---

## Staging

Each step is worth having on its own, which is the point of the order.

1. ~~**Widen the manifest schema** with `unlocks` and `required`~~ - **done.** A declared payload can
   already reach the verdict instead of only being probed. Small, no new file, and it makes
   the `port` field earn its place.
2. ~~**Per-target port overrides**~~ - **done.** On the registration rather than in a services file. This was
   the case of *ftpsrv, but not on 2121*, and it needs the number to reach the connecting
   code - not only the probe - or the config can lie while the check goes green.
3. **Capabilities file**, defaulting to today's five, with providers and `speaks`.
4. **Move pldmgr's four paths onto its provider entry**, which is the point at which a second
   autoloader becomes a thing somebody can add rather than a fork.
5. **Dependencies**, once there is more than one provider to depend on anything.

Steps 1 and 2 are useful whether or not 3 through 5 ever happen. Nothing here is worth doing
as one change.
