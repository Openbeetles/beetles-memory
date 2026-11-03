# Beetle Memory 0.11.0 Source Release Notes

0.11.0 closes long-term memory control integrity: complete filtered pagination, durable suppression
policy inspection and exact removal, atomic factual source withdrawal, and protection of explicit
corrections from older background learning. Release identity is the `v0.11.0` source tag and its commit.
See the [API](api.md) and [integration guide](integration.md).

## Long-term lists and suppression policies

Repeated durable owner identities within one intake batch are rejected atomically, rather than silently keeping the last value.

Long-term lists apply mounted-subject visibility before pagination. Cursors bind the query, subject
and snapshot; restart the query when a cursor becomes stale. Denied content and evidence remain hidden.

Inspect durable rules through `list_memory_governance_policies` / `get_memory_governance_policy`.
`mutate_memory_governance_policy(operation_id, request)` requires a stable operation ID. Removal takes
the returned `MemoryGovernancePolicyRef { policy_id, policy_revision }`, rather than a selector guess.
Success is `Committed`; retrying the same intent returns `Replayed`. Stale revisions, cross-subject
requests and operation ID reuse with a different intent are rejected. Rules remain manageable after
the original fact is deleted; an old removal request cannot delete a newly created matching rule.

Rules constrain future factual writes for the mounted subject. Sharing a HumanUser actor does not
share suppression scope between Agents. Omitted selector scope binds to the current Runtime, not all
subjects. Existing accepted facts do not become invisible merely because a rule exists.

## Source withdrawal and explicit corrections

Transcript `Mask` / `DeleteRaw` atomically withdraw derived factual evidence and its facet/graph links.
Current, as-of, final recall delivery and projection cannot revive withdrawn sources. Independent valid
evidence remains usable. Accepted factual content remains; deleting a fact requires explicit governance.

The canonical turn's `factual_admission_revision` records source admission order. Explicit
Correct/Supersede establishes an owner barrier that older queued or in-flight learning, retries and
dedupe deletion cannot bypass. Truly later sources can still support valid learning; this is not a
permanent fact freeze. `LongTermTransitionBasis` preserves transition authority. Explicit control
requires SDK-private authorization and cannot be forged by a host.

## Breaking API and Store boundary

- Only Store v16 is accepted, with immutable long-term material v6 and control revision v4. Store v15
  and earlier are rejected without writing. There is no automatic migration, permissive missing-field
  reader or dual write. This release neither migrates nor deletes real data.
- Policy mutation requires an operation ID; removal uses exact policy identity/revision. Policy
  list/detail are public SDK and thin-adapter operations. Adapter V2 remains the envelope version,
  not a promise of compatibility with the old policy payloads.
- Public `CanonicalTurnDelta` intake does not accept a host-supplied admission counter. Store assigns
  `factual_admission_revision` when persisting the first `TranscriptTurnRecord`. Tests or tools that
  explicitly construct persisted records or long-term material/control fixtures must update their
  required fields. Use official SDK intake/control so owner, head, material, index, audit, event
  and receipt are planned in one transaction.
- When integrating from source, use the same tag for every Beetle crate. Rollback requires the old source and its matching complete
  Store; archive import is not schema rollback or migration. Use a fresh synthetic development Store;
  handling real data is a separate operator action.

Production method learning, governed persona growth and user-device local time from 0.10.0 remain
available with their existing ownership boundaries.

## Validation and delivery limits

Automated contracts cover InMemory/File/SQLite, reopen, current/as-of, subject isolation, withdrawal,
stale-learning races, exact policy CAS/idempotency and unchanged state after failed transactions;
the macOS desktop contract is also checked. Strict cross-target compilation was run separately but
does not establish Windows/Linux/ESP integration. Synthetic models prove wiring, not real Provider
quality. Full comparative-learning experiments, real data, GUI/UAT, trusted Linux dynamic evidence,
external benchmarks, hosted Releases, installers and deployment are outside this source release.
The 13 crate publish dry-runs are deferred until crates.io publication is in scope; no upload occurred.
