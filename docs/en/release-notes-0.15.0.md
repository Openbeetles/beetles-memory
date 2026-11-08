# Beetle Memory 0.15.0 macOS SDK Source Release Notes

This source release repairs failure handling in the official post-turn learning worker. The published identity is the verified `v0.15.0` Git tag. Delivery is limited to the macOS `bm-sdk` source and its required Core, Store, and Entry contracts; it does not include crates.io, an installer, a hosted Release, or deployment.

## Governed learning recovery

- A failed or truncated HTTP response body read is a transport failure and enters the existing bounded retry schedule.
- A complete model response with invalid JSON, an invalid response shape, or an invalid persona, private-garden, or long-term extraction output is a typed malformed-model-output failure and enters bounded retry.
- Trusted input, source, identity, configuration, and response-byte-budget violations still reject immediately. Credential failure is no longer classified as malformed model output.
- A governance job and its controlled queue summary expose fixed failure-stage and failure-code enums. They do not retain the raw response, model text, credential, or private error message. Success, cancellation, and blocking clear stale failure diagnostics.

## Source and Store contract

The public Rust job type is `PostTurnGovernanceJob`; the new diagnostic types are `PostTurnGovernanceFailureStage` and `PostTurnGovernanceFailureCode`. These names have no revision suffix. The persisted Job and Store still carry numeric schema generations so a v0.14.0 Store fails to open without modifying its data. No compatibility reader or automatic migration is included. Hosts must use a fresh Store unless its data owner has separately resolved the older data.

The current source tag also corrects stale archive-root type references in the Entry console and Store contract fixture. No archive behavior is changed.

## Validation boundary

Targeted Core, Store, Entry, and SDK tests cover response-body interruption, malformed output, terminal contract errors, retry transitions, safe diagnostics, and schema rejection. The macOS embedded SDK production profile compiles. Build and test caches were placed on an external disk. Full packaging and platform matrices, a live Provider, real user data, and downstream application integration were not exercised. Earlier intermittent failures cannot be retrospectively attributed to a specific path because the old job record did not retain a safe phase code.
