# D030 - The guard routes a location; it never rewrites an identity


**decided** · 2026-09-17 · principle 2

`guard::check` refuses a staging destination the console will silently ignore and offers a
better one. It had grown a second job: judging the title's **prefix**. Anything not `PPSA`,
`CUSA` or `FAKE` was declared incompatible and the id was *rewritten* to `PPSA<suffix>` -
`sanitize_title_id`. So a homebrew title `MESA00001` sent to `/data/homebrew/MESA00001` - the
exact right place - was refused, and with `-y` its destination became `/data/homebrew/PPSA00001`,
a Sony id nobody asked for that landed on top of an unrelated title already installed there. A
guard meant to prevent a silent non-appearance caused a silent overwrite.

Two things were wrong, and both are now gone:

- **A prefix is not a defect, and the homebrew folder is where a non-Sony prefix belongs.** The
  rule this project actually wants is a *location* rule - a title whose id is not a Sony prefix
  goes to `/data/homebrew` - and `MESA00001` under `/data/homebrew` already satisfies it. So any
  destination that is not an inert system path is accepted as written, whatever the prefix. The
  guard's one remaining job is the one it began with: catching `/user/app`, where an upload
  succeeds on the wire and is never scanned.

- **Nothing here rewrites an identity.** A redirect keeps the title's own id: an inert
  `/user/app/GLCB00001` is offered `/data/homebrew/GLCB00001`, not a `PPSA` invention.
  `suggested_id` stays a field of `Refusal` for callers, but it is always the id that came in.

Why this is a principle-2 correction and not a feature change: the prefix list was **reasoned**,
not measured. The module asserted ShadowMountPlus "strictly requires PPSA/CUSA/FAKE"; nothing
here measured that, and the person holding the console says a non-Sony id mounts from
`/data/homebrew` under its own name. A measured constraint could return one day - as a
*non-destructive* finding with a citation and never an id rewrite - but a reasoned one that
overwrites a title on `-y` is the shape of exactly the plausible-wrong-default this project
exists to refuse. `is_supported_prefix`, `SUPPORTED_PREFIXES` and `sanitize_title_id` are removed
with it; `IssueKind` keeps its one measured variant, `InertPath`.
