#![cfg(all(feature = "embedded-store", feature = "sqlite-store"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use bm_core::feature_gate::ProfileId;
use bm_core::memory::{
    canonical_recall_evidence_group, governed_evidence_document_content_digest,
    governed_evidence_source_ref_from_document, plan_governed_evidence_document_upsert,
    scoped_governed_evidence_document_key, GovernedEvidenceDocument, GovernedEvidenceDocumentChunk,
    GovernedEvidenceDocumentDraft, GovernedEvidenceDocumentPlan,
    GovernedEvidenceDocumentSourceKind, GovernedEvidenceSourceRef, MemoryEvidenceAuthority,
    MemoryPrivacyClass,
};
use bm_sdk::nonproduction_replay_harness::{
    EmbeddedStoreEngine, FileStoreEngine, GovernedEvidenceOwnerClaimBinding,
    GovernedEvidenceSourceClaimManifest, InMemoryStoreEngine, SqliteStoreEngine,
    StoreBackendConfig, StoreCapacityBudget, StoreEngine,
    GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
};
use bm_sdk::{
    MemoryCapabilityPolicy, MemoryClock, MemoryEvidenceDocumentMutation,
    MemoryEvidenceDocumentReadRequest, MemoryIdentity, MemoryPrivacyPolicy, MemoryRuntime,
    MemoryScope, MemoryStoreHandle, MemoryWriteRequest, NoopMemoryAuditSink,
};
use serde_json::{json, Value};

const MEMORY_SPACE_ID: &str = "space:owner-default";
const MOUNTED_SUBJECT_ID: &str = "agent:agent-main";
const OWNER_NAMESPACE: &str = "governed_evidence_documents";
const CLAIM_NAMESPACE: &str = "governed_evidence_source_refs";
const BLOB_NAMESPACE: &str = "state_fs";

type StoreAddress = (String, String);
type StoreAddresses = Vec<StoreAddress>;

struct FixedClock;

impl MemoryClock for FixedClock {
    fn now_secs(&self) -> u64 {
        1_800_000_000
    }
}

struct EngineCase {
    name: &'static str,
    engine: Box<dyn StoreEngine>,
    cleanup_root: Option<PathBuf>,
}

struct RuntimeCase {
    name: &'static str,
    store: MemoryStoreHandle,
    cleanup_root: Option<PathBuf>,
}

#[derive(Clone)]
struct GovernedFixture {
    manifest: GovernedEvidenceSourceClaimManifest,
    owners: Vec<GovernedEvidenceDocument>,
    claims: Vec<GovernedEvidenceSourceRef>,
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "bm-governed-evidence-exact-read-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos()
    ))
}

fn draft(document_id: &str, source_revision: u64) -> GovernedEvidenceDocumentDraft {
    let source_locator = format!("opaque://exact-read/{document_id}");
    let canonical_evidence_group =
        canonical_recall_evidence_group(&format!("exact_read:{document_id}"));
    let body = format!("Exact governed evidence body for {document_id}.");
    let chunks = vec![GovernedEvidenceDocumentChunk {
        identity: "section:exact-read".to_string(),
        ordinal: 0,
        body: body.clone(),
    }];
    GovernedEvidenceDocumentDraft {
        memory_space_id: MEMORY_SPACE_ID.to_string(),
        mounted_subject_id: MOUNTED_SUBJECT_ID.to_string(),
        document_id: document_id.to_string(),
        source_kind: GovernedEvidenceDocumentSourceKind::StructuredMaterial,
        source_locator: source_locator.clone(),
        canonical_evidence_group: canonical_evidence_group.clone(),
        evidence_family_group: None,
        source_revision,
        body: body.clone(),
        chunks: chunks.clone(),
        content_digest: governed_evidence_document_content_digest(
            &source_locator,
            &canonical_evidence_group,
            None,
            &body,
            &chunks,
        ),
        authority: MemoryEvidenceAuthority::WorldObservation,
        privacy: MemoryPrivacyClass::SharedWithSubject,
        observed_at: 1_799_999_900 + source_revision,
    }
}

fn governed_fixture() -> GovernedFixture {
    let owners = [("evidence:exact-read:a", 1), ("evidence:exact-read:b", 2)]
        .into_iter()
        .map(|(document_id, revision)| {
            match plan_governed_evidence_document_upsert(
                None,
                &draft(document_id, revision),
                1_800_000_000,
            ) {
                GovernedEvidenceDocumentPlan::Created(owner) => owner,
                other => panic!("fixture owner must be created: {other:?}"),
            }
        })
        .collect::<Vec<_>>();
    let claims = owners
        .iter()
        .map(|owner| governed_evidence_source_ref_from_document(owner).expect("source claim"))
        .collect::<Vec<_>>();
    let bindings = owners
        .iter()
        .zip(&claims)
        .map(|(owner, claim)| {
            GovernedEvidenceOwnerClaimBinding::from_document_claim(owner, claim)
                .expect("owner claim binding")
        })
        .collect::<Vec<_>>();
    let manifest = GovernedEvidenceSourceClaimManifest::build(
        MEMORY_SPACE_ID,
        MOUNTED_SUBJECT_ID,
        bindings,
        16,
    )
    .expect("governed manifest");
    GovernedFixture {
        manifest,
        owners,
        claims,
    }
}

fn engine_cases() -> Vec<EngineCase> {
    let capacity = StoreCapacityBudget::full();
    let file_root = temp_root("file-engine");
    let file_config = StoreBackendConfig::file(
        &file_root,
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("file config");
    let (file, _, _) =
        FileStoreEngine::open_with_capacity(&file_config, capacity).expect("file engine");

    let sqlite_root = temp_root("sqlite-engine");
    std::fs::create_dir_all(&sqlite_root).expect("sqlite root");
    let sqlite_config = StoreBackendConfig::sqlite(
        sqlite_root.join("memory.sqlite3"),
        ProfileId::native_dev_full().expect("native dev-full profile"),
    )
    .expect("sqlite config");
    let (sqlite, _) =
        SqliteStoreEngine::open_with_capacity(&sqlite_config, capacity).expect("sqlite engine");

    vec![
        EngineCase {
            name: "in-memory",
            engine: Box::new(InMemoryStoreEngine::new(capacity)),
            cleanup_root: None,
        },
        EngineCase {
            name: "embedded",
            engine: Box::new(EmbeddedStoreEngine::new(capacity)),
            cleanup_root: None,
        },
        EngineCase {
            name: "file",
            engine: Box::new(file),
            cleanup_root: Some(file_root),
        },
        EngineCase {
            name: "sqlite",
            engine: Box::new(sqlite),
            cleanup_root: Some(sqlite_root),
        },
    ]
}

fn runtime_cases() -> Vec<RuntimeCase> {
    let file_root = temp_root("file-runtime");
    let sqlite_root = temp_root("sqlite-runtime");
    std::fs::create_dir_all(&sqlite_root).expect("sqlite runtime root");
    let native = ProfileId::native_dev_full().expect("native dev-full profile");
    let configs = [
        (
            "in-memory",
            StoreBackendConfig::in_memory(native).expect("in-memory config"),
            None,
        ),
        (
            "embedded",
            StoreBackendConfig::embedded(ProfileId::EspEmbeddedSdk).expect("embedded config"),
            None,
        ),
        (
            "file",
            StoreBackendConfig::file(&file_root, native).expect("file config"),
            Some(file_root),
        ),
        (
            "sqlite",
            StoreBackendConfig::sqlite(sqlite_root.join("memory.sqlite3"), native)
                .expect("sqlite config"),
            Some(sqlite_root),
        ),
    ];
    configs
        .into_iter()
        .map(|(name, config, cleanup_root)| RuntimeCase {
            name,
            store: MemoryStoreHandle::open_for_nonproduction_harness(config)
                .unwrap_or_else(|error| panic!("open {name} runtime store: {error}")),
            cleanup_root,
        })
        .collect()
}

fn runtime(store: MemoryStoreHandle, case_name: &str) -> MemoryRuntime {
    MemoryRuntime::builder()
        .identity(MemoryIdentity::new("agent-main", "owner-default").expect("identity"))
        .scope(MemoryScope::new("test", format!("exact-read-{case_name}")).expect("scope"))
        .store(store)
        .clock(Arc::new(FixedClock))
        .capability_policy(MemoryCapabilityPolicy::strict_profile())
        .privacy_policy(MemoryPrivacyPolicy::standard_private_boundary())
        .audit_sink(Arc::new(NoopMemoryAuditSink))
        .build()
        .unwrap_or_else(|error| panic!("build {case_name} runtime: {error}"))
}

fn closure_addresses(fixture: &GovernedFixture) -> (StoreAddresses, StoreAddresses) {
    let missing_owner_key =
        scoped_governed_evidence_document_key(MEMORY_SPACE_ID, "evidence:exact-read:missing")
            .expect("missing owner key");
    let mut json = vec![(
        GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE.to_string(),
        fixture.manifest.physical_key.clone(),
    )];
    json.extend(
        fixture
            .manifest
            .owner_keys
            .iter()
            .cloned()
            .map(|key| (OWNER_NAMESPACE.to_string(), key)),
    );
    json.extend(
        fixture
            .manifest
            .claim_keys
            .iter()
            .cloned()
            .map(|key| (CLAIM_NAMESPACE.to_string(), key)),
    );
    json.push((OWNER_NAMESPACE.to_string(), missing_owner_key));
    let blobs = vec![(BLOB_NAMESPACE.to_string(), "exact.bin".to_string())];
    (json, blobs)
}

fn seed_engine(engine: &dyn StoreEngine, fixture: &GovernedFixture) {
    engine
        .put_json_value(
            GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE,
            &fixture.manifest.physical_key,
            serde_json::to_value(&fixture.manifest).expect("serialize manifest"),
        )
        .expect("seed manifest");
    for owner in &fixture.owners {
        engine
            .put_json_value(
                OWNER_NAMESPACE,
                &owner.physical_key,
                serde_json::to_value(owner).expect("serialize owner"),
            )
            .expect("seed owner");
    }
    for claim in &fixture.claims {
        engine
            .put_json_value(
                CLAIM_NAMESPACE,
                &claim.physical_key,
                serde_json::to_value(claim).expect("serialize claim"),
            )
            .expect("seed claim");
    }
    engine
        .put_json_value("test_exact_read", "json-plus-one", json!(0))
        .expect("seed one-byte JSON sentinel");
    engine
        .put_blob(BLOB_NAMESPACE, "exact.bin", b"exact")
        .expect("seed exact blob");
    engine
        .put_blob(BLOB_NAMESPACE, "blob-plus-one.bin", b"x")
        .expect("seed one-byte blob sentinel");
}

#[test]
fn four_backends_return_the_same_exact_governed_evidence_view() {
    let fixture = governed_fixture();
    let expected_ids = fixture
        .owners
        .iter()
        .take(1)
        .map(|owner| owner.document_id.clone())
        .collect::<Vec<_>>();

    for case in runtime_cases() {
        let case_name = case.name;
        let runtime = runtime(case.store.clone(), case_name);
        for (index, document_id) in expected_ids.iter().enumerate() {
            runtime
                .write(MemoryWriteRequest::GovernedEvidenceDocuments {
                    mutations: vec![MemoryEvidenceDocumentMutation::Upsert {
                        draft: Box::new(draft(document_id, index as u64 + 1)),
                    }],
                })
                .unwrap_or_else(|error| panic!("write governed evidence on {case_name}: {error}"));
        }

        let report = runtime
            .read_governed_evidence_documents(MemoryEvidenceDocumentReadRequest {
                memory_space_id: MEMORY_SPACE_ID.to_string(),
                document_ids: expected_ids.clone(),
            })
            .unwrap_or_else(|error| panic!("exact governed read on {case_name}: {error}"));

        assert!(report.store_snapshot_consistent, "{case_name}");
        assert_eq!(
            report
                .documents
                .iter()
                .map(|document| document.owner_ref.owner_id.clone())
                .collect::<Vec<_>>(),
            expected_ids,
            "{case_name}"
        );
        assert!(report.missing_document_ids.is_empty(), "{case_name}");
        let missing = runtime
            .read_governed_evidence_documents(MemoryEvidenceDocumentReadRequest {
                memory_space_id: MEMORY_SPACE_ID.to_string(),
                document_ids: vec!["evidence:exact-read:missing".to_string()],
            })
            .unwrap_or_else(|error| panic!("exact missing read on {case_name}: {error}"));
        assert!(missing.store_snapshot_consistent, "{case_name}");
        assert!(missing.documents.is_empty(), "{case_name}");
        assert_eq!(
            missing.missing_document_ids,
            vec!["evidence:exact-read:missing".to_string()],
            "{case_name} must return explicit absence"
        );

        let snapshot = case
            .store
            .replay_harness()
            .export_store_snapshot()
            .unwrap_or_else(|error| panic!("export {case_name} snapshot: {error}"));
        let manifest = snapshot
            .json_docs
            .iter()
            .find(|doc| doc.namespace == GOVERNED_EVIDENCE_SOURCE_CLAIM_MANIFEST_NAMESPACE)
            .unwrap_or_else(|| panic!("{case_name} manifest"));
        let manifest: GovernedEvidenceSourceClaimManifest =
            serde_json::from_value(manifest.value.clone()).expect("typed manifest");
        let actual_owner_keys = snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == OWNER_NAMESPACE)
            .map(|doc| doc.key.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let actual_claim_keys = snapshot
            .json_docs
            .iter()
            .filter(|doc| doc.namespace == CLAIM_NAMESPACE)
            .map(|doc| doc.key.clone())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            manifest
                .owner_keys
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            actual_owner_keys,
            "{case_name} owners must come from the same manifest closure"
        );
        assert_eq!(
            manifest
                .claim_keys
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            actual_claim_keys,
            "{case_name} claims must be manifest-derived"
        );
        let cleanup_root = case.cleanup_root.clone();
        drop(runtime);
        drop(case);
        if let Some(root) = cleanup_root {
            std::fs::remove_dir_all(root).expect("remove runtime backend root");
        }
    }
}

#[test]
fn four_backend_receipts_cover_manifest_owners_manifest_claims_and_exact_absence() {
    let fixture = governed_fixture();
    let (json_addresses, blob_addresses) = closure_addresses(&fixture);
    let expected_json_docs = 1 + fixture.owners.len() + fixture.claims.len();
    let expected_json_bytes = serde_json::to_vec(&fixture.manifest)
        .expect("manifest bytes")
        .len()
        + fixture
            .owners
            .iter()
            .map(|owner| serde_json::to_vec(owner).expect("owner bytes").len())
            .sum::<usize>()
        + fixture
            .claims
            .iter()
            .map(|claim| serde_json::to_vec(claim).expect("claim bytes").len())
            .sum::<usize>();
    let expected_entry_count = json_addresses.len() + blob_addresses.len();
    let mut parity_receipt = None;

    for case in engine_cases() {
        seed_engine(case.engine.as_ref(), &fixture);
        let mut exact_capacity = StoreCapacityBudget::full();
        exact_capacity.kv_max_entries = expected_entry_count;
        exact_capacity.snapshot_max_bytes = expected_json_bytes;
        exact_capacity.blob_max_bytes = b"exact".len();

        let exact = case
            .engine
            .read_consistent_known_keys(&json_addresses, &blob_addresses, false, exact_capacity)
            .unwrap_or_else(|error| panic!("{} exact receipt: {error}", case.name));
        assert_eq!(
            exact.receipt.entry_count, expected_entry_count,
            "{}",
            case.name
        );
        assert_eq!(
            exact.receipt.json_doc_count, expected_json_docs,
            "{}",
            case.name
        );
        assert_eq!(exact.receipt.blob_count, 1, "{}", case.name);
        assert_eq!(
            exact.receipt.json_bytes, expected_json_bytes,
            "{}",
            case.name
        );
        assert_eq!(exact.receipt.blob_bytes, b"exact".len(), "{}", case.name);
        assert_eq!(
            exact.json.last().and_then(|read| read.value.as_ref()),
            None,
            "{} must retain the requested absent owner address",
            case.name
        );
        assert_eq!(
            exact
                .json
                .iter()
                .map(|read| (read.namespace.clone(), read.key.clone()))
                .collect::<Vec<_>>(),
            json_addresses,
            "{} must return only the requested addresses in order",
            case.name
        );
        if let Some(expected) = &parity_receipt {
            assert_eq!(&exact.receipt, expected, "{} receipt parity", case.name);
        } else {
            parity_receipt = Some(exact.receipt.clone());
        }

        let mut entry_plus_one = json_addresses.clone();
        entry_plus_one.push((OWNER_NAMESPACE.to_string(), "absent-plus-one".to_string()));
        let error = case
            .engine
            .read_consistent_known_keys(&entry_plus_one, &blob_addresses, false, exact_capacity)
            .expect_err("entry +1 must fail closed");
        assert_eq!(
            error.stage(),
            "store_consistent_read_budget_exceeded",
            "{}",
            case.name
        );

        let mut json_plus_one = json_addresses.clone();
        json_plus_one.push(("test_exact_read".to_string(), "json-plus-one".to_string()));
        let mut json_capacity = exact_capacity;
        json_capacity.kv_max_entries = StoreCapacityBudget::full().kv_max_entries;
        let error = case
            .engine
            .read_consistent_known_keys(&json_plus_one, &blob_addresses, false, json_capacity)
            .expect_err("JSON +1 byte must fail closed");
        assert_eq!(
            error.stage(),
            "store_consistent_read_budget_exceeded",
            "{}",
            case.name
        );

        let mut blob_plus_one = blob_addresses.clone();
        blob_plus_one.push((BLOB_NAMESPACE.to_string(), "blob-plus-one.bin".to_string()));
        let mut blob_capacity = exact_capacity;
        blob_capacity.kv_max_entries = StoreCapacityBudget::full().kv_max_entries;
        let error = case
            .engine
            .read_consistent_known_keys(&json_addresses, &blob_plus_one, false, blob_capacity)
            .expect_err("blob +1 byte must fail closed");
        assert_eq!(
            error.stage(),
            "store_consistent_read_budget_exceeded",
            "{}",
            case.name
        );

        let cleanup_root = case.cleanup_root.clone();
        drop(case.engine);
        if let Some(root) = cleanup_root {
            std::fs::remove_dir_all(root).expect("remove engine backend root");
        }
    }
}

#[test]
fn legacy_v1_and_unknown_manifest_shapes_fail_closed() {
    let fixture = governed_fixture();
    let bindings = fixture.manifest.owner_claim_bindings.clone();

    let mut legacy_v1 = serde_json::to_value(&fixture.manifest).expect("manifest JSON");
    legacy_v1["schema_version"] = json!(1);
    let legacy_v1: GovernedEvidenceSourceClaimManifest =
        serde_json::from_value(legacy_v1).expect("legacy version still has a typed shape");
    assert!(legacy_v1
        .validate_exact(MEMORY_SPACE_ID, MOUNTED_SUBJECT_ID, bindings.clone(), 16)
        .is_err());

    let mut unknown = serde_json::to_value(&fixture.manifest).expect("manifest JSON");
    unknown["unknown_owner_projection"] = Value::Bool(true);
    assert!(serde_json::from_value::<GovernedEvidenceSourceClaimManifest>(unknown).is_err());
}

#[test]
fn crate_private_exact_reader_contract_rejects_wrong_or_extra_returned_addresses() {
    let source = include_str!("../../sdk/src/store_internal/transaction.rs");
    assert!(source.contains("reads.len() != addresses.len()"));
    assert!(source.contains("&read.namespace != namespace || &read.key != key"));
    assert!(
        source.contains("immutable read session returned an extra, missing, or wrong JSON address")
    );
    assert!(source.contains("manifest address was not returned explicitly"));
}
