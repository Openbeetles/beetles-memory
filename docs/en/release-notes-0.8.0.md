# Beetle Memory 0.8.0 Source Release Notes

This document describes the 0.8.0 source contract. The release is identified by the `v0.8.0`
Git tag and its exact commit; a working-tree version number does not prove publication.
The previous source contract is [0.7.0](release-notes-0.7.0.md).

## Learning evidence and declaration authority

Procedural input uses `PostTurnLearningInputV2`; tool feedback uses `AgentToolUsageFeedbackV3`.
Execution facts and method evidence are separate: facts contain typed outcomes and bounded call
references, not arbitrary result bodies or error summaries. Methods have canonical content,
source classifications and exact execution references. Model proposals, user declarations and
execution witnesses remain distinct. Tool success does not prove task completion or method quality.
Method activation and Runtime Skill promotion retain their separate evidence requirements.

Trusted host initialization explicitly controls durable producer bindings and issues non-serializable
`MemoryProceduralSubmissionCapability` values through the public SDK. Feedback uses `finalize_turn_with_procedural_evidence`;
ordinary no-feedback turns still use `finalize_turn` without a producer grant.
Bearer identity, operation permission and selection receipts do not independently grant declaration
authority. Remote payloads cannot carry the capability. Hosts must not maintain another permission
policy in their UI, adapter or database.

Valid execution facts with rejected methods may be partially accepted. Consumers must inspect
`partially_accepted_count` and `method_dispositions`, not treat submission success as acceptance of
every item. Delivery retries reuse the exact canonical payload; new executions use new call identities.

## Withdrawal, recovery and reads

Narrowed/revoked producer permissions and source visibility changes immediately restrict affected
current/as-of reads; the existing official learning service performs bounded reconciliation.
Unavailable reads distinguish states such as `Reconciling` and `Blocked`. Unread statistics are
`None`, not fabricated zeros or permission to display stale method bodies.

Capacity blocks require explicitly authorized recovery after the resource issue is resolved.
Wake and reopen do not clear them automatically. `procedural_reconciliation_status` discovers the exact
durable recovery reference after a full reopen without retaining an old job report. Entry attachment
status consumes that same SDK owner. Capacity exhaustion is not reported as Store corruption.
Permanent deletion, producer revocation and retired Skills are not reversed by retries,
reconciliation or restoring an older archive.
Public archives do not transfer source-Store producer authority or internal derived audit events.
When protected target learning sources cannot be preserved, restore returns a typed rejection
without partial writes. See [archive boundaries](replay-and-archive.md).

## Breaking Store and integration contracts

The 0.8.0 source accepts only Store v14. Procedural evidence/job/index/ledger/receipt use V2,
Agent Tool material/head use V3, and Runtime Skill owner records use schema 2.
Long-term material v5 and Adapter V2 retain their responsibilities.
Older stores are rejected: no automatic migration, silent empty-store replacement, data deletion
or compatibility reader. Owners must explicitly recreate disposable development data if desired.
Rollback pairs the older source with its corresponding intact older Store.

Embedded consumers attach `bm-entry::MemoryLearningService` to an existing `Arc<MemoryRuntime>`;
Entry-created runtimes use the same service. Store, registry, MemorySpace and control capabilities
must match. The host remains the one owner of model configuration and keys, without a second
product-facing governance-model configuration. See the [compilable integration example](integration.md#compilable-official-learning-service-example)
and [public API](api.md).

## Release and verification boundaries

Automated contracts, synthetic persistent reopen and
static platform checks do not establish real Provider, GUI/UAT, real-data, trusted Linux dynamic
quality or external benchmark validation. The intended delivery is a source release, not crates.io
publication, installers, signing/notarization, hosted Releases or deployment.
Release identity requires the exact tag and verified remote receipts, independently of these notes.
