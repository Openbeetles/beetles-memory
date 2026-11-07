# Beetle Memory 0.14.0 macOS SDK Source Release Notes

This version implements vision as the first modality of a general perception contract. Delivery and validation cover the macOS `bm-sdk` and its required Core and Store source only. A repository source tag does not imply a crates.io upload, installer, other-platform build, live Provider integration, or downstream app release. The verified `v0.14.0` Git tag, once published, establishes the source identity.

## Perception input and model delivery

- The generic `PerceptionInputEnvelope` carries source, time, subject scope, and revocation epoch. Media and digests belong to the visual `VisualInput` payload. Vision accepts images, time-ordered frame streams, and short video without audio. Host observations and user-shared photos have distinct source kinds; a shared photo must bind the actual user message and attachment.
- Hosts use `register_perception_host`, `register_visual_tool_snapshot`, `authorize_visual_source`, and `submit_visual_input` to register a device, tool state, source authorization, and media. Capability state distinguishes absent, installed but unavailable, and available, per host and device. Disabling a camera does not prevent the user from sharing a photo.
- `project_for_perception_host` exposes callable vision only for the executing host device. `append_visual_delivery` supplies newly submitted media within a turn. The host returns the actual final-request media and transformations through `attest_visual_delivery`; `record_visual_provider_response` can add Provider-response proof. Camera media remains a restricted perception source even when a Provider requires a user wire role; it is not a user utterance.
- `prepare_turn_visual_external_uses` prepares only visual sources known to Beetle. Durable learning from a visual reply requires the host to submit a manifest of **all** external content and final requests through `submit_turn_external_content_manifest`, including other tool results and retrieved material. Completeness is host-attested: Beetle checks known visual receipts, replay owners, and tool observations, but cannot independently prove that the host added no unlisted content. Replies with an unresolved source or receipt cannot be learned as ordinary source-free text.

## Visual memory and withdrawal

The host decides when to look and owns camera drivers, operating-system permission, UI indicators, pre-send sensitive-media interception, metadata removal, compression, audio removal, and recipient consent. Raw temporary media remains in memory before memory admission. Beetle uses an authorized `BeetleVisualInferenceClient` to interpret the media and decide whether an ordinary view merits a semantic candidate or whether restricted media evidence is needed. The host cannot provide the final fact or memory decision.

`list_visual_memory`, `read_visual_memory_media`, `forget_visual_memory`, and `forbid_visual_source_memory` expose governed inspection, media access, deletion, and source-level memory prohibition. Source lineage covers visual episodes, affected Transcript and Session replay, long-term facts, and Soul or relationship growth. Revocation, deletion, and prohibition invalidate dependent sources. An ordinary home background is not sensitive by itself; typed rules constrain third parties, minors, private content, and uncertain attribution. Face recognition is off by default. Capability state and revocation are scoped by host, device, and generation.

Visual evidence may contribute to subject growth only when its source is outside a relationship, spans at least two original turns, and is paired with an independent canonical user or runtime text observation in the current turn. A single image, the vision model's own description, or an assistant reply cannot change the Soul by itself. Relationship-scoped visual evidence contributes only to that relationship.

Cross-relationship replay checks the currently mounted relationship. A Transcript message from one relationship cannot be presented as the current conversation in another; only an admitted user-scoped long-term fact may cross that boundary.

## Store and archive

The Store schema is **v18** and requires its exact identity. There is no v17 compatibility reader or automatic migration; opening an older Store must fail without changing its data. Archive import is not a schema migration path.

`export_memory_space` exports source-bound **semantic** visual records by default, without media bytes. `export_memory_space_with_visual_media` explicitly includes eligible media under the private-material policy, exact owner and SHA checks, scope, and budget. `import_memory_space` atomically replaces JSON and admitted media while checking local tombstones, revocation, memory prohibition, and stricter current retention. An archive cannot silently remove or rewrite an existing visual episode or its dependency root. User-facing visual memory inspection is separate from ordinary subject projection and operator raw inspection.

Cross-device synchronization must carry source revocations, memory prohibitions, and tombstone authority first. A fresh Store that never received those controls cannot infer a remote deletion from an older archive. This release does not provide a cross-device synchronization service.

## Verification boundary

This source release is scoped to macOS SDK targeted contracts and source identity. Live camera operation, visual Providers, the host's complete final-request receipt, downstream UI, and multi-device hardware behavior require separate host integration acceptance. The source API alone does not establish that those host steps have occurred.
