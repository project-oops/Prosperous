# D025 - `pros-link` takes the logging facade, and the empty dependency table goes


**decided** · 2026-08-30 · measured in obSCEne's own dependency tree

D001 made this crate std-only, and gave the reason: obSCEne takes it and keeps a small,
individually-argued dependency list, so anything added here is added to a policy somebody else
holds. That was right when it was written.

**What changed is that obSCEne also takes `pros-core`**, two lines below `pros-link` in its own
manifest, for the HTTP handover an installer needs. `pros-core` brings `serde`, `serde_json` and
`sha2`. So the boundary the empty table protected was already crossed there, deliberately and
with an argument - and this crate was holding a stricter line than the project it was holding it
for.

What that cost: **the transport was the only layer with no way to say what it was doing.** A
connection that is refused and a name that does not resolve look identical from outside - the
tool simply does not connect - and the remedies are unrelated.

The price, measured in obSCEne's tree rather than estimated: **four packages, no proc-macro.**
`attributes` is off, because that is `#[instrument]`, which nothing here uses, and it drags in a
proc-macro chain and a second copy of `syn`. And `tracing/max_level_off` compiles every call
away statically, so a consumer that wants to pay nothing still can - which is the part that
settles it. The empty table was never the only way to reach zero; it was just the only way
anybody had looked at.

**What has not changed.** No runtime, no TLS stack, no serialisation framework. Hashing and
manifest reading still live one layer up in `pros-core`. The rule is now what obSCEne's own is:
argue for each dependency individually, rather than count them.

Levels here are deliberately low. A refused port is `debug`, not `warn`: on this target a shut
port is the ordinary answer for a service the console is not running, and the caller is asking
in order to find that out - a warning would fire on a *successful* check. The per-service probe
is `trace`, because one runs per service on every check.

