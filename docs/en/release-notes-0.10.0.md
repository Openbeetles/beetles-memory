# Beetle Memory 0.10.0 Source Release Notes

0.10.0 completes production method learning, governed persona growth and user-device local time.
The release identity is the `v0.10.0` source tag and its commit; package checks do not constitute a
crates.io upload. See [integration](integration.md) and the [API](api.md) for consumer setup.

## Production method learning

`PostTurnLearningInputV3` accepts bounded, sourced learning material, actual method adoption and
independent task evaluations. The official `MemoryLearningService` / `MemoryLearningEngine`
extracts `ModelInferred` Candidates from authorized material. Models cannot grant producer
permissions, claim actual execution or mark their own task outcomes successful.

Use `discover_method_trials` for authorized Candidate trials and `MethodContentRefV1::from_selection`
for exact references from normal method delivery. Selection, adoption, tool execution and task
success remain distinct. Promotion requires actual adoption plus independently authorized task
evidence; repeated observations of one task are not independent successes. A still-valid Failed
outcome cannot be erased by revoking an unrelated declaration capability.

Source withdrawal, correction and raw deletion invalidate affected current and historical reads.
Raw deletion removes source-dependent method and skill content; independently authored skills are
not deleted solely because an unrelated usage witness is removed. Retained body-free receipts
continue binding exact job input and immutable execution configuration after content is deleted.
Missing or contradictory authority requires repair, not a rehashed or silently reconstructed proof.

## Governed persona growth

Delivered canonical turns with signed Soul admission can become source-bound observations in the
existing subject generation. The model interprets evidence; it cannot choose a subject, generation,
revision or confirmation status. Stable independent observations feed existing SelfModel/Core
revision governance. Natural value orientation and the canonical governance priority order are
separate dimensions. Task/tool habits do not become personality, and missing evidence does not
invent a personality or relationship.

Source review holds affected Soul/private projection while independent lawful shared facts remain
usable. A real subsequent turn can support a governed correction within the same generation.
Restoring current access never revives denied historical revisions. Reset cannot reauthorize an
old signed turn or queued observation in the new generation. Relationship boundaries and private
runtime/report separation remain enforced.

## User-device local time

`UserLocalTimeProvider` supplies the user's local time and time zone to foreground and background
model context. Desktop profiles use the device OS zone by default and resolve the offset for each
instant. Remote/server/device profiles need a trusted user-scoped provider; they do not assume the
server's zone is the user's. `None` omits the complete field, with no guessed UTC, stale clock or
unlabelled epoch. Persistent timestamps, lease/CAS and ordering continue using UTC instants.

## Breaking contracts and Store admission

- Store v15 is required. Store v14 and older are rejected without writes or silent replacement;
  there is no automatic migration, compatibility reader or dual write.
- Input/evidence V3, semantic Job V4/scope index/receipt V4, method material V4 and application
  ledger V3 carry the new provenance and atomic completion contracts. Projection receipt V2,
  Soul schema 2, source-dependents schema 2 and reconciliation checkpoint schema 2 enforce exact
  generation/source/task closure. Long-term material v5 and Adapter V2 remain unchanged.
- Consumers must update producer claims, evaluator definitions and explicit struct literals.
  `MemoryProceduralSubmissionCapability` is still required for authorized procedural submissions;
  ordinary turns use `PostTurnLearningInputV3::empty()` and `finalize_turn` without a producer grant.
- Persona `Growth.value_orientation` is explicit; canonical priority tokens cannot be replaced by
  free-form values. Callers cannot write a private evidence ledger or supply governance authority.
- SubjectShell no longer exposes a separate unlabelled current epoch. Use the SDK's typed local-time
  provider and existing projection renderer, never a second host clock-rendering layer.

Open a new synthetic/development Store for this generation. Preserve existing data separately;
this source release does not delete or migrate it. Any real-data replacement remains an explicit
operator action. Rollback uses the previous source tag and its matching intact Store; neither old
code opening Store v15 nor archive import is a schema rollback mechanism.

## Official service integration

Attach the official service to the same Runtime, Store and SubjectRegistry, or consume the
Entry-created runtime. Reuse the host's single configuration and credential resolver. Networked
model execution requires `bm-entry/governance-model-client-std`; configuring a key alone cannot
supply a compiled-out execution port. Missing bindings/credentials retain durable work for lawful
recovery. Hosts supply facts and credentials, not a second worker, policy store or promotion engine.
`procedural_reconciliation_status` remains the recovery discovery API.

## Verification and delivery boundary

Automated contracts cover formal SDK/Entry consumers, InMemory/File/SQLite reopen, exact current/as-of
reads, revocation/reset/deletion, privacy, task evidence, source corruption and local time. Synthetic
model execution and loopback HTTP verify integration, not real Provider learning quality. Strict
platform compilation is not trusted Linux dynamic execution or GUI/UAT. Full comparative learning
experiments, real-data migration, external benchmarks, hosted Releases, installers and deployment
are not part of this source release. Packaging and `cargo publish --dry-run` never upload packages.
