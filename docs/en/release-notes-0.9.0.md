# Beetle Memory 0.9.0 Source Candidate Notes

This document describes the 0.9.0 source candidate, following [0.8.0](release-notes-0.8.0.md).
A working-tree version is not publication: release identity requires a `v0.9.0` Git tag and its
verified commit. This candidate combines the source-read proof repair and governed subject
projection fixes. It does not include comparative learning or a new tool executor.

## Shared-source proof repair

A valid read of a MemorySpace-owned source may require an operation proof produced by a different
subject in the same space. The SDK now derives the exact source address and dependency root from
the durable Store pre-image before admitting that read. It no longer misclassifies this valid path
as `procedural_source_closure_repair_required`. Foreign-space identities, noncanonical addresses,
caller-proposed images and unrelated operation receipts remain rejected. Read authority does not
grant permission to modify another subject's source or expand mutation fanout.

## Governed subject projection

All six accepted OuterVoice dimensions reach typed expression guidance. Expression remains style,
not identity, tool permission or disclosure authority; relationship constraints may narrow it.
Partial founding charters no longer borrow internal display Markdown to invent a reply stance,
initiative or boundary mode. An unseeded subject does not acquire a fabricated personality.

FeltSignificance, TemperamentContinuity and InnerConflict derived state participates only in the
permitted private runtime context. Subject, relationship, generation, current/history and profile
checks still apply. Optional expression/private blocks are omitted as whole units when the budget
cannot fit them; reports describe the actual delivered payload rather than an earlier preview.

Relationship runtime inheritance now comes from the exact verified Soul material. Truth, mutual
boundaries, repair and self-preservation clauses remain relationship constraints and are not
mistaken for personality priorities. An unseeded relationship reports no core revision. Protected
relationship text and expression guidance are excluded from public UI/operator/gateway audit surfaces.
These controls do not claim to prevent every possible semantic disclosure by a model.

## Breaking public API changes

- `LLMRuntimeProjectionEnvelope` adds `expression_guidance: Option<SubjectExpressionDiscipline>`.
  Its `runtime_constraints` changes from `Vec<String>` to `Vec<RuntimeProjectionSourceBlock>`.
- Core `InhabitedSubjectProjection` also adds `expression_guidance`.
- `SubjectSoulRelationshipRuntimeViewV1` adds `inherited_core_revision`,
  `inherited_priority_constitution`, `inherited_response_mode`, `inherited_initiative_posture`
  and `inherited_relationship_posture`. Its serialized runtime-view shape changes too; this
  ephemeral view is not a persisted Soul revision material or an archive format.
- `SubjectExpressionDiscipline` is exported by Core and SDK. It consumes existing OuterVoice and
  typed relationship ceilings rather than defining another persistent personality model.

Consumers using explicit struct literals must update them. Consumers iterating constraints now
receive source blocks with `.content` and source/protection metadata; do not discard that metadata
and expose raw runtime text. Do not construct an alternate renderer or a compatibility fallback.
Prefer `projection.provider_payload().system_memory_block()` for model delivery and
`projection.report().ui_api_projection()`, `.operator_projection()` or `.gateway_audit()` for the
corresponding safe consumer. Do not log or display the raw envelope through `Debug`.

## Store and learning integration

Store v14 and long-term material v5 are unchanged from 0.8.0; Adapter V2 remains current.
There is no Store migration or data-rebuild requirement introduced by this candidate.
Store v13 and earlier remain rejected by the existing admission rules: no automatic migration,
compatibility reader or silent deletion. This release preparation does not touch real data.
Before an actual deployment, preserve an exact Store backup outside the data path; rollback uses
the previously validated source and its corresponding intact Store backup, not an archive import.

The existing `PostTurnLearningInputV2`, `AgentToolUsageFeedbackV3`,
`MemoryProceduralSubmissionCapability` and `procedural_reconciliation_status` contracts remain.
Ordinary turns use `finalize_turn`; authorized evidence uses `finalize_turn_with_procedural_evidence`.
Embedded consumers continue to attach `bm-entry::MemoryLearningService` to the same Runtime,
Store and registry. Hosts retain the single configuration/credential owner; no second governance
configuration or worker is required. See [integration](integration.md) and [API](api.md).

## Delivery boundary

The candidate is intended for source/tag delivery. Package and `cargo publish --dry-run` checks
are packaging evidence only, never a crates.io upload. Installers, signing/notarization, hosted
Releases and deployment are separate. Synthetic models, File/SQLite reopen and local HTTP capture
do not establish real Provider quality, GUI/UAT, real-data safety, trusted Linux dynamic quality
or external benchmark results. See the [release checklist](release-checklist.md).
