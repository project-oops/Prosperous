# D025 - `pros-link` takes `tracing` and nothing else

**Status:** decided
**Date:** 2026-09-26

`pros-link`'s one dependency is `tracing`, without `attributes`. No runtime, TLS stack or
serialisation framework; hashing and manifests live in `pros-core`. Each further dependency is
argued for individually in the manifest. A refused port logs at `debug` and a per-service probe at
`trace`.

**Why:** obSCEne takes this crate and holds a deliberate dependency list. `tracing` costs a few
packages and no proc-macro, `max_level_off` compiles it away for a consumer that wants nothing, and
without it the transport cannot say why a connection failed. A shut port is an ordinary answer, so
a warning would fire on a successful check.

**Rejected:**
- No dependencies at all: the transport cannot report what it is doing.
- `tracing` with `attributes`: a proc-macro chain for `#[instrument]`, which nothing uses.
