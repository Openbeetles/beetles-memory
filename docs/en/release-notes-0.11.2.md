# Beetle Memory 0.11.2 Source Release Notes

0.11.2 fixes exact replay of committed procedural learning turns after a method revision advances or a producer is revoked. The `v0.11.2` source tag and its commit identify this release.

An exact replay must match the canonical turn, Store-private raw input commitment, historical sealed producer binding, mounted subject, and Store incarnation. When both durable intents already exist, it returns their existing identities without a Store write or worker wake. A committed turn with a missing intent enters a separate recovery path: it validates the canonical duplicate first, then requires current producer and source authority with exact Store preconditions before creating any missing intent. Changed input, withdrawn raw content, or withdrawn authority cannot create work through replay.

Public APIs, Store v16, long-term material v6, control revision v4, and Adapter V2 are unchanged. The [0.11.0 control integrity contract](release-notes-0.11.0.md) still applies. There is no automatic migration for older Stores. Use the same source tag for every Beetle crate.

The targeted SDK intake, producer revocation, source authority, method revision, raw input commitment, and missing-intent recovery contracts passed, together with formatting and static checks. Windows/Linux/ESP integration, real data, Provider, and GUI remain unverified. This is a Git source and tag release only; no crates.io upload, hosted Release, installer, or deployment was made.
