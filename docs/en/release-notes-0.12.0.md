# Beetle Memory 0.12.0 Source Release Notes

This source version is an SDK-first breaking release. Release publication is evidenced by the `v0.12.0` Git tag, remote readback, and recorded checks; this note alone does not establish those results. No crates.io package, binary, installer, hosted Release, or deployment is implied.

## SDK turn contract

`CanonicalTurnDelta.assistant_messages: Vec<TranscriptAssistantMessage>` replaces the singular `assistant_message`. Messages retain their order and each has a `Complete` or `InterruptedDeliveredPrefix` settlement. A `Delivered` turn contains one or more complete assistant messages. `Cancelled`, `UpstreamFailed`, and `IncompleteStream` can preserve complete messages and one final settled interrupted delivered prefix. Unsettled stream fragments are not committed. `UserOnly` and `RejectedByPolicy` have no assistant messages. Intake rejects the entire turn if it has more than 16 assistant messages or any assistant message body exceeds 4 KiB. Exact retry requires the original canonical payload, including message order and settlements.

Replay, timeline, search, export, and source references consume the ordered messages. Post-turn learning admits only the settled evidence allowed by the turn status; an interrupt does not promote transient text into an action or tool capability. `DeleteRaw` removes affected raw transcript text and its Session shadow, attrs/auxiliary material, host-reference labels, indexes, and dependent summary text in the same governed closure. Accepted long-term facts remain subject to their own explicit governance.

## Store and integration

Store v17 is a clean break. It does not read Store v16 or provide a compatibility reader, dual write, or automatic migration. Opening an old Store must fail without rewriting it. Do not point this version at the only copy of a real Store; owners must decide any separate data transition. Archive import is not a schema migration. All Beetle crates in one host must use the same source tag. See [Getting Started](getting-started.md), [Integration](integration.md), [API Surface](api.md), and [Store Backends](store-backends.md).

The published v0.11.2 tag remains the Store v16 contract. This version does not change that historical release. Local Core/SDK/Store checks, release gate results, tag/remote receipts, real data, Provider, GUI, and target-platform evidence must be reported separately; this note does not claim any unrecorded check or publication.
