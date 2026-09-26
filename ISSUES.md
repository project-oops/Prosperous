# Issues

Open defects, gaps and unmeasured facts, one line each. Delete a line when it is fixed.

## Defects

- 24 user-visible strings carry long space runs from lost line continuations (`doctor.rs` provision, `listing.rs` install refusal, the "fix all" hover text, test messages).

## Gaps

- Save retargeting (`graft`, `set_account`, `sfo::set`) has no caller.
- No progress is shown for a single large file.
- A send is not read back and verified.
- Shell builtins `notify`, `browse`, `mount`, `sfoinfo` and `sfocreate` have no verb.
- The capabilities file and its providers, and a pad-facts crate, are not built.
- No `PCTL` control record exists.
- Moonlight audio, and an end-to-end test with a real client, are missing.
- Media transfer is not built.
- `doctor.rs`, `pros-cli/src/main.rs` and `recovery.rs` are over 1,500 lines.

## Unmeasured

- Whether a grafted save is accepted.
- The `PARAMS` HMAC.
- `ACCOUNT_ID=0`.
- An `sdimg_` round trip.
